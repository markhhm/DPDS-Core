// SPDX-License-Identifier: Apache-2.0
//! Fixed-run HNG scheduler. One instance owns one direction's shared C/Q.
use serde::{Deserialize, Deserializer};
use std::{collections::VecDeque, ffi::c_void, str::FromStr};

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Probability {
    pub threshold: u64,
    pub always: bool,
}

impl Probability {
    pub const ZERO: Self = Self {
        threshold: 0,
        always: false,
    };
    pub const ONE: Self = Self {
        threshold: 0,
        always: true,
    };
    pub fn sample(self, seed: u64, seq: u64) -> bool {
        self.always || selection_word(seed, seq) < self.threshold
    }
}

impl FromStr for Probability {
    type Err = &'static str;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (whole, fraction) = s.split_once('.').unwrap_or((s, ""));
        if !matches!(whole, "0" | "1")
            || fraction.len() > 18
            || !fraction.bytes().all(|b| b.is_ascii_digit())
            || s.ends_with('.')
        {
            return Err("p must be a decimal string in [0,1], with at most 18 fractional digits");
        }
        let numerator = if fraction.is_empty() {
            0
        } else {
            fraction
                .parse::<u128>()
                .map_err(|_| "invalid probability")?
        };
        if whole == "1" {
            return if numerator == 0 {
                Ok(Self::ONE)
            } else {
                Err("p exceeds 1")
            };
        }
        Ok(Self {
            threshold: ((numerator << 64) / 10u128.pow(fraction.len() as u32)) as u64,
            always: false,
        })
    }
}

