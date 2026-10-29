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

struct Service {
    finish: u128,
    l2_bytes: u64,
}

pub struct HngBuffer {
    config: HngConfig,
    ordinary: VecDeque<HngPacket>,
    late: VecDeque<HngPacket>,
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
        let mut ordinary = VecDeque::new();
        let mut late = VecDeque::new();
        let mut service = VecDeque::new();
        ordinary
            .try_reserve_exact(config.storage_capacity_packets)
            .map_err(|_| "ordinary storage allocation failed")?;
        late.try_reserve_exact(config.storage_capacity_packets)
            .map_err(|_| "late storage allocation failed")?;
        service
            .try_reserve_exact(config.storage_capacity_packets)
            .map_err(|_| "Q metadata allocation failed")?;
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
        let result = self.admit(packet_ptr, packet_bytes, now_ns, seq);
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
        let service_ns = finish / u128::from(self.config.c_bps)
            + u128::from(finish % u128::from(self.config.c_bps) != 0);
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
        self.service.push_back(Service { finish, l2_bytes });
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
        if late {
            self.late.push_back(packet);
            self.stats.selected_late += 1;
        } else {
            self.ordinary.push_back(packet);
        }
        if clamp_extra_ns > 0 {
            self.stats.clamp_packets += 1;
            self.stats.clamp_extra_ns = clamp_total_ns;
            self.stats.clamp_max_ns = self.stats.clamp_max_ns.max(clamp_extra_ns);
        }
        self.stats.accepted += 1;
        self.stats.accepted_bytes += u64::from(packet_bytes);
        self.stats.accepted_l2_bytes += l2_bytes;
        self.stats.accepted_wire_bytes += wire_bytes;
        self.stats.storage_bytes += u64::from(packet_bytes);
        self.stats.storage_packets += 1;
        self.stats.storage_high_water_bytes = self
            .stats
            .storage_high_water_bytes
            .max(self.stats.storage_bytes);
        self.stats.storage_high_water_packets = self
            .stats
            .storage_high_water_packets
            .max(self.stats.storage_packets);
        PushResult::Accepted
    }

    /// Release Q on service completion, independently of propagation storage.
    fn release_q(&mut self, now_ns: u64) {
        self.stats.ledger_watermark_ns = now_ns;
        let now = u128::from(now_ns) * u128::from(self.config.c_bps);
        while self.service.front().is_some_and(|s| s.finish <= now) {
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
    pub fn stats(&self) -> HngStats {
        self.stats
    }

    fn ordinary_first(&self) -> bool {
        match (self.ordinary.front(), self.late.front()) {
            (Some(a), Some(b)) => (a.release_ns, a.ingress_seq) <= (b.release_ns, b.ingress_seq),
            (Some(_), None) => true,
            _ => false,
        }
    }

    pub fn front(&self) -> Option<&HngPacket> {
        if self.ordinary_first() {
            self.ordinary.front()
        } else {
            self.late.front()
        }
    }

    /// W certifies all ingress records with arrival <= W have been processed.
    pub fn ready(&self, now_ns: u64, input_watermark_ns: u64) -> Option<&HngPacket> {
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
        self.ready(now_ns, input_watermark_ns)?;
        self.pop()
    }

    /// Bytes reserved for both packet FIFOs and the service ledger.
    pub fn metadata_bytes(&self) -> usize {
        (self.ordinary.capacity() + self.late.capacity()) * std::mem::size_of::<HngPacket>()
            + self.service.capacity() * std::mem::size_of::<Service>()
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
        let p = if self.ordinary_first() {
            self.ordinary.pop_front()
        } else {
            self.late.pop_front()
        }?;
        self.stats.dequeued += 1;
        self.stats.dequeued_bytes += u64::from(p.packet_bytes);
        self.stats.dequeued_l2_bytes += p.l2_bytes;
        self.stats.dequeued_wire_bytes += p.wire_bytes;
        self.stats.storage_bytes -= u64::from(p.packet_bytes);
        self.stats.storage_packets -= 1;
        Some(p)
    }
}

/// Core metadata bytes reserved per storage slot: two FIFO entries and a ledger entry.
pub fn slot_metadata_bytes() -> usize {
    2 * std::mem::size_of::<HngPacket>() + std::mem::size_of::<Service>()
}

/// SplitMix64 finalizer with fixed wrapping arithmetic, stable across builds.
pub fn selection_word(seed: u64, seq: u64) -> u64 {
    let mut x = seed.wrapping_add(seq.wrapping_add(1).wrapping_mul(0x9e3779b97f4a7c15));
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}
