//! Validated VLAN/line configuration and the generated C boundary.
use crate::hng::{HngBuffer, HngConfig, HngPacket, HngStats, PushResult, TraceProfile};
use serde::Deserialize;
use std::{
    ffi::{c_char, CStr},
    fs,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigFile {
    schema: String,
    lines: Vec<LineConfig>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LineConfig {
    name: String,
    ingress_port: u16,
    egress_port: u16,
    vlan_id: u16,
    model: HngConfig,
    envelope: Envelope,
    trace: Option<TraceProfile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    min_q_bytes: u32,
    max_q_bytes: u32,
    burst_bound_packets: u64,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct LineInfo {
    pub ingress_port: u16,
    pub egress_port: u16,
    pub vlan_id: u16,
    pub min_q_bytes: u32,
    pub max_q_bytes: u32,
    pub storage_capacity_packets: usize,
    pub storage_capacity_bytes: u64,
    pub c_bps: u64,
    pub q_bytes: u64,
    pub seed: u64,
    pub threshold: u64,
    pub always_late: bool,
}

pub struct Line {
    pub info: LineInfo,
    pub(crate) buffer: HngBuffer,
}
pub struct Engine {
    pub lines: Vec<Line>,
}

impl Engine {
    pub fn from_json(json: &str) -> Result<Self, String> {
        let config: ConfigFile = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if config.schema != "hng-v1" || config.lines.is_empty() || config.lines.len() > 4 {
            return Err("hng-v1 requires 1..4 VLAN line/direction instances".into());
        }
        let mut lines: Vec<Line> = Vec::new();
        let mut names = Vec::new();
        for c in config.lines {
            if c.name.is_empty()
                || names.contains(&c.name)
                || c.vlan_id > 4094
                || lines
                    .iter()
                    .any(|l| l.info.ingress_port == c.ingress_port && l.info.vlan_id == c.vlan_id)
            {
                return Err(
                    "line names and (ingress_port,VLAN) mappings must be unique; VLAN is 0..4094"
                        .into(),
                );
            }
            c.model.validate()?;
            if c.envelope.min_q_bytes < 64
                || c.envelope.max_q_bytes > 1522
                || c.envelope.max_q_bytes < c.envelope.min_q_bytes
                || c.envelope.burst_bound_packets == 0
            {
                return Err(
                    "require a positive burst bound and 64 <= min_q_bytes <= max_q_bytes <= 1522"
                        .into(),
                );
            }
            let delay = if let Some(t) = &c.trace {
                t.validate()?;
                t.max_delay_ns()
            } else {
                c.model
                    .t_ns
                    .checked_add(c.model.delta_ns)
                    .ok_or("delay overflow")?
            };
            let n = u128::from(c.model.c_bps) * u128::from(delay);
            let d = 8_000_000_000u128 * (u128::from(c.envelope.min_q_bytes) + 20);
            let need = u128::from(c.model.q_bytes / u64::from(c.envelope.min_q_bytes))
                + u128::from(c.envelope.burst_bound_packets)
                + n / d
                + u128::from(n % d != 0);
            if need > c.model.storage_capacity_packets as u128
                || need * u128::from(c.envelope.max_q_bytes - 4)
                    > u128::from(c.model.storage_capacity_bytes)
            {
                return Err(format!(
                    "{}: storage is below the declared envelope (at least {need} packet slots)",
                    c.name
                ));
            }
            let info = LineInfo {
                ingress_port: c.ingress_port,
                egress_port: c.egress_port,
                vlan_id: c.vlan_id,
                min_q_bytes: c.envelope.min_q_bytes,
                max_q_bytes: c.envelope.max_q_bytes,
                storage_capacity_packets: c.model.storage_capacity_packets,
                storage_capacity_bytes: c.model.storage_capacity_bytes,
                c_bps: c.model.c_bps,
                q_bytes: c.model.q_bytes,
                seed: c.model.seed,
                threshold: c.model.p.threshold,
                always_late: c.model.p.always,
            };
            let buffer = if let Some(trace) = c.trace {
                HngBuffer::with_trace(c.model, trace)?
            } else {
                HngBuffer::new(c.model)?
            };
            names.push(c.name);
            lines.push(Line { info, buffer });
        }
        Ok(Self { lines })
    }
}

/// Load and validate before RX starts. Errors are printed and return NULL.
/// # Safety
/// `path` is a valid NUL-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn hng_engine_load(path: *const c_char) -> *mut Engine {
    let result = (|| {
        if path.is_null() {
            return Err("null config path".to_owned());
        }
        let path = CStr::from_ptr(path).to_str().map_err(|e| e.to_string())?;
        let json = fs::read_to_string(path).map_err(|e| e.to_string())?;
        Engine::from_json(&json)
    })();
    match result {
        Ok(e) => Box::into_raw(Box::new(e)),
        Err(e) => {
            eprintln!("HNG configuration error: {e}");
            std::ptr::null_mut()
        }
    }
}

/// # Safety
/// All engine pointers must be NULL or live handles from hng_engine_load.
#[no_mangle]
pub unsafe extern "C" fn hng_engine_line_count(engine: *const Engine) -> usize {
    engine.as_ref().map_or(0, |e| e.lines.len())
}

/// # Safety
/// Engine is live; out is NULL or points to writable LineInfo.
#[no_mangle]
pub unsafe extern "C" fn hng_engine_line_info(
    engine: *const Engine,
    line: usize,
    out: *mut LineInfo,
) -> bool {
    match (
        engine.as_ref().and_then(|e| e.lines.get(line)),
        out.as_mut(),
    ) {
        (Some(l), Some(out)) => {
            *out = l.info;
            true
        }
        _ => false,
    }
}

/// Rejected pointers remain with the host; ACCEPTED pointers return via pop.
/// # Safety
/// Engine is live and exclusively owned for this call; packet stays live until popped.
#[no_mangle]
pub unsafe extern "C" fn hng_engine_offer(
    engine: *mut Engine,
    line: usize,
    packet: *mut std::ffi::c_void,
    bytes: u32,
    arrival_ns: u64,
    seq: u64,
) -> PushResult {
    engine
        .as_mut()
        .and_then(|e| e.lines.get_mut(line))
        .map_or(PushResult::InvalidInput, |l| {
            l.buffer.push_with_seq(packet, bytes, arrival_ns, seq)
        })
}

/// Peek an eligible head without transferring custody, for shared-port merge.
/// # Safety
/// Engine is live; out is NULL or writable; W certifies all arrivals <= W are processed.
#[no_mangle]
pub unsafe extern "C" fn hng_engine_peek(
    engine: *const Engine,
    line: usize,
    now_ns: u64,
    watermark_ns: u64,
    out: *mut HngPacket,
) -> bool {
    let Some(buffer) = engine
        .as_ref()
        .and_then(|e| e.lines.get(line))
        .map(|l| &l.buffer)
    else {
        return false;
    };
    match (buffer.ready(now_ns, watermark_ns), out.as_mut()) {
        (Some(p), Some(out)) => {
            *out = *p;
            true
        }
        _ => false,
    }
}

/// # Safety
/// Same contract as peek; exclusive access. Successful pop returns pointer custody.
#[no_mangle]
pub unsafe extern "C" fn hng_engine_pop(
    engine: *mut Engine,
    line: usize,
    now_ns: u64,
    watermark_ns: u64,
    out: *mut HngPacket,
) -> bool {
    if out.is_null() {
        return false;
    }
    match engine
        .as_mut()
        .and_then(|e| e.lines.get_mut(line))
        .and_then(|l| l.buffer.try_pop(now_ns, watermark_ns))
    {
        Some(p) => {
            *out = p;
            true
        }
        None => false,
    }
}

/// Abort-only extraction; caller records a shutdown discard and frees once.
/// # Safety
/// Engine is live/exclusive; out is NULL or writable. Never use this for normal TX.
#[no_mangle]
pub unsafe extern "C" fn hng_engine_abort_pop(
    engine: *mut Engine,
    line: usize,
    out: *mut HngPacket,
) -> bool {
    if out.is_null() {
        return false;
    }
    match engine
        .as_mut()
        .and_then(|e| e.lines.get_mut(line))
        .and_then(|l| l.buffer.pop())
    {
        Some(p) => {
            *out = p;
            true
        }
        None => false,
    }
}

/// # Safety
/// Engine is live; out is NULL or writable; caller serializes with all mutation.
#[no_mangle]
pub unsafe extern "C" fn hng_engine_stats(
    engine: *const Engine,
    line: usize,
    out: *mut HngStats,
) -> bool {
    match (
        engine.as_ref().and_then(|e| e.lines.get(line)),
        out.as_mut(),
    ) {
        (Some(l), Some(out)) => {
            *out = l.buffer.stats();
            true
        }
        _ => false,
    }
}

/// # Safety
/// Engine is NULL or a live handle. Nonempty handles are retained and return false.
#[no_mangle]
pub unsafe extern "C" fn hng_engine_destroy(engine: *mut Engine) -> bool {
    if engine.is_null() {
        return true;
    }
    if (*engine).lines.iter().any(|l| !l.buffer.is_empty()) {
        return false;
    }
    drop(Box::from_raw(engine));
    true
}

/// # Safety
/// Engine is live and not being mutated; call before launching pipeline threads.
#[no_mangle]
pub unsafe extern "C" fn hng_tx_policy_new(
    engine: *const Engine,
    line: usize,
) -> *mut crate::hng_trace::TxPolicy {
    engine
        .as_ref()
        .and_then(|e| e.lines.get(line))
        .map_or(std::ptr::null_mut(), |l| {
            Box::into_raw(Box::new(l.buffer.tx_policy()))
        })
}

/// Immutable policy handle can be read by the TX owner concurrently with Engine.
/// # Safety
/// Policy is a live handle, independently owned until TX exits.
#[no_mangle]
pub unsafe extern "C" fn hng_tx_policy_gate(
    policy: *const crate::hng_trace::TxPolicy,
    now_ns: u64,
) -> u32 {
    policy.as_ref().map_or(3, |p| p.gate(now_ns))
}

/// # Safety
/// Policy is NULL or a handle from hng_tx_policy_new, and is never used afterwards.
#[no_mangle]
pub unsafe extern "C" fn hng_tx_policy_destroy(policy: *mut crate::hng_trace::TxPolicy) {
    if !policy.is_null() {
        drop(Box::from_raw(policy));
    }
}

/// # Safety
/// Live Engine; caller serializes with mutation.
#[no_mangle]
pub unsafe extern "C" fn hng_engine_min_delay(engine: *const Engine, line: usize) -> u64 {
    engine
        .as_ref()
        .and_then(|e| e.lines.get(line))
        .map_or(0, |l| l.buffer.min_delay_ns())
}

/// # Safety
/// Live Engine; caller serializes with mutation.
#[no_mangle]
pub unsafe extern "C" fn hng_engine_len(engine: *const Engine, line: usize) -> usize {
    engine
        .as_ref()
        .and_then(|e| e.lines.get(line))
        .map_or(0, |l| l.buffer.len())
}

/// All RX/handoffs have been processed; future offers become invalid.
/// # Safety
/// Engine is live/exclusive; caller certifies there are no unprocessed arrivals.
#[no_mangle]
pub unsafe extern "C" fn hng_engine_end_input(engine: *mut Engine) {
    if let Some(e) = engine.as_mut() {
        for l in &mut e.lines {
            l.buffer.end_input();
        }
    }
}
