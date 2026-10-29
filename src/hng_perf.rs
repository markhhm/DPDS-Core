// SPDX-License-Identifier: Apache-2.0
//! Reproducible scheduling-core microbenchmarks (see `examples/hng_bench.rs`).
//!
//! Each sample is the wall time of one batch of `admit + every due dequeue`,
//! divided by the batch size. Arrivals are back-to-back frames at `c_bps`, so
//! in steady state every admitted packet is also dequeued once.
use crate::hng::{HngBuffer, HngConfig, Probability, PushResult};
use crate::hng_trace::{OutageAction, TerminalPolicy, TraceProfile, TraceRow};
use std::{ffi::c_void, hint::black_box, time::Instant};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Mode {
    Fixed,
    Trace,
}

#[derive(Debug, Clone)]
pub struct BenchSpec {
    pub mode: Mode,
    pub packets: u64,
    pub batch: u64,
    /// mbuf bytes; the default 1518 is a 1500-byte IP packet with one VLAN tag.
    pub packet_bytes: u32,
    pub c_bps: u64,
    pub t_ns: u64,
    /// Fixed mode: late amount. Trace mode: the delay span of the synthetic trace.
    pub delta_ns: u64,
    /// Fixed mode only; trace mode requires p = 0.
    pub p: Probability,
    pub trace_row_ns: u64,
    /// Batches that end before this virtual time are not sampled.
    pub warmup_ns: u64,
}

impl Default for BenchSpec {
    fn default() -> Self {
        Self {
            mode: Mode::Fixed,
            packets: 8_000_000,
            batch: 1024,
            packet_bytes: 1518,
            c_bps: 100_000_000_000,
            t_ns: 142_000_000,
            delta_ns: 320_000_000,
            p: Probability {
                // 0.01 * 2^64, the roadmap's lower late fraction.
                threshold: 184_467_440_737_095_516,
                always: false,
            },
            trace_row_ns: 1_000_000,
            warmup_ns: 462_000_000,
        }
    }
}

#[derive(Debug, Default)]
pub struct BenchResult {
    pub samples_ns: Vec<f64>,
    pub accepted: u64,
    pub dequeued: u64,
    pub in_flight: u64,
    pub selected_late: u64,
    pub storage_slots: usize,
}

fn wire_bytes(packet_bytes: u32) -> u64 {
    (u64::from(packet_bytes) + 4).max(64) + 20
}

fn arrival_ns(spec: &BenchSpec, i: u64) -> u64 {
    // Exact back-to-back arrivals at C; truncation never makes two packets overlap Q.
    (u128::from(i) * u128::from(wire_bytes(spec.packet_bytes)) * 8_000_000_000
        / u128::from(spec.c_bps)) as u64
}

fn trace_profile(spec: &BenchSpec, end_ns: u64) -> TraceProfile {
    let rows = (end_ns / spec.trace_row_ns).max(1);
    TraceProfile {
        end_ns,
        // A 16-step sawtooth over [t, t + delta]; falling steps exercise the clamp.
        rows: (0..rows)
            .map(|k| TraceRow {
                at_ns: k * spec.trace_row_ns,
                t_ns: spec.t_ns + spec.delta_ns / 15 * (k % 16),
                loss: Probability::ZERO,
                outage: false,
            })
            .collect(),
        outage_action: OutageAction::Drop,
        terminal: TerminalPolicy::Error,
        loss_seed: 0,
        terminal_horizon_ns: None,
    }
}

