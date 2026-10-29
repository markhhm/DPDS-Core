// SPDX-License-Identifier: Apache-2.0
//! Software-only production-side replay adapter. Contains no reference formulas.
use dpds_core::hng::{HngBuffer, HngConfig, HngPacket, HngStats, PushResult, TraceProfile};
use dpds_core::hng_engine::{
    hng_engine_has_backlog, hng_line_drain_advance, hng_line_end_input, hng_line_offer,
    hng_line_stats, hng_lines_pop_merged, Engine,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    error::Error,
    ffi::c_void,
    io::{self, BufRead, Write},
    mem::MaybeUninit,
    path::PathBuf,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyHeader {
    model: HngConfig,
    trace: Option<TraceProfile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    schema: String,
    config: Value,
    #[serde(default)]
    base_dir: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyEvent {
    ingress_seq: u64,
    arrival_ns: u64,
    length: u32,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Event {
    Packet {
        line: usize,
        ingress_port: u16,
        ingress_seq: u64,
        arrival_ns: u64,
        length: u32,
    },
    Frontier {
        ingress_port: u16,
        through_ns: u64,
        next_ingress_seq: u64,
        now_ns: u64,
    },
    End {
        ingress_port: u16,
        next_ingress_seq: u64,
        now_ns: u64,
    },
}

#[derive(Default)]
struct Port {
    next_seq: u64,
    arrival: u64,
    frontier: Option<u64>,
    ended: bool,
}

fn outcome(result: PushResult) -> &'static str {
    match result {
        PushResult::Accepted => "accepted",
        PushResult::QDrop => "q_drop",
        PushResult::TraceLoss => "trace_loss",
        PushResult::TraceOutage => "trace_outage",
        PushResult::StorageDrop => "storage_drop",
        PushResult::InvalidInput => "invalid_input",
    }
}

fn output(packet: HngPacket) -> Value {
    json!({"kind":"output", "ingress_seq":packet.ingress_seq,
        "selection_seq":packet.selection_seq, "service_ns":packet.service_ns,
        "deadline_ns":packet.deadline_ns, "release_ns":packet.release_ns,
        "late":packet.late, "clamp_extra_ns":packet.clamp_extra_ns,
        "l2_bytes":packet.l2_bytes, "wire_bytes":packet.wire_bytes})
}

fn pointer(seq: u64) -> Result<*mut c_void, Box<dyn Error>> {
    Ok(usize::try_from(seq.checked_add(1).ok_or("ID overflow")?)? as *mut c_void)
}

fn legacy(
    header: LegacyHeader,
    input: impl Iterator<Item = io::Result<String>>,
    out: &mut impl Write,
) -> Result<(), Box<dyn Error>> {
    let mut buffer = match header.trace {
        Some(trace) => HngBuffer::with_trace(header.model, trace)?,
        None => HngBuffer::new(header.model)?,
    };
    for (selection_seq, line) in input.enumerate() {
        let event: LegacyEvent = serde_json::from_str(&line?)?;
        let result = buffer.push_with_seq(
            pointer(event.ingress_seq)?,
            event.length,
            event.arrival_ns,
            event.ingress_seq,
        );
        writeln!(
            out,
            "{}",
            json!({"kind":"admission", "ingress_seq":event.ingress_seq,
            "selection_seq":selection_seq,
            "late":header.model.p.sample(header.model.seed, selection_seq as u64),
            "outcome":outcome(result), "q_bytes":buffer.stats().q_bytes,
            "q_drain_ns":buffer.q_drain_ns()})
        )?;
        while let Some(packet) = buffer.try_pop(event.arrival_ns, event.arrival_ns) {
            writeln!(out, "{}", output(packet))?;
        }
    }
    buffer.end_input();
    while let Some(packet) = buffer.try_pop(u64::MAX, u64::MAX) {
        writeln!(out, "{}", output(packet))?;
    }
    if buffer.has_backlog() {
        return Err("replay did not drain".into());
    }
    Ok(())
}

fn snapshots(engine: &Engine) -> Vec<Value> {
    engine
        .lines
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let mut stats = HngStats::default();
            // SAFETY: immutable live line, no concurrent mutation.
            unsafe { hng_line_stats(line, &mut stats) };
            json!({"line":index, "q_bytes":stats.q_bytes, "q_drain_ns":line.q_drain_ns(),
            "ledger_watermark_ns":stats.ledger_watermark_ns, "input_complete":stats.input_complete})
        })
        .collect()
}