impl<'de> Deserialize<'de> for Probability {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HngConfig {
    pub c_bps: u64,
    /// Q charges L2 bytes including FCS and the packet currently in service.
    pub q_bytes: u64,
    pub t_ns: u64,
    pub delta_ns: u64,
    pub p: Probability,
    pub seed: u64,
    /// Storage reserves space at admission, including packets still in Q.
    pub storage_capacity_packets: usize,
    /// Stored mbuf bytes, excluding FCS and physical-layer overhead.
    pub storage_capacity_bytes: u64,
}

impl HngConfig {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.c_bps == 0 {
            return Err("c_bps must be positive");
        }
        if self.storage_capacity_packets == 0 || self.storage_capacity_bytes == 0 {
            return Err("storage capacities must be positive");
        }
        if self.t_ns.checked_add(self.delta_ns).is_none() {
            return Err("t_ns + delta_ns overflows");
        }
        Ok(())
    }
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PushResult {
    Accepted,
    QDrop,
    StorageDrop,
    InvalidInput,
    TraceLoss,
    TraceOutage,
}

pub use crate::hng_trace::{OutageAction, TerminalPolicy, TraceProfile, TraceRow};
use crate::hng_trace::{TracePolicy, TxPolicy};

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct HngPacket {
    pub packet_ptr: *mut c_void,
    pub ingress_seq: u64,
    pub selection_seq: u64,
    pub service_ns: u64,
    pub deadline_ns: u64,
    pub release_ns: u64,
    pub clamp_extra_ns: u64,
    pub packet_bytes: u32,
    pub l2_bytes: u64,
    pub wire_bytes: u64,
    pub late: bool,
}

#[repr(C)]
#[derive(Debug, Default, Copy, Clone)]
pub struct HngStats {
    pub accepted: u64,
    pub accepted_bytes: u64,
    pub accepted_l2_bytes: u64,
    pub accepted_wire_bytes: u64,
    pub dequeued: u64,
    pub dequeued_bytes: u64,
    pub dequeued_l2_bytes: u64,
    pub dequeued_wire_bytes: u64,
    pub q_drops: u64,
    pub q_drop_bytes: u64,
    pub q_drop_payload_bytes: u64,
    pub storage_drops: u64,
    pub storage_drop_bytes: u64,
    pub invalid_drops: u64,
    pub invalid_drop_bytes: u64,
    pub selected_late: u64,
    pub q_bytes: u64,
    pub q_high_water_bytes: u64,
    pub storage_bytes: u64,
    pub storage_high_water_bytes: u64,
    pub storage_packets: u64,
    pub storage_high_water_packets: u64,
    pub admission_watermark_ns: u64,
    pub ledger_watermark_ns: u64,
    pub input_complete: bool,
    pub trace_loss_drops: u64,
    pub trace_loss_bytes: u64,
    pub trace_outage_drops: u64,
    pub trace_outage_bytes: u64,
    pub clamp_packets: u64,
    pub clamp_extra_ns: u64,
    pub clamp_max_ns: u64,
}

/// FIFO entry: one 64-byte cache line. L2 and wire bytes follow from packet_bytes.
#[derive(Copy, Clone)]
#[repr(align(64))]
struct Stored {
    packet_ptr: *mut c_void,
    ingress_seq: u64,
    selection_seq: u64,
    service_ns: u64,
    deadline_ns: u64,
    release_ns: u64,
    clamp_extra_ns: u64,
    packet_bytes: u32,
    late: bool,
}

impl Stored {
    fn new(p: &HngPacket) -> Self {
        Self {
            packet_ptr: p.packet_ptr,
            ingress_seq: p.ingress_seq,
            selection_seq: p.selection_seq,
            service_ns: p.service_ns,
            deadline_ns: p.deadline_ns,
            release_ns: p.release_ns,
            clamp_extra_ns: p.clamp_extra_ns,
            packet_bytes: p.packet_bytes,
            late: p.late,
        }
    }
    fn expand(&self) -> HngPacket {
        // Same derivation as admission: mbuf bytes + FCS, padded to 64; + 20 on the wire.
        let l2_bytes = (u64::from(self.packet_bytes) + 4).max(64);
        HngPacket {
            packet_ptr: self.packet_ptr,
            ingress_seq: self.ingress_seq,
            selection_seq: self.selection_seq,
            service_ns: self.service_ns,
            deadline_ns: self.deadline_ns,
            release_ns: self.release_ns,
            clamp_extra_ns: self.clamp_extra_ns,
            packet_bytes: self.packet_bytes,
            l2_bytes,
            wire_bytes: l2_bytes + 20,
            late: self.late,
        }
    }
}

/// Fixed-capacity FIFO ring, filled once at construction so every page is
/// touched. Pushes and pops prefetch the slot they will reach a few packets
/// later: with a ring larger than the in-flight set both ends are cold.
struct Ring<T: Copy> {
    buf: Box<[T]>,
    head: usize,
    len: usize,
}

const PREFETCH_AHEAD: usize = 8;

impl<T: Copy> Ring<T> {
    fn new(capacity: usize, fill: T) -> Result<Self, ()> {
        let mut v = Vec::new();
        v.try_reserve_exact(capacity).map_err(|_| ())?;
        v.resize(capacity, fill);
        Ok(Self {
            buf: v.into_boxed_slice(),
            head: 0,
            len: 0,
        })
    }
    fn slot(&self, offset: usize) -> usize {
        let i = self.head + offset;
        if i >= self.buf.len() {
            i - self.buf.len()
        } else {
            i
        }
    }
    fn prefetch(&self, offset: usize) {
        #[cfg(target_arch = "x86_64")]
        if offset < self.buf.len() {
            // SAFETY: SSE is baseline on x86_64; prefetch never faults.
            unsafe {
                use std::arch::x86_64::{_mm_prefetch, _MM_HINT_T0};
                _mm_prefetch::<_MM_HINT_T0>((&self.buf[self.slot(offset)] as *const T).cast());
            }
        }
        #[cfg(not(target_arch = "x86_64"))]
        let _ = offset;
    }
    fn len(&self) -> usize {
        self.len
    }
    fn capacity(&self) -> usize {
        self.buf.len()
    }
    fn front(&self) -> Option<&T> {
        (self.len > 0).then(|| &self.buf[self.head])
    }
    /// False when full; admission checks storage capacity first, so callers never see it.
    fn push_back(&mut self, x: T) -> bool {
        if self.len == self.buf.len() {
            return false;
        }
        let i = self.slot(self.len);
        self.buf[i] = x;
        self.len += 1;
        self.prefetch(self.len + PREFETCH_AHEAD);
        true
    }
    fn pop_front(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        let x = self.buf[self.head];
        self.head = self.slot(1);
        self.len -= 1;
        self.prefetch(PREFETCH_AHEAD);
        Some(x)
    }
}

struct Service {
    service_ns: u64,
    l2_bytes: u64,
}

pub struct HngBuffer {
    config: HngConfig,
    ordinary: Ring<Stored>,
    late: Ring<Stored>,
    service: VecDeque<Service>,
    // Rational service time: ns * C. Avoid rounding every serialization up.
    finish: u128,
    last_arrival_ns: u64,
    next_seq: u64,
    next_selection_seq: u64,
    input_complete: bool,
    pub(crate) frame_envelope: (u64, u64),
    stats: HngStats,
    trace: Option<TracePolicy>,
    trace_cursor: usize,
    trace_deadline_ns: u64,
    minimum_delay_ns: u64,
}

impl HngBuffer {
    pub fn new(config: HngConfig) -> Result<Self, &'static str> {
        config.validate()?;
        let mut service = VecDeque::new();
        service
            .try_reserve_exact(config.storage_capacity_packets)
            .map_err(|_| "Q metadata allocation failed")?;
        // Touch every reserved page now, so first use in the packet path takes no page fault.
        let empty = Stored::new(&HngPacket {
            packet_ptr: std::ptr::null_mut(),
            ingress_seq: 0,
            selection_seq: 0,
            service_ns: 0,
            deadline_ns: 0,
            release_ns: 0,
            clamp_extra_ns: 0,
            packet_bytes: 0,
            l2_bytes: 0,
            wire_bytes: 0,
            late: false,
        });
        let ordinary = Ring::new(config.storage_capacity_packets, empty)
            .map_err(|_| "ordinary storage allocation failed")?;
        let late = Ring::new(config.storage_capacity_packets, empty)
            .map_err(|_| "late storage allocation failed")?;
        service.resize_with(service.capacity(), || Service {
            service_ns: 0,
            l2_bytes: 0,
        });
        service.clear();
        Ok(Self {
            config,
            ordinary,
            late,
            service,
            finish: 0,
            last_arrival_ns: 0,
            next_seq: 0,
            next_selection_seq: 0,
            input_complete: false,
            frame_envelope: (64, u64::MAX),
            stats: HngStats::default(),
            trace: None,
            trace_cursor: 0,
            trace_deadline_ns: 0,
            minimum_delay_ns: config.t_ns,
        })
    }

