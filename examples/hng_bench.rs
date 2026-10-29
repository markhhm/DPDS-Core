// SPDX-License-Identifier: Apache-2.0
//! Scheduling-core microbenchmark CLI. Run with
//! `cargo run --release --locked --example hng_bench -- [options]`.
use dpds_core::hng_perf::{percentile, scheduler_bench, service_arithmetic_bench, BenchSpec, Mode};
use std::{env, fs, process::exit};

const USAGE: &str = "usage: hng_bench [--mode fixed|trace|both] [--packets N] [--batch N]
  [--bytes MBUF_BYTES] [--c-bps C] [--t-ns T] [--delta-ns D] [--p DECIMAL]
  [--trace-row-ns R] [--warmup-ns W] [--repeat K]";

fn cpu_line(key: &str) -> String {
    fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with(key))
                .and_then(|l| l.split_once(':'))
                .map(|(_, v)| v.trim().to_owned())
        })
        .unwrap_or_else(|| "unknown".into())
}

fn main() {
    let mut spec = BenchSpec::default();
    let mut modes = vec![Mode::Fixed, Mode::Trace];
    let mut repeat = 3;
    let args: Vec<String> = env::args().skip(1).collect();
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let Some(value) = it.next() else {
            eprintln!("{USAGE}");
            exit(2);
        };
        let number = || {
            value.parse::<u64>().unwrap_or_else(|_| {
                eprintln!("{flag}: expected an unsigned integer\n{USAGE}");
                exit(2)
            })
        };
        match flag.as_str() {
            "--mode" => {
                modes = match value.as_str() {
                    "fixed" => vec![Mode::Fixed],
                    "trace" => vec![Mode::Trace],
                    "both" => vec![Mode::Fixed, Mode::Trace],
                    _ => {
                        eprintln!("{USAGE}");
                        exit(2)
                    }
                }
            }
            "--packets" => spec.packets = number(),
            "--batch" => spec.batch = number(),
            "--bytes" => spec.packet_bytes = number() as u32,
            "--c-bps" => spec.c_bps = number(),
            "--t-ns" => spec.t_ns = number(),
            "--delta-ns" => spec.delta_ns = number(),
            "--p" => {
                spec.p = value.parse().unwrap_or_else(|e| {
                    eprintln!("--p: {e}");
                    exit(2)
                })
            }
            "--trace-row-ns" => spec.trace_row_ns = number(),
            "--warmup-ns" => spec.warmup_ns = number(),
            "--repeat" => repeat = number().max(1),
            _ => {
                eprintln!("{USAGE}");
                exit(2)
            }
        }
    }
    println!(
        "cpu_model=\"{}\" cpu_mhz_proc_cpuinfo={} timer=std::time::Instant(CLOCK_MONOTONIC) clocksource={}",
        cpu_line("model name"),
        cpu_line("cpu MHz"),
        fs::read_to_string("/sys/devices/system/clocksource/clocksource0/current_clocksource")
            .map(|s| s.trim().to_owned())
            .unwrap_or_else(|_| "unknown".into())
    );
    for mode in modes {
        let spec = BenchSpec {
            mode,
            ..spec.clone()
        };
        for run in 0..repeat {
            let r = scheduler_bench(&spec).unwrap_or_else(|e| {
                eprintln!("benchmark failed: {e}");
                exit(1)
            });
            if r.samples_ns.is_empty() {
                eprintln!("no sampled batch: raise --packets or lower --warmup-ns");
                exit(1);
            }
            println!(
                "scheduler mode={mode:?} run={run} packets={} batch={} sampled_batches={} \
                 admit_dequeue_ns_per_packet_p50={:.2} p99={:.2} max={:.2} \
                 accepted={} dequeued={} in_flight_end={} selected_late={} storage_slots={}",
                spec.packets,
                spec.batch,
                r.samples_ns.len(),
                percentile(&r.samples_ns, 50.0),
                percentile(&r.samples_ns, 99.0),
                percentile(&r.samples_ns, 100.0),
                r.accepted,
                r.dequeued,
                r.in_flight,
                r.selected_late,
                r.storage_slots
            );
        }
    }
    for run in 0..repeat {
        let (exact, baseline) = service_arithmetic_bench(50_000_000);
        println!(
            "arithmetic run={run} exact_u128_service_ns_per_op={exact:.2} \
             rounded_u64_ps_baseline_ns_per_op={baseline:.2} difference_ns={:.2}",
            exact - baseline
        );
    }
}
