// SPDX-License-Identifier: Apache-2.0
//! Validated VLAN/line configuration and the generated C boundary.
use crate::hng::{HngBuffer, HngConfig, HngPacket, HngStats, PushResult, TraceProfile};
use serde::Deserialize;
use std::{
    ffi::{c_char, CStr},
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
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
    trace: Option<TraceFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TraceFile {
    path: String,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TraceDocument {
    schema: String,
    source_id: String,
    direction: String,
    units: String,
    conversion: String,
    source_sha256: String,
    splice_policy: String,
    trace_epoch_ns: u64,
    time_scale: u64,
    profile: TraceProfile,
}

fn valid_sha(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit())
}

impl TraceFile {
    fn load(&self, base: &Path) -> Result<TraceProfile, String> {
        if !valid_sha(&self.sha256) {
            return Err("trace sha256 must have 64 hex digits".into());
        }
        let bytes = fs::read(base.join(&self.path)).map_err(|e| format!("{}: {e}", self.path))?;
        // Hash the exact bytes parsed below, avoiding a second-read race.
        let mut child = Command::new("sha256sum")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("sha256sum is required at startup: {e}"))?;
        if let Err(e) = child
            .stdin
            .take()
            .ok_or("sha256sum stdin unavailable")?
            .write_all(&bytes)
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e.to_string());
        }
        let output = child.wait_with_output().map_err(|e| e.to_string())?;
        let digest = String::from_utf8_lossy(&output.stdout);
        if !output.status.success()
            || !digest
                .split_whitespace()
                .next()
                .is_some_and(|d| d.eq_ignore_ascii_case(&self.sha256))
        {
            return Err("trace SHA-256 mismatch".into());
        }
        let doc: TraceDocument = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if doc.schema != "hng-trace-v1"
            || doc.units != "ns_owd"
            || doc.trace_epoch_ns != 0
            || doc.time_scale != 1
            || doc.source_id.is_empty()
            || doc.direction.is_empty()
            || doc.conversion.is_empty()
            || !valid_sha(&doc.source_sha256)
            || !matches!(
                doc.splice_policy.as_str(),
                "natural_hold" | "paired_splice" | "unspliced_phases"
            )
        {
            return Err("invalid trace provenance, epoch, scale, units or splice policy".into());
        }
        doc.profile.validate()?;
        Ok(doc.profile)
    }
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

// Owners update their lines per packet; 128-byte alignment keeps them apart.
#[repr(align(128))]
pub struct Line {
    pub info: LineInfo,
    pub(crate) buffer: HngBuffer,
}
pub struct Engine {
    pub lines: Vec<Line>,
}

/// `MemAvailable` from /proc/meminfo, or None where the kernel does not report it.
fn mem_available_bytes() -> Option<u64> {
    let info = fs::read_to_string("/proc/meminfo").ok()?;
    let line = info.lines().find(|l| l.starts_with("MemAvailable:"))?;
    let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    kib.checked_mul(1024)
}

/// Fail cleanly when the core reservation exceeds available memory. Under
/// overcommit the allocation itself succeeds and prefaulting would end in an
/// out-of-memory kill. The check is a snapshot: memory taken by other processes
/// afterwards can still cause that.
pub fn check_reservation(bytes: u64, available: Option<u64>) -> Result<(), String> {
    match available {
        Some(a) if bytes > a => Err(format!(
            "core storage needs {bytes} bytes but only {a} bytes of memory are available"
        )),
        _ => Ok(()),
    }
}

impl Engine {
    pub fn from_json(json: &str) -> Result<Self, String> {
        Self::from_json_at(json, Path::new("."))
    }