    pub fn with_trace(config: HngConfig, trace: TraceProfile) -> Result<Self, &'static str> {
        trace.validate()?;
        if config.p != Probability::ZERO || config.delta_ns != 0 {
            return Err("trace per-packet late amounts are reserved, not implemented");
        }
        let mut buffer = Self::new(config)?;
        buffer.minimum_delay_ns = trace.rows.iter().map(|r| r.t_ns).min().unwrap_or(0);
        buffer.trace = Some(TracePolicy::new(trace)?);
        Ok(buffer)
    }

    pub fn push(&mut self, packet_ptr: *mut c_void, packet_bytes: u32, now_ns: u64) -> PushResult {
        self.push_with_seq(packet_ptr, packet_bytes, now_ns, self.next_seq)
    }

    pub fn push_with_seq(
        &mut self,
        packet_ptr: *mut c_void,
        packet_bytes: u32,
        now_ns: u64,
        seq: u64,
    ) -> PushResult {
        self.push_with_record(packet_ptr, packet_bytes, now_ns, seq, None)
    }

    /// Like `push_with_seq`; an accepted packet's record is also copied to `record`.
    pub fn push_with_record(
        &mut self,
        packet_ptr: *mut c_void,
        packet_bytes: u32,
        now_ns: u64,
        seq: u64,
        record: Option<&mut HngPacket>,
    ) -> PushResult {
        let result = self.admit(packet_ptr, packet_bytes, now_ns, seq, record);
        match result {
            PushResult::Accepted => {}
            PushResult::QDrop => {
                self.stats.q_drops += 1;
                self.stats.q_drop_bytes += (u64::from(packet_bytes) + 4).max(64);
                self.stats.q_drop_payload_bytes += u64::from(packet_bytes);
            }
            PushResult::StorageDrop => {
                self.stats.storage_drops += 1;
                self.stats.storage_drop_bytes += packet_bytes as u64;
            }
            PushResult::InvalidInput => {
                self.stats.invalid_drops += 1;
                self.stats.invalid_drop_bytes += packet_bytes as u64;
            }
            PushResult::TraceLoss => {
                self.stats.trace_loss_drops += 1;
                self.stats.trace_loss_bytes += u64::from(packet_bytes);
            }
            PushResult::TraceOutage => {
                self.stats.trace_outage_drops += 1;
                self.stats.trace_outage_bytes += u64::from(packet_bytes);
            }
        }
        result
    }

    fn admit(
        &mut self,
        packet_ptr: *mut c_void,
        packet_bytes: u32,
        now_ns: u64,
        seq: u64,
        record: Option<&mut HngPacket>,
    ) -> PushResult {
        if self.input_complete
            || packet_ptr.is_null()
            || packet_bytes == 0
            || now_ns < self.last_arrival_ns
            || seq < self.next_seq
            || seq == u64::MAX
        {
            return PushResult::InvalidInput;
        }
        // mbuf length excludes FCS; Ethernet pads to 60 bytes before FCS.
        let l2_bytes = (u64::from(packet_bytes) + 4).max(64);
        if l2_bytes < self.frame_envelope.0 || l2_bytes > self.frame_envelope.1 {
            return PushResult::InvalidInput;
        }
        self.release_q(now_ns);
        self.last_arrival_ns = now_ns;
        self.stats.admission_watermark_ns = now_ns;
        self.next_seq = seq + 1;
        let selection_seq = self.next_selection_seq;
        self.next_selection_seq += 1;
        if l2_bytes > self.config.q_bytes || self.stats.q_bytes > self.config.q_bytes - l2_bytes {
            return PushResult::QDrop;
        }
        let wire_bytes = l2_bytes + 20;
        let now = u128::from(now_ns) * u128::from(self.config.c_bps);
        let Some(finish) = now
            .max(self.finish)
            .checked_add(u128::from(wire_bytes) * 8_000_000_000)
        else {
            return PushResult::InvalidInput;
        };
        // One 128-bit division: the remainder follows from the quotient.
        let c = u128::from(self.config.c_bps);
        let quotient = finish / c;
        let service_ns = quotient + u128::from(finish != quotient * c);
        let Ok(service_ns) = u64::try_from(service_ns) else {
            return PushResult::InvalidInput;
        };
        let late = self.config.p.sample(self.config.seed, selection_seq);
        let Some(mut deadline_ns) = service_ns
            .checked_add(if self.trace.is_some() {
                0
            } else {
                self.config.t_ns
            })
            .and_then(|t| t.checked_add(if late { self.config.delta_ns } else { 0 }))
        else {
            return PushResult::InvalidInput;
        };

        let mut trace_drop = None;
        let mut trace_cursor = self.trace_cursor;
        let mut release_ns = deadline_ns;
        let mut clamp_extra_ns = 0;
        if let Some(trace) = &self.trace {
            if !trace.covers(service_ns) {
                return PushResult::InvalidInput;
            }
            while trace_cursor + 1 < trace.profile.rows.len()
                && trace.profile.rows[trace_cursor + 1].at_ns <= service_ns
            {
                trace_cursor += 1;
            }
            let row = trace.profile.rows[trace_cursor];
            let Some(raw_deadline_ns) = service_ns.checked_add(row.t_ns) else {
                return PushResult::InvalidInput;
            };
            deadline_ns = raw_deadline_ns.max(self.trace_deadline_ns);
            clamp_extra_ns = deadline_ns - raw_deadline_ns;
            if row.loss.sample(trace.profile.loss_seed, selection_seq) {
                trace_drop = Some(PushResult::TraceLoss);
            } else {
                match trace.release(deadline_ns) {
                    Ok(Some(release)) => release_ns = release,
                    Ok(None) => trace_drop = Some(PushResult::TraceOutage),
                    Err(_) => return PushResult::InvalidInput,
                }
            }
        }
        if self.service.len() >= self.config.storage_capacity_packets
            || (trace_drop.is_none()
                && (self.len() >= self.config.storage_capacity_packets
                    || u64::from(packet_bytes) > self.config.storage_capacity_bytes
                    || self.stats.storage_bytes
                        > self.config.storage_capacity_bytes - u64::from(packet_bytes)))
        {
            return PushResult::StorageDrop;
        }

        let Some(clamp_total_ns) = self.stats.clamp_extra_ns.checked_add(clamp_extra_ns) else {
            return PushResult::InvalidInput;
        };
        self.finish = finish;
        self.trace_cursor = trace_cursor;
        self.service.push_back(Service {
            service_ns,
            l2_bytes,
        });
        self.stats.q_bytes += l2_bytes;
        self.stats.q_high_water_bytes = self.stats.q_high_water_bytes.max(self.stats.q_bytes);
        if let Some(result) = trace_drop {
            return result;
        }
        if self.trace.is_some() {
            self.trace_deadline_ns = deadline_ns;
        }
        let packet = HngPacket {
            packet_ptr,
            ingress_seq: seq,
            selection_seq,
            service_ns,
            deadline_ns,
            release_ns,
            clamp_extra_ns,
            packet_bytes,
            l2_bytes,
            wire_bytes,
            late,
        };
        if let Some(r) = record {
            *r = packet;
        }
        // Cannot be full: each ring holds storage_capacity_packets, checked above.
        let pushed = if late {
            self.late.push_back(Stored::new(&packet))
        } else {
            self.ordinary.push_back(Stored::new(&packet))
        };
        debug_assert!(pushed);
        if late {
            self.stats.selected_late += 1;
        }
        if clamp_extra_ns > 0 {
            self.stats.clamp_packets += 1;
            self.stats.clamp_extra_ns = clamp_total_ns;
            self.stats.clamp_max_ns = self.stats.clamp_max_ns.max(clamp_extra_ns);
        }
        self.stats.accepted += 1;
        self.stats.accepted_bytes += u64::from(packet_bytes);
        self.stats.accepted_l2_bytes += l2_bytes;
        self.stats.storage_bytes += u64::from(packet_bytes);
        self.stats.storage_high_water_bytes = self
            .stats
            .storage_high_water_bytes
            .max(self.stats.storage_bytes);
        self.stats.storage_high_water_packets =
            self.stats.storage_high_water_packets.max(self.len() as u64);
        PushResult::Accepted
    }

    /// Release Q on service completion, independently of propagation storage.
    fn release_q(&mut self, now_ns: u64) {
        self.stats.ledger_watermark_ns = now_ns;
        // Integer clocks: ceil(finish / C) <= now iff finish <= now * C.
        while self.service.front().is_some_and(|s| s.service_ns <= now_ns) {
            self.stats.q_bytes -= self.service.pop_front().unwrap().l2_bytes;
        }
    }

    pub fn len(&self) -> usize {
        self.ordinary.len() + self.late.len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn has_backlog(&self) -> bool {
        !self.is_empty() || !self.service.is_empty()
    }
    /// Counters that follow from others are derived here, off the packet path:
    /// every stored packet was accepted, and wire bytes are L2 bytes + 20.
    pub fn stats(&self) -> HngStats {
        let mut s = self.stats;
        s.storage_packets = self.len() as u64;
        s.dequeued = s.accepted - s.storage_packets;
        s.dequeued_bytes = s.accepted_bytes - s.storage_bytes;
        s.accepted_wire_bytes = s.accepted_l2_bytes + 20 * s.accepted;
        s.dequeued_wire_bytes = s.dequeued_l2_bytes + 20 * s.dequeued;
        s
    }

    fn ordinary_first(&self) -> bool {
        match (self.ordinary.front(), self.late.front()) {
            (Some(a), Some(b)) => (a.release_ns, a.ingress_seq) <= (b.release_ns, b.ingress_seq),
            (Some(_), None) => true,
            _ => false,
        }
    }

    pub fn front(&self) -> Option<HngPacket> {
        if self.ordinary_first() {
            self.ordinary.front()
        } else {
            self.late.front()
        }
        .map(Stored::expand)
    }

    /// W certifies all ingress records with arrival <= W have been processed.
    pub fn ready(&self, now_ns: u64, input_watermark_ns: u64) -> Option<HngPacket> {
        let p = self.front()?;
        if p.release_ns > now_ns
            || (!self.input_complete
                && p.release_ns > input_watermark_ns.saturating_add(self.minimum_delay_ns))
        {
            None
        } else {
            Some(p)
        }
    }

    pub fn try_pop(&mut self, now_ns: u64, input_watermark_ns: u64) -> Option<HngPacket> {
        if self.input_complete {
            self.release_q(now_ns);
        }
        let p = self.ready(now_ns, input_watermark_ns)?;
        self.pop_fifo(p.late)
    }

    /// Bytes reserved for both packet FIFOs and the service ledger.
    pub fn metadata_bytes(&self) -> usize {
        (self.ordinary.capacity() + self.late.capacity()) * std::mem::size_of::<Stored>()
            + self.service.capacity() * std::mem::size_of::<Service>()
    }
    /// `try_pop` after `ready()` returned a head in the selected late/ordinary FIFO.
    pub(crate) fn pop_ready(&mut self, now_ns: u64, late: bool) -> Option<HngPacket> {
        if self.input_complete {
            self.release_q(now_ns);
        }
        self.pop_fifo(late)
    }
    pub fn min_delay_ns(&self) -> u64 {
        self.minimum_delay_ns
    }
    pub fn end_input(&mut self) {
        self.input_complete = true;
        self.stats.input_complete = true;
    }
    pub fn drain_advance(&mut self, now_ns: u64) -> bool {
        if !self.input_complete {
            return false;
        }
        self.release_q(now_ns);
        true
    }
    pub fn tx_policy(&self) -> TxPolicy {
        TxPolicy {
            trace: self.trace.clone(),
        }
    }

    /// Pop transfers only the stored pointer; the host owns and frees payloads.
    /// The host checks the deadline before normal dequeue; cleanup may drain early.
    pub fn pop(&mut self) -> Option<HngPacket> {
        self.pop_fifo(!self.ordinary_first())
    }

    fn pop_fifo(&mut self, late: bool) -> Option<HngPacket> {
        let p = if late {
            self.late.pop_front()
        } else {
            self.ordinary.pop_front()
        }?
        .expand();
        self.stats.dequeued_l2_bytes += p.l2_bytes;
        self.stats.storage_bytes -= u64::from(p.packet_bytes);
        Some(p)
    }
}

/// Core metadata bytes reserved per storage slot: two FIFO entries and a ledger entry.
pub fn slot_metadata_bytes() -> usize {
    2 * std::mem::size_of::<Stored>() + std::mem::size_of::<Service>()
}

/// SplitMix64 finalizer with fixed wrapping arithmetic, stable across builds.
pub fn selection_word(seed: u64, seq: u64) -> u64 {
    let mut x = seed.wrapping_add(seq.wrapping_add(1).wrapping_mul(0x9e3779b97f4a7c15));
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}

#[cfg(test)]
mod layout_tests {
    /// Each FIFO entry occupies exactly one cache line, never two.
    #[test]
    fn stored_entry_is_one_aligned_cache_line() {
        assert_eq!(std::mem::size_of::<super::Stored>(), 64);
        assert_eq!(std::mem::align_of::<super::Stored>(), 64);
    }
}
