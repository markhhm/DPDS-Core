//! Software-only production-side replay adapter. Contains no reference formulas.
use dpds_core::hng::{HngBuffer, HngConfig, HngPacket, PushResult, TraceProfile};
use serde::Deserialize;
use serde_json::json;
use std::{
    error::Error,
    ffi::c_void,
    io::{self, BufRead, Write},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    model: HngConfig,
    trace: Option<TraceProfile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Event {
    ingress_seq: u64,
    arrival_ns: u64,
    length: u32,
}

fn output(out: &mut impl Write, packet: HngPacket) -> io::Result<()> {
    writeln!(
        out,
        "{}",
        json!({"kind":"output", "ingress_seq":packet.ingress_seq,
            "selection_seq":packet.selection_seq, "service_ns":packet.service_ns,
            "deadline_ns":packet.deadline_ns, "release_ns":packet.release_ns,
            "late":packet.late, "clamp_extra_ns":packet.clamp_extra_ns,
            "l2_bytes":packet.l2_bytes, "wire_bytes":packet.wire_bytes})
    )
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut input = io::stdin().lock().lines();
    let header: Header = serde_json::from_str(&input.next().ok_or("missing header")??)?;
    let mut buffer = match header.trace {
        Some(trace) => HngBuffer::with_trace(header.model, trace)?,
        None => HngBuffer::new(header.model)?,
    };
    let mut out = io::BufWriter::new(io::stdout().lock());
    for line in input {
        let event: Event = serde_json::from_str(&line?)?;
        let id = usize::try_from(event.ingress_seq.checked_add(1).ok_or("ID overflow")?)?;
        let result = buffer.push_with_seq(
            id as *mut c_void,
            event.length,
            event.arrival_ns,
            event.ingress_seq,
        );
        let outcome = match result {
            PushResult::Accepted => "accepted",
            PushResult::QDrop => "q_drop",
            PushResult::TraceLoss => "trace_loss",
            PushResult::TraceOutage => "trace_outage",
            PushResult::StorageDrop => "storage_drop",
            PushResult::InvalidInput => "invalid_input",
        };
        writeln!(
            out,
            "{}",
            json!({"kind":"admission", "ingress_seq":event.ingress_seq,
                "outcome":outcome,"q_bytes":buffer.stats().q_bytes})
        )?;
        while let Some(packet) = buffer.try_pop(event.arrival_ns, event.arrival_ns) {
            output(&mut out, packet)?;
        }
    }
    buffer.end_input();
    while let Some(packet) = buffer.try_pop(u64::MAX, u64::MAX) {
        output(&mut out, packet)?;
    }
    if buffer.has_backlog() {
        return Err("replay did not drain".into());
    }
    out.flush()?;
    Ok(())
}