fn drain(
    engine: &mut Engine,
    ports: &BTreeMap<u16, Port>,
    now_ns: u64,
    trigger: usize,
    out: &mut impl Write,
) -> io::Result<()> {
    let mut groups: BTreeMap<u16, Vec<usize>> = BTreeMap::new();
    for (i, line) in engine.lines.iter().enumerate() {
        groups.entry(line.info.egress_port).or_default().push(i);
    }
    for (egress_port, indices) in groups {
        if indices.iter().any(|&i| {
            let p = &ports[&engine.lines[i].info.ingress_port];
            !p.ended && p.frontier.is_none()
        }) {
            continue;
        }
        let watermarks: Vec<_> = indices
            .iter()
            .map(|&i| {
                let p = &ports[&engine.lines[i].info.ingress_port];
                if p.ended {
                    u64::MAX
                } else {
                    p.frontier.unwrap()
                }
            })
            .collect();
        let handles: Vec<_> = indices
            .iter()
            .map(|&i| std::ptr::from_mut(&mut engine.lines[i]))
            .collect();
        loop {
            let mut packet = MaybeUninit::<HngPacket>::uninit();
            // SAFETY: distinct live lines, matching watermark array, exclusive owner.
            let k = unsafe {
                hng_lines_pop_merged(
                    handles.as_ptr(),
                    watermarks.as_ptr(),
                    handles.len(),
                    now_ns,
                    now_ns,
                    packet.as_mut_ptr(),
                )
            };
            if k < 0 {
                break;
            }
            let index = indices[k as usize];
            // SAFETY: a nonnegative result initialized the packet.
            let mut row = output(unsafe { packet.assume_init() });
            row["line"] = json!(index);
            row["ingress_port"] = json!(engine.lines[index].info.ingress_port);
            row["egress_port"] = json!(egress_port);
            row["trigger"] = json!(trigger);
            row["now_ns"] = json!(now_ns);
            writeln!(out, "{row}")?;
        }
    }
    for line in &mut engine.lines {
        if ports[&line.info.ingress_port].ended {
            // SAFETY: this line has been explicitly sealed, exclusively owned.
            unsafe { hng_line_drain_advance(line, now_ns) };
        }
    }
    Ok(())
}

