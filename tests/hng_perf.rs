// SPDX-License-Identifier: Apache-2.0
use dpds_core::hng_perf::{percentile, scheduler_bench, service_arithmetic_bench, BenchSpec, Mode};

fn small(mode: Mode) -> BenchSpec {
    BenchSpec {
        mode,
        packets: 20_000,
        batch: 1_000,
        t_ns: 10_000,
        delta_ns: 20_000,
        warmup_ns: 0,
        ..BenchSpec::default()
    }
}

#[test]
fn fixed_bench_admits_every_packet_and_samples_each_batch() {
    let r = scheduler_bench(&small(Mode::Fixed)).unwrap();
    assert_eq!(r.samples_ns.len(), 20);
    assert_eq!(r.accepted, 20_000);
    assert_eq!(r.dequeued + r.in_flight, r.accepted);
    assert!(r.dequeued > 0 && r.selected_late > 0);
}

#[test]
fn trace_bench_admits_every_packet_and_samples_each_batch() {
    let r = scheduler_bench(&small(Mode::Trace)).unwrap();
    assert_eq!(r.samples_ns.len(), 20);
    assert_eq!(r.accepted, 20_000);
    assert_eq!(r.dequeued + r.in_flight, r.accepted);
    assert!(r.dequeued > 0);
}

#[test]
fn warmup_batches_are_not_sampled() {
    let spec = BenchSpec {
        warmup_ns: 1_000_000,
        ..small(Mode::Fixed)
    };
    let r = scheduler_bench(&spec).unwrap();
    // 1 ms of 1542-byte frames at 100 Gbit/s is about 8106 packets: 8 full batches.
    assert!(r.samples_ns.len() < 20 && r.samples_ns.len() >= 10);
}

#[test]
fn arithmetic_bench_reports_both_paths() {
    let (exact, baseline) = service_arithmetic_bench(10_000);
    assert!(exact > 0.0 && baseline > 0.0);
}

#[test]
fn percentile_uses_nearest_rank() {
    let v = [5.0, 1.0, 4.0, 2.0, 3.0];
    assert_eq!(percentile(&v, 50.0), 3.0);
    assert_eq!(percentile(&v, 99.0), 5.0);
    assert_eq!(percentile(&v, 0.0), 1.0);
}