    pub fn from_json_at(json: &str, base: &Path) -> Result<Self, String> {
        let config: ConfigFile = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if config.schema != "hng-v1" || config.lines.is_empty() || config.lines.len() > 4 {
            return Err("hng-v1 requires 1..4 VLAN line/direction instances".into());
        }
        // Core storage is prefaulted at construction; refuse up front if it cannot fit.
        let reserve = config.lines.iter().fold(0u64, |sum, c| {
            sum.saturating_add(
                (c.model.storage_capacity_packets as u64)
                    .saturating_mul(crate::hng::slot_metadata_bytes() as u64),
            )
        });
        check_reservation(reserve, mem_available_bytes())?;
        let mut lines: Vec<Line> = Vec::new();
        let mut names = Vec::new();
        for c in config.lines {
            if c.name.is_empty()
                || names.contains(&c.name)
                || c.vlan_id == 0
                || c.vlan_id > 4094
                || lines
                    .iter()
                    .any(|l| l.info.ingress_port == c.ingress_port && l.info.vlan_id == c.vlan_id)
            {
                return Err(
                    "line names and (ingress_port,VLAN) mappings must be unique; VLAN is 1..4094"
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
            let trace = c.trace.as_ref().map(|t| t.load(base)).transpose()?;
            let delay = if let Some(t) = &trace {
                if c.model.t_ns != 0 {
                    return Err("trace model.t_ns must be zero; delay comes from the trace".into());
                }
                if t.terminal == crate::hng::TerminalPolicy::HoldLastSample {
                    let q = u128::from(c.model.q_bytes);
                    let frames = q / u128::from(c.envelope.min_q_bytes);
                    let wire_ns = (q + 20 * frames) * 8_000_000_000u128;
                    let drain = wire_ns.div_ceil(u128::from(c.model.c_bps));
                    let required =
                        u128::from(t.max_delay_ns()) + u128::from(c.model.delta_ns) + drain;
                    let extension = t.terminal_horizon_ns.unwrap() - t.end_ns;
                    if u128::from(extension) < required {
                        return Err(format!(
                            "{}: terminal HOLD extension {extension} ns is below t+delta+Q drain {required} ns for actual line C/Q",
                            c.name
                        ));
                    }
                }
                t.max_delay_ns()
                    .checked_add(if t.outage_action == crate::hng::OutageAction::Hold {
                        t.max_hold_ns()
                    } else {
                        0
                    })
                    .ok_or("trace delay/hold bound overflow")?
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
            let mut buffer = if let Some(trace) = trace {
                HngBuffer::with_trace(c.model, trace)?
            } else {
                HngBuffer::new(c.model)?
            };
            buffer.frame_envelope = (
                u64::from(c.envelope.min_q_bytes),
                u64::from(c.envelope.max_q_bytes),
            );
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
        Engine::from_json_at(&json, Path::new(path).parent().unwrap_or(Path::new(".")))
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
            *out = p;
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
    if (*engine).lines.iter().any(|l| l.buffer.has_backlog()) {
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

/// Core metadata bytes reserved per storage slot (both FIFOs and the ledger).
#[no_mangle]
pub extern "C" fn hng_slot_metadata_bytes() -> usize {
    crate::hng::slot_metadata_bytes()
}

/// Core metadata bytes actually reserved by all lines; 0 for NULL.
/// # Safety
/// Engine is NULL or live; caller serializes with mutation.
#[no_mangle]
pub unsafe extern "C" fn hng_engine_metadata_bytes(engine: *const Engine) -> usize {
    engine.as_ref().map_or(0, |e| {
        e.lines.iter().map(|l| l.buffer.metadata_bytes()).sum()
    })
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

/// Payload custody and future model service events both count as backlog.
/// # Safety
/// Live Engine; caller serializes with all mutation.
#[no_mangle]
pub unsafe extern "C" fn hng_engine_has_backlog(engine: *const Engine) -> bool {
    engine
        .as_ref()
        .is_some_and(|e| e.lines.iter().any(|l| l.buffer.has_backlog()))
}

/// Age service accounting only after all ingress has been processed and sealed.
/// # Safety
/// Live exclusive Engine; caller uses the same monotonic run clock.
#[no_mangle]
pub unsafe extern "C" fn hng_engine_drain_advance(engine: *mut Engine, now_ns: u64) -> bool {
    let Some(e) = engine.as_mut() else {
        return false;
    };
    e.lines.iter_mut().all(|l| l.buffer.drain_advance(now_ns))
}

/// Explicit invalidating abort after all payloads are extracted. Snapshot Q first.
/// # Safety
/// Engine is NULL or exclusively owned; caller records forced abort and outstanding
/// q_bytes per line before destroying the retained service ledger.
#[no_mangle]
pub unsafe extern "C" fn hng_engine_abort_destroy(engine: *mut Engine) -> bool {
    if engine.is_null() {
        return true;
    }
    if (*engine).lines.iter().any(|l| !l.buffer.is_empty()) {
        return false;
    }
    drop(Box::from_raw(engine));
    true
}

// Line handles. A scheduler owner may hold handles for a subset of lines and
// use them while another owner uses handles for other lines of the same
// Engine. Each handle must have one owner at a time, and no `hng_engine_*`
// call may run while any handle is in use: those calls borrow the whole Engine.

/// Handle for one line, or NULL for an invalid index. Take handles before
/// launching owner threads; they stay valid until the Engine is destroyed.
/// # Safety
/// Engine is NULL or live and not in use by any other call.
#[no_mangle]
pub unsafe extern "C" fn hng_engine_line(engine: *mut Engine, line: usize) -> *mut Line {
    match engine.as_mut() {
        Some(e) if line < e.lines.len() => e.lines.as_mut_ptr().add(line),
        _ => std::ptr::null_mut(),
    }
}

/// Same contract as `hng_engine_offer`, for one line handle.
/// # Safety
/// Line is NULL or a live handle owned by the calling thread.
#[no_mangle]
pub unsafe extern "C" fn hng_line_offer(
    line: *mut Line,
    packet: *mut std::ffi::c_void,
    bytes: u32,
    arrival_ns: u64,
    seq: u64,
) -> PushResult {
    line.as_mut().map_or(PushResult::InvalidInput, |l| {
        l.buffer.push_with_seq(packet, bytes, arrival_ns, seq)
    })
}

/// Like `hng_line_offer`. On ACCEPTED, also writes the stored record (release,
/// deadline and sequence numbers never change after admission) to `out`
/// unless it is NULL, so the host can annotate the packet while it is cached.
/// # Safety
/// Line is NULL or a live handle owned by the calling thread; out is NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn hng_line_offer_record(
    line: *mut Line,
    packet: *mut std::ffi::c_void,
    bytes: u32,
    arrival_ns: u64,
    seq: u64,
    out: *mut HngPacket,
) -> PushResult {
    line.as_mut().map_or(PushResult::InvalidInput, |l| {
        l.buffer
            .push_with_record(packet, bytes, arrival_ns, seq, out.as_mut())
    })
}

/// Same contract as `hng_engine_peek`.
/// # Safety
/// Line is NULL or a live handle owned by the calling thread; out is NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn hng_line_peek(
    line: *const Line,
    now_ns: u64,
    watermark_ns: u64,
    out: *mut HngPacket,
) -> bool {
    match (
        line.as_ref()
            .and_then(|l| l.buffer.ready(now_ns, watermark_ns)),
        out.as_mut(),
    ) {
        (Some(p), Some(out)) => {
            *out = p;
            true
        }
        _ => false,
    }
}

/// Same contract as `hng_engine_pop`.
/// # Safety
/// Line is NULL or a live handle owned by the calling thread; out is NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn hng_line_pop(
    line: *mut Line,
    now_ns: u64,
    watermark_ns: u64,
    out: *mut HngPacket,
) -> bool {
    match (
        out.as_mut(),
        line.as_mut()
            .filter(|_| !out.is_null())
            .and_then(|l| l.buffer.try_pop(now_ns, watermark_ns)),
    ) {
        (Some(out), Some(p)) => {
            *out = p;
            true
        }
        _ => false,
    }
}

/// Same contract as `hng_engine_abort_pop`.
/// # Safety
/// Line is NULL or a live handle owned by the calling thread; out is NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn hng_line_abort_pop(line: *mut Line, out: *mut HngPacket) -> bool {
    match (
        out.as_mut(),
        line.as_mut()
            .filter(|_| !out.is_null())
            .and_then(|l| l.buffer.pop()),
    ) {
        (Some(out), Some(p)) => {
            *out = p;
            true
        }
        _ => false,
    }
}

/// # Safety
/// Line is NULL or a live handle owned by the calling thread; out is NULL or writable.
#[no_mangle]
pub unsafe extern "C" fn hng_line_stats(line: *const Line, out: *mut HngStats) -> bool {
    match (line.as_ref(), out.as_mut()) {
        (Some(l), Some(out)) => {
            *out = l.buffer.stats();
            true
        }
        _ => false,
    }
}

/// # Safety
/// Line is NULL or a live handle owned by the calling thread.
#[no_mangle]
pub unsafe extern "C" fn hng_line_min_delay(line: *const Line) -> u64 {
    line.as_ref().map_or(0, |l| l.buffer.min_delay_ns())
}

/// Seal input for this line only; see `hng_engine_end_input`.
/// # Safety
/// Line is NULL or a live handle owned by the calling thread.
#[no_mangle]
pub unsafe extern "C" fn hng_line_end_input(line: *mut Line) {
    if let Some(l) = line.as_mut() {
        l.buffer.end_input();
    }
}

/// See `hng_engine_drain_advance`; false for NULL or unsealed input.
/// # Safety
/// Line is NULL or a live handle owned by the calling thread.
#[no_mangle]
pub unsafe extern "C" fn hng_line_drain_advance(line: *mut Line, now_ns: u64) -> bool {
    line.as_mut()
        .is_some_and(|l| l.buffer.drain_advance(now_ns))
}

/// # Safety
/// Line is NULL or a live handle owned by the calling thread.
#[no_mangle]
pub unsafe extern "C" fn hng_line_has_backlog(line: *const Line) -> bool {
    line.as_ref().is_some_and(|l| l.buffer.has_backlog())
}

/// Pop the smallest `(release_ns, ingress_seq)` ready head among `n` lines that
/// share one egress port, if its release is at most `limit_ns`. Line k uses
/// input watermark `watermarks[k]`. Returns the index k of the popped line, or
/// -1 when no head is ready or the smallest one exceeds the limit.
/// # Safety
/// `lines` and `watermarks` hold `n` entries (or n is 0); every handle is
/// distinct, live and owned by the calling thread; out is writable.
#[no_mangle]
pub unsafe extern "C" fn hng_lines_pop_merged(
    lines: *const *mut Line,
    watermarks: *const u64,
    n: usize,
    now_ns: u64,
    limit_ns: u64,
    out: *mut HngPacket,
) -> isize {
    if n == 0 || lines.is_null() || watermarks.is_null() || out.is_null() {
        return -1;
    }
    let lines = std::slice::from_raw_parts(lines, n);
    let watermarks = std::slice::from_raw_parts(watermarks, n);
    let mut best: Option<(usize, (u64, u64), bool)> = None;
    for (k, &l) in lines.iter().enumerate() {
        let Some(l) = l.as_ref() else { return -1 };
        if let Some(p) = l.buffer.ready(now_ns, watermarks[k]) {
            let key = (p.release_ns, p.ingress_seq);
            if best.is_none_or(|(_, b, _)| key < b) {
                best = Some((k, key, p.late));
            }
        }
    }
    match best {
        Some((k, (release, _), late)) if release <= limit_ns => {
            // ready() just returned this head; release_q cannot change heads.
            match (*lines[k]).buffer.pop_ready(now_ns, late) {
                Some(p) => {
                    *out = p;
                    k as isize
                }
                None => -1,
            }
        }
        _ => -1,
    }
}

/// Offer `n` packets in order, packet i to `lines[line_index[i]]`, as if
/// `hng_line_offer_record` were called for each: `results[i]` gets the outcome
/// and, when ACCEPTED, `records[i]` the admitted record.
/// # Safety
/// The input arrays hold `n` entries, `results` and `records` are writable for
/// `n` entries, every `line_index` is in range of `lines`, and every handle it
/// selects is live and owned by the calling thread.
#[no_mangle]
pub unsafe extern "C" fn hng_lines_offer_burst(
    lines: *const *mut Line,
    line_index: *const u16,
    packets: *const *mut std::ffi::c_void,
    bytes: *const u32,
    arrivals_ns: *const u64,
    seqs: *const u64,
    n: usize,
    results: *mut PushResult,
    records: *mut HngPacket,
) {
    for i in 0..n {
        let line = *lines.add(usize::from(*line_index.add(i)));
        *results.add(i) = line.as_mut().map_or(PushResult::InvalidInput, |l| {
            l.buffer.push_with_record(
                *packets.add(i),
                *bytes.add(i),
                *arrivals_ns.add(i),
                *seqs.add(i),
                records.add(i).as_mut(),
            )
        });
    }
}