pub fn scheduler_bench(spec: &BenchSpec) -> Result<BenchResult, String> {
    if spec.packets == 0 || spec.batch == 0 || spec.c_bps == 0 || spec.trace_row_ns == 0 {
        return Err("packets, batch, c_bps and trace_row_ns must be positive".into());
    }
    let max_delay = spec.t_ns + spec.delta_ns;
    let duration = arrival_ns(spec, spec.packets);
    let pps = spec.c_bps as f64 / (8.0 * wire_bytes(spec.packet_bytes) as f64);
    // The deepest in-flight set is about pps * max delay; reserve a margin.
    let slots = (pps * max_delay as f64 / 1e9 * 1.05) as usize + 2 * spec.batch as usize + 1024;
    let config = HngConfig {
        c_bps: spec.c_bps,
        q_bytes: 1 << 40,
        t_ns: if spec.mode == Mode::Fixed {
            spec.t_ns
        } else {
            0
        },
        delta_ns: if spec.mode == Mode::Fixed {
            spec.delta_ns
        } else {
            0
        },
        p: if spec.mode == Mode::Fixed {
            spec.p
        } else {
            Probability::ZERO
        },
        seed: 1,
        storage_capacity_packets: slots,
        storage_capacity_bytes: slots as u64 * u64::from(spec.packet_bytes),
    };
    let mut b = match spec.mode {
        Mode::Fixed => HngBuffer::new(config)?,
        Mode::Trace => HngBuffer::with_trace(
            config,
            trace_profile(spec, duration + max_delay + 1_000_000),
        )?,
    };
    let mut result = BenchResult {
        storage_slots: slots,
        ..BenchResult::default()
    };
    let mut i = 0;
    while i < spec.packets {
        let end = (i + spec.batch).min(spec.packets);
        let full = end - i == spec.batch;
        let start = Instant::now();
        let mut now = 0;
        while i < end {
            now = arrival_ns(spec, i);
            // Non-null, never dereferenced: the core stores opaque pointers only.
            let ptr = (i + 1) as usize as *mut c_void;
            if b.push(ptr, spec.packet_bytes, now) != PushResult::Accepted {
                return Err(format!("packet {i} was not accepted"));
            }
            while let Some(p) = b.try_pop(now, now) {
                black_box(p);
                result.dequeued += 1;
            }
            i += 1;
        }
        let elapsed = start.elapsed().as_nanos() as f64;
        if full && now >= spec.warmup_ns {
            result.samples_ns.push(elapsed / spec.batch as f64);
        }
    }
    let s = b.stats();
    result.accepted = s.accepted;
    result.in_flight = b.len() as u64;
    result.selected_late = s.selected_late;
    Ok(result)
}

/// ns per operation of the exact u128 service arithmetic used at admission,
/// and of a rounded u64 picosecond baseline (which drifts and is not used).
pub fn service_arithmetic_bench(iterations: u64) -> (f64, f64) {
    let c = black_box(100_000_000_000u64);
    let wire = black_box(1542u64);
    let mut finish = 0u128;
    let mut acc = 0u64;
    let start = Instant::now();
    for i in 0..iterations {
        let arrival = black_box(i * 123);
        let now = u128::from(arrival) * u128::from(c);
        finish = now.max(finish) + u128::from(wire) * 8_000_000_000;
        let service = finish / u128::from(c) + u128::from(!finish.is_multiple_of(u128::from(c)));
        acc ^= service as u64;
    }
    let exact = start.elapsed().as_nanos() as f64 / iterations as f64;
    black_box(acc);
    let ps_per_byte = black_box(8_000_000_000_000 / c);
    let mut finish_ps = 0u64;
    let start = Instant::now();
    for i in 0..iterations {
        let arrival_ps = black_box(i * 123) * 1000;
        finish_ps = arrival_ps.max(finish_ps) + wire * ps_per_byte;
        acc ^= finish_ps.div_ceil(1000);
    }
    let baseline = start.elapsed().as_nanos() as f64 / iterations as f64;
    black_box(acc);
    (exact, baseline)
}

/// Nearest-rank percentile (`p` in 0..=100) of unsorted samples.
pub fn percentile(samples: &[f64], p: f64) -> f64 {
    let mut v = samples.to_vec();
    v.sort_by(f64::total_cmp);
    let rank = ((p / 100.0) * v.len() as f64).ceil() as usize;
    v[rank.clamp(1, v.len()) - 1]
}