fn multiline(
    header: Header,
    input: impl Iterator<Item = io::Result<String>>,
    out: &mut impl Write,
) -> Result<(), Box<dyn Error>> {
    if header.schema != "hng-m2-replay-v1" {
        return Err("unsupported replay schema".into());
    }
    let mut engine = Engine::from_json_at(&header.config.to_string(), &header.base_dir)?;
    let models: Vec<HngConfig> = header.config["lines"]
        .as_array()
        .ok_or("missing lines")?
        .iter()
        .map(|line| serde_json::from_value(line["model"].clone()))
        .collect::<Result<_, _>>()?;
    let mut selections = vec![0u64; engine.lines.len()];
    let mut ports: BTreeMap<u16, Port> = engine
        .lines
        .iter()
        .map(|line| (line.info.ingress_port, Port::default()))
        .collect();
    for line in &engine.lines {
        if engine.lines.iter().any(|other| {
            other.info.egress_port == line.info.egress_port
                && other.info.ingress_port != line.info.ingress_port
        }) {
            return Err("shared egress requires one ingress source".into());
        }
    }
    let mut clock = 0;
    let mut trigger = 0;
    for (index, line) in input.enumerate() {
        trigger = index;
        let event: Event = serde_json::from_str(&line?)?;
        let (port_id, seq, now_ns, frontier) = match event {
            Event::Packet {
                line,
                ingress_port,
                ingress_seq,
                arrival_ns,
                length,
            } => {
                let port = ports.get_mut(&ingress_port).ok_or("unknown ingress port")?;
                if port.ended {
                    return Err("packet after end".into());
                }
                let target = engine.lines.get_mut(line).ok_or("unknown line")?;
                if target.info.ingress_port != ingress_port
                    || ingress_seq != port.next_seq
                    || arrival_ns < port.arrival
                    || port.frontier.is_some_and(|f| arrival_ns <= f)
                {
                    return Err("invalid packet history".into());
                }
                let selection_seq = selections[line];
                selections[line] = selection_seq.checked_add(1).ok_or("selection overflow")?;
                // SAFETY: live exclusive line; opaque IDs are never dereferenced.
                let result = unsafe {
                    hng_line_offer(
                        target,
                        pointer(ingress_seq)?,
                        length,
                        arrival_ns,
                        ingress_seq,
                    )
                };
                let mut stats = HngStats::default();
                unsafe { hng_line_stats(target, &mut stats) };
                writeln!(
                    out,
                    "{}",
                    json!({"kind":"admission", "line":line,
                    "ingress_port":ingress_port, "egress_port":target.info.egress_port,
                    "ingress_seq":ingress_seq, "selection_seq":selection_seq,
                    "late":models[line].p.sample(models[line].seed, selection_seq),
                    "outcome":outcome(result), "q_bytes":stats.q_bytes,
                    "q_drain_ns":target.q_drain_ns()})
                )?;
                port.next_seq = ingress_seq.checked_add(1).ok_or("sequence overflow")?;
                port.arrival = arrival_ns;
                continue;
            }
            Event::Frontier {
                ingress_port,
                through_ns,
                next_ingress_seq,
                now_ns,
            } => (ingress_port, next_ingress_seq, now_ns, Some(through_ns)),
            Event::End {
                ingress_port,
                next_ingress_seq,
                now_ns,
            } => (ingress_port, next_ingress_seq, now_ns, None),
        };
        if now_ns < clock || ports.values().any(|p| now_ns < p.arrival) {
            return Err("backward replay clock".into());
        }
        clock = now_ns;
        let port = ports.get_mut(&port_id).ok_or("unknown ingress port")?;
        if port.ended {
            return Err("marker after end".into());
        }
        if seq != port.next_seq {
            return Err("marker sequence mismatch".into());
        }
        if let Some(through) = frontier {
            if through < port.arrival
                || port.frontier.is_some_and(|f| through < f)
                || through > now_ns
            {
                return Err("invalid frontier".into());
            }
            port.frontier = Some(through);
        } else {
            port.ended = true;
            for line in &mut engine.lines {
                if line.info.ingress_port == port_id {
                    // SAFETY: marker certifies no more offers for these lines.
                    unsafe { hng_line_end_input(line) };
                }
            }
        }
        drain(&mut engine, &ports, now_ns, index, out)?;
        writeln!(
            out,
            "{}",
            json!({"kind":"marker", "trigger":index,
            "ingress_port":port_id, "marker":if frontier.is_some() { "frontier" } else { "end" },
            "lines":snapshots(&engine)})
        )?;
    }
    if ports.values().any(|p| !p.ended) {
        return Err("missing end marker".into());
    }
    drain(&mut engine, &ports, u64::MAX, trigger + 1, out)?;
    // SAFETY: the engine is live and exclusively owned after its final drain.
    if unsafe { hng_engine_has_backlog(&engine) } {
        return Err("replay did not drain".into());
    }
    writeln!(
        out,
        "{}",
        json!({"kind":"drained", "lines":snapshots(&engine)})
    )?;
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut input = io::stdin().lock().lines();
    let first = input.next().ok_or("missing header")??;
    let value: Value = serde_json::from_str(&first)?;
    let mut out = io::BufWriter::new(io::stdout().lock());
    if value.get("schema").is_some() {
        multiline(serde_json::from_value(value)?, input, &mut out)?;
    } else {
        legacy(serde_json::from_value(value)?, input, &mut out)?;
    }
    out.flush()?;
    Ok(())
}
