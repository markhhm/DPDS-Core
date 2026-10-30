<!-- SPDX-License-Identifier: Apache-2.0 -->

# M4 performance engineering report

Status: software-only results on one development machine, 2026-10-30.

**All numbers in this report are reference values from a shared WSL2 virtual
machine. They are not evidence of 100 Gbit/s performance.** No NIC, hugepage,
PCI device or testbed host was used. Values marked *estimate* are calculations,
not measurements.

## Machine

| Item | Value |
|---|---|
| CPU | AMD Ryzen 7 5800X 8-Core Processor, 16 logical CPUs |
| Frequency source | `/proc/cpuinfo` reports 3800.046 MHz; kernel clocksource `tsc`; DPDK TSC estimate 3.8 GHz |
| Timers | core: `std::time::Instant` (CLOCK_MONOTONIC); host: `rte_rdtsc_precise` |
| Kernel | Linux, WSL2 |
| Memory | DPDK `--no-huge` (4 KiB pages) |
| Load | other workloads ran at the same time (load average about 4); CPU frequency scaling and isolation are not controlled |

## Commits

DPDS-Core `hng/m4-perf`: `bbc05e7` (core microbenchmark), `f32e9b5` (metadata
size API), `ce4b176` (line handles), `0fa6bed` (admitted record), `ee6d418`
(merged pop), `1c73f9f` (direct merged pop), and this report.

DPDS `hng/m4-perf`: `6c18bdc` (net_ring rate), `520b3ca` (pool calculator),
`9f79dc8` (scheduler per ingress port), `cad2194` (scheduler-owner
microbenchmark), `770f23f` (annotate at admission), `f6f7d4d` (merged pop in
the worker), `c9e4012` (core pin).

## 1. Microbenchmarks

### Scheduling core (DPDS-Core)

```sh
cargo run --release --locked --example hng_bench -- --mode both --repeat 3
```

Defaults: C = 100 Gbit/s, 1518-byte mbufs back to back, t = 142 ms,
delta = 320 ms, p = 0.01, batches of 1024 packets, no sample before 462 ms of
virtual time. One sample is the wall time of a batch of admissions plus all due
dequeues, divided by 1024. In steady state each packet is admitted once and
dequeued once, with about 1.18 million packets in flight.

| Mode | admit + dequeue p50 (ns/packet) | p99 |
|---|---:|---:|
| fixed, 3 runs | 28.14 / 28.35 / 31.02 | 73.71 / 104.56 / 127.72 |
| trace, 3 runs | 46.55 / 47.11 / 47.42 | 114.81 / 140.25 / 142.93 |

The trace mode replays a synthetic sawtooth trace (one row per ms between t and
t + delta). It is slower because each admission looks up the trace row for the
release time (binary search) and the monotone-deadline clamp holds packets
longer, so about 3.7 million packets are in flight at the end.

**u128 arithmetic.** The exact rational service time costs 2.94-3.22 ns per
operation, against 1.33-1.39 ns for a rounded u64 picosecond baseline that the
core does not use because it drifts. The difference is 1.61-1.83 ns per packet,
about 6% of the fixed-mode p50.

### Scheduler owner in the host (DPDS)

```sh
cd build && ./hng-sched-bench --lcores=0@6 --no-pci --no-huge --no-telemetry \
  -m 512 --in-memory --file-prefix=hng-sched -- -r 3
```

This times the production `worker_step_at()` (classify, admit, merge, dequeue)
with real mbufs and a fixture clock: 1518-byte frames at 100 Gbit/s per
direction, cable and LEO alternating, t = 1 / 1.5 ms, delta = 2 ms, p = 0.01.
Samples are 64 steps of 16 packets. The run must end with no non-model drop and
all mbufs reclaimed. t is short (not 142 ms) so that the mbuf population fits
in `--no-huge` memory; the core benchmark above covers the deep in-flight case.

### Maximum host packet rate over net_ring (DPDS)

```sh
python3 perf/ring_rate.py build/hng-ring-rate -- -b 1518 -d 3 -w 0.5
python3 perf/ring_rate.py --split build/hng-ring-rate -- -b 1518 -d 3 -w 0.5
```

Production RX, scheduler and TX lcores over two `net_ring` ports, closed-loop
generator on the main lcore, 3 s window after 0.5 s warmup. Final code:

| Layout | mbuf bytes | total pps | per direction |
|---|---:|---:|---:|
| 4 lcores, one scheduler | 1518 | 9.93 M | 4.96 M |
| 4 lcores, one scheduler | 60 | 10.55 M | 5.28 M |
| 5 lcores, scheduler per ingress port | 1518 | 9.52 M | 4.76 M |
| 5 lcores, scheduler per ingress port | 60 | 9.37 M | 4.69 M |

Every run was conserved and drained, with `pending_peak=9`. Before the
scheduler optimizations below, one scheduler reached 7.4-8.3 M total and the
split layout 10.6 M, so the scheduler was the limit then. Now the split does not
raise the rate: the single RX lcore (polling both ports) or the single TX lcore
is the limit on this machine. Neither reaches 8.127 Mpps per direction
(*estimate* for 100G at 1538-byte untagged frames).

## 2. Memory per in-flight packet

From `hng-pool-calc` (real `rte_mempool_calc_obj_size` and core sizes):

| Part | Bytes |
|---|---:|
| mbuf element: `rte_mbuf` 128 + private metadata 56 + data room 2176 (128 headroom + 2048) | 2360 |
| mempool object stride (with object header/trailer, aligned) | 2496 |
| core metadata per storage slot: 2 FIFO entries of `HngPacket` (88 B) + one ledger entry (32 B) | 208 |
| **total per storage slot** | **2704** |

A 1518-byte frame plus 128 bytes of headroom fits in one 2176-byte data room.
The core reserves both FIFOs and the ledger at full slot capacity, so the 208
bytes are reserved per slot, not per packet actually in flight. Mempool objects
never straddle a page: with 2 MiB pages one page holds 840 objects (0.02% waste,
*estimate*); with 4 KiB pages under `--no-huge` one object uses a whole page
(4096 bytes, 64% more than the stride).

## 3. Pool-size calculator

```sh
./build/hng-pool-calc        # defaults are the roadmap parameters
```

Roadmap parameters: 100 Gbit/s, t = 142 ms, delta = 320 ms, Q = 50 ms (625 MB
of L2 bytes), all-MTU envelope (1522-byte L2 frames), burst bound 1024,
256 RX/TX descriptors, 4 lcores, 2 MiB pages, one line per direction:

| Per NUMA node (one direction) | Value |
|---|---:|
| storage slots per line | 4,156,804 |
| pool objects | 4,160,580 |
| mempool (hugepages) | 9.708 GiB |
| core metadata (process heap) | 0.805 GiB |
| **total** | **10.513 GiB** (21.027 GiB for both nodes) |

The required size is the same for p = 0.01 and p = 0.10, because the engine's
startup check assumes that every packet is late (t + delta). The calculator
also prints the mean occupancy with t + p*delta: 1,587,686 packets (3.998 GiB)
at p = 0.01 and 1,821,149 packets (4.586 GiB) at p = 0.10 per node. These are
*estimates*, not sizing bounds, and the engine does not accept them as an
envelope.

Other cases (same parameters):

| Case | Per node | Both nodes |
|---|---:|---:|
| cable and LEO with these parameters (`--lines-per-port 2`) | 21.014 GiB | 42.028 GiB |
| 64-byte minimum frame envelope (`--min-frame 64`) | 198.783 GiB | 397.566 GiB |

The core metadata lives in the Rust heap of the process that loads the
configuration. Its NUMA placement follows that thread, not the pool's node.

**Check against a real allocation.** The meson test `perf-pool-calc` runs:

```sh
./build/hng-pool-calc -l 0 --no-pci --no-huge --no-telemetry -m 512 \
  --vdev=net_null0 --vdev=net_null1 -- --c-bps 1000000000 --t-ms 20 \
  --delta-ms 40 --q-ms 5 --burst 256 --lines-per-port 2 --verify
```

| Item | Calculated | Real | Error |
|---|---:|---:|---:|
| pool objects per port (vs `host_pool_objects`) | 14,836 | 14,836 | exact |
| object stride | 2496 | 2496 | exact |
| mempool bytes per port (DPDK heap growth) | 61,956,672 | 61,958,592 | 0.003% |
| core metadata (process heap growth across load) | 4,600,960 | 4,623,280 | 0.48% |
| **total** | 128,514,304 | 128,540,464 | **0.02%** (gate 5%: PASS) |

The first version ignored the page rule and was 37% low under 4 KiB pages.

## 4. One scheduler per direction

The host has a 5-lcore layout: RX, one scheduler per ingress port, TX. Each
scheduler drives only its lines through core line handles (`hng_engine_line`,
`hng_line_*`), so the two owners never borrow the whole engine at once. Each
has its own RX handoff ring and counters, and both feed one multi-producer TX
ring. Startup rejects the layout unless every egress port is fed by lines of
one ingress port.

**Same semantics.** `tests/split.c` feeds the same input and clock schedule to
one owner of all lines and to two per-port owners. Per-egress output (sequence,
VLAN, length, release and deadline) and line counters are identical: 2938 and
2890 packets per egress, 612 Q drops and 1824 late selections. The PMD
integrity oracle also runs with two owners: `io-ring-split` and
`io-ring-stepped-split` pass all 32 seeds x 153 epochs (10,027,008 input
packets), with interleaved and partially due steps and full rings on both
owners, like the existing `io-ring-stepped`.

**Gate result (p50 <= 60 ns/packet per owner): not met, close.** Final code,
three runs of `-r 3` pinned to CPUs 6, 10 and 14 (9 samples each):

| Owner | p50 ns/packet, sorted | median | p99 range |
|---|---|---:|---:|
| port 0 owner | 61.30 61.60 61.88 63.16 64.42 66.21 66.38 66.59 67.19 | 64.42 | 107-151 |
| port 1 owner | 60.82 61.33 61.65 62.00 63.96 64.06 65.06 65.85 67.01 | 63.96 | 105-163 |
| single owner (all four lines) | 63.04 63.25 64.11 64.29 64.77 65.37 67.29 68.01 69.44 | 64.77 | 112-160 |

The single owner would have needed p50 <= 30 ns; it measures about 65 ns, so
the split is required. The per-packet cost of one owner barely depends on how
many lines it owns; the split helps because each owner sees half the packets.
Against the 123 ns per-packet budget per direction (*estimate*, 8.127 Mpps),
one per-port owner uses about 52%.

What was tried, in order (p50 per owner on this machine):

| Step | p50 |
|---|---:|
| split only, dequeue writes release/deadline/port into the mbuf | 113-205 ns (noisy) |
| write those fields at admission from `hng_line_offer_record`; dequeue touches no mbuf | about 72 ns |
| one frontier snapshot per step, per-port safety limit computed once, a port that cannot emit is not retried in that step | about 63 ns |
| one core call per packet for the merge (`hng_lines_pop_merged`) and no repeated ready check | 61-67 ns |

A rough cycle split of the remaining cost (rdtsc instrumentation, not
committed): classify about 10 ns, admission in the core about 30 ns, merge and
dequeue about 28 ns per packet, with instrumentation overhead included. A hot
cache configuration (t = 10 us) gives about 52 ns, so cache misses on the core
FIFOs and ledger explain part of the gap. Remaining options, not done: a burst
admission call (one FFI call per 16 packets), fewer per-packet counter updates
in the core, a smaller `HngPacket`, and measurement on an isolated, fixed-
frequency core on the target host.

## 5. TX pending bound (gap 6)

`TX_PENDING_PER_LINE = 256` holds packets that `rte_eth_tx_burst` did not
accept. At 8.127 Mpps per direction it covers 31.5 us (*estimate*) of a stalled
or slow port before the run is invalidated. That is shorter than plausible NIC
backpressure events and about as long as the 256-entry TX descriptor ring.

**Recommendation: 1024 per line.** Reasons:

- 1024 covers 126 us at 8.127 Mpps (*estimate*). A stall that exhausts it would
  already add more than the 0.1 ms median timing gate to the packets behind it,
  so a longer buffer only turns an invalid run into late packets that the
  timing gate rejects anyway. 2048 (252 us) would still be inside the 0.5 ms
  p99 gate if a longer margin is wanted.
- Memory: 4 lines x 768 extra mbufs x 2496 bytes is about 7.3 MiB per pool
  (*estimate*), under 0.1% of the 10.5 GiB pool.
- Cost: the pending table removes sent packets with `memmove`, so a full
  1024-entry table copies up to 8 KiB per burst. That only happens while
  backlogged; beyond 1024, replace the table with a ring index.
- Raise the TX descriptor count from 256 together with it, so that the NIC ring
  rather than host memory absorbs short bursts. The right value depends on the
  NIC and is untested.

The software runs here never exceeded `pending_peak=9`; net_ring does not model
NIC backpressure, so this is not evidence for any value. The constant is not
changed in this branch.

## Round 2 (after review)

Sections 1-5 above describe round 1. This section supersedes them where they
differ. Load average on the machine was 3-7 during these runs.

### Commits

DPDS-Core: `c9470fd` (64-byte stored entry, prefaulted prefetching ring,
derived counters, 128-byte `Line` alignment, burst offer). DPDS: `be2855a`
(handles taken before launch), `95f8c32` (departures, counters, pre-M4 golden),
`3c6cfea` (independent RX directions), `e025346` (production fixture,
concurrent owners), `b6f851c` (burst admission, frontier alignment),
`481624e` (calculator), `10a1f6d` (TX pending 1024).

### Fixes from review

- Line handles are taken by `host_load` on the main lcore; owners copy only
  their own. `host_take_line_handles` fails and invalidates after
  `host_mark_launched`, which the split test checks.
- `tests/split.c`: every owner gets one call per tick, then is drained at the
  same time, so both layouts see the same call schedule. Per egress, records
  (sequence, VLAN, length, release, deadline, departure tick) and the worker
  counters `sent`, `emulator_drops`, `tx_ring_drops` match. The single-owner
  output matches a golden digest (`tests/split-golden.csv`) produced by the
  pre-M4 scheduler, DPDS `d75f4fc`, on the same input.
- RX: a full owner ring skips only that port (`ring_full_skips`), so one
  congested direction no longer stalls the other (`rx_split` test).

### Gate: p50 <= 60 ns per owner, owners concurrent on pinned cores

**Still missed for concurrent owners.** Two fixtures:

- small: 100 Gbit/s arrivals, t = 1 / 1.5 ms, delta = 2 ms; about 10,000
  packets in flight per owner;
- production-sized: t = 142 ms, delta = 320 ms, p = 0.01, Q = 50 ms; arrivals
  at 25 Gbit/s per direction so about 294,000 packets in flight per owner
  (a quarter of 100G; 640,000 mbufs, about 2.1 GB resident). Full scale did not
  fit beside other work on this machine.

Quietest run (01:25, CPUs 2/6/10, 4 samples each), the numbers in the commits:

| Fixture | single owner | port 0 owner | port 1 owner |
|---|---:|---:|---:|
| production-sized | 60.1 | 63.4 | 65.2 |
| small | 56.9 | 63.6 | 60.0 |

Re-run after the restart, load average 6-7, CPU sets 2/6/10, 4/8/12, 2/10/14,
6 samples each (p50 median, range; p99 median):

| Fixture | single owner | port 0 owner | port 1 owner |
|---|---|---|---|
| production-sized | 70.5 (59.5-85.7); 210.7 | 64.9 (61.8-91.0); 114.5 | 81.8 (63.8-214.7); 200.8 |
| small | 58.4 (55.4-73.2); 108.4 | 65.9 (54.2-79.5); 108.5 | 67.1 (57.2-169.4); 155.8 |

Rerun:

```sh
cd build
python3 ../perf/sched_bench.py ./hng-sched-bench --cpus 2,6,10 -- -r 3
python3 ../perf/sched_bench.py ./hng-sched-bench --cpus 2,6,10 --memory 1800 -- \
  -c sched_bench_prod.json -g 493440 -n 3000000 -w 470000000 -m 640000 -r 3
```

Each attempt on the production-sized fixture, medians of 4 runs with
concurrent owners (noise is several ns):

| Attempt | single | per owner | Kept |
|---|---:|---:|---|
| starting point | 109 | 109-113 | |
| frontier: one cache line per port | 106 | 107-108 | yes |
| prefetch input mbufs | 109 | 106-108 | no |
| 64-byte stored FIFO entry (was 88) | 98 | 99-101 | yes |
| touch reserved storage at startup (page faults had been in the timed path) | 70 | 76-78 | yes |
| burst admission, one core call per burst | 70 | 75 | yes |
| 128-byte `Line` alignment | 69 | 74 | yes |
| own ring, prefetch 8 slots ahead at both ends | 57 | 65-72 | yes |
| 128-byte alignment of bench per-owner state | 59 | 62-63 | yes (bench) |
| derive dequeued, storage_packets and wire counters in `stats()` | 60 | 63-65 | yes |

What limits it: a cycle split (rdtsc, not committed) puts classify at about
5 ns, core admission at about 30 ns and merge plus dequeue at about 30 ns per
packet on the production-sized fixture. Two owners at once are 3-8 ns slower
each than one owner alone, which points at the shared 32 MiB L3 and memory
(each owner's working set is about 19 MB of FIFO entries plus cold mbufs).
4 KiB pages (TLB misses) and the shared, loaded WSL2 host add noise of the
same size as the remaining gap. Not tried: transparent huge pages as the
default (optional `HNG_BENCH_THP=1` and `GLIBC_TUNABLES=glibc.malloc.hugetlb=1`
hints exist; one run gave mixed results), a larger prefetch distance, and an
isolated fixed-frequency core.

### Core and host rate after round 2 (loaded machine)

Core microbenchmark, fixed p50 32.37 / 33.54 ns, trace 49.37 / 49.46 ns;
u128 arithmetic 3.40 ns against 1.51-1.61 ns (difference 1.79-1.88 ns). The
core benchmark now includes the startup prefault outside the timed region.
net_ring rate, 1518 bytes: 9.30 M pps total with one scheduler, 9.16 M with
one per ingress port; both conserved, `pending_peak` 8-9.

### Memory and calculator

Core metadata per slot is now 160 bytes (2 x 64-byte entries + 32-byte
ledger entry); with the 2496-byte mbuf stride that is 2656 bytes per slot.

`hng-pool-calc` now defaults to 5 lcores, models trace lines
(`--trace-max-delay-ms`, `--trace-max-hold-ms`: largest delay plus longest
outage hold), and marks the 2 MiB page result *unverified*. `--verify` builds
the pool with the production `host_create_pool` and compares the calculated
object count with the mempool's own size and populated count.

| Case (per NUMA node; both nodes) | Objects | GiB |
|---|---:|---:|
| roadmap: 100G, t=142, delta=320 ms, Q=50 ms, p=0.01 or 0.10 | 4,164,452 | 10.335; 20.670 |
| cable and LEO with those parameters | 8,321,256 | 20.652; 41.304 |
| trace, illustrative max delay 60 ms + hold 30 ms | 1,148,887 | 2.859; 5.717 |

Mean occupancy (*estimate*, not a bound): 3.927 GiB at p = 0.01 and 4.505 GiB
at p = 0.10 per node. Verify (4 KiB pages): objects 14,868 calculated and in
the mempool on both ports; mempool bytes error 0.000; core metadata error
1.0%; total error 0.03% (gate 5%: PASS).

### TX pending

Applied: `TX_PENDING_PER_LINE = 1024` and 1024 TX descriptors per port
(about 126 us at 8.127 Mpps, *estimate*). `tx_pending` checks that a blocked
port holds exactly 1024 packets per line before invalidating, and that ports
request 1024 descriptors. The larger reserves need `-m 256` under `--no-huge`.
Not tested on a NIC.

### Skipped

Item 8 (one RX and TX lcore per direction, JSON fixture SPDX notes) was not
started. Trace-mode `--verify` is not supported (fixed mode only).

## Round 3 (second review)

Commits: DPDS-Core `76c9f74` (64-byte aligned FIFO entry), `fbed6b9`
(available-memory check). DPDS `8db7cc0` (golden digest tracked), `28cc751`
(pin), `ce55f10` (TX pending head index), `fec6516` (NIC loss invalidates),
`5661858` (static asserts), `b112ae3` (pin).

- `tests/split-golden.csv` was ignored by `*.csv` and never committed, so a
  clean clone failed `dpds:split`. It is now tracked. A fresh `git clone
  --recursive` of the pushed `hng/m4-perf` branch into a scratch directory
  built and passed all 24 meson tests, including `split`.
- `Stored` is now `#[repr(align(64))]` (size and alignment 64, unit test), so
  no FIFO entry straddles two cache lines. Measured at load average 7.6-8.0,
  production-sized fixture, concurrent owners, CPU sets 2/6/10, 4/8/12,
  2/10/14: p50 median 71.1 / 74.9 ns aligned against 83.9 / 90.5 ns without,
  then 75.8 / 84.5 ns aligned again. The effect is inside this noise.
- The TX pending table keeps a head index in a 2x buffer and compacts only
  when an append reaches the end, so removal is O(1) and appends amortized
  O(1). `tx_pending` drains a full 1024-entry table through terminal drops
  (217,512 cycles, against 298,072 with the old per-packet `memmove`) and
  checks FIFO order across 8192 packets with refused, partial and full sends.
- Nonzero `imissed` or `rx_nombuf` now invalidates the run, checked before the
  status line. net_ring does not report these counters, so the test uses
  synthetic counters. `rx_ring_full_skips` counts skipped polls, not packets;
  the frames stay in the NIC queue and show up as `imissed` only if it
  overflows.
- The static asserts after `rx_stats`, `worker_stats` and `tx_stats` now check
  their own 64-byte alignment and whole-line size.
- Prefault: load now compares the total core reservation with `MemAvailable`
  and returns a configuration error if it does not fit. This is a snapshot;
  memory taken by other processes afterwards can still cause an
  out-of-memory kill under overcommit. The prefault stays serial on the main
  lcore before launch (about 0.6 GiB per line at the production envelope).

**Gate: still open.** Rerun of the production-sized fixture after these
changes, pinned as above, load average 12.0 at start and end: p50 median
single owner 85.0 ns (61.2-90.8), port 0 owner 94.2 ns (82.3-98.6), port 1
owner 96.2 ns (69.0-103.2). At this load the run cannot resolve a 60 ns
gate; the quietest measurement remains 63.4 / 65.2 ns per concurrent owner
(round 2). A clean measurement needs an idle machine or the target host.

One local `check.sh` run on DPDS `b112ae3` exited 1 without a captured cause;
the pre-push hook on the same commit and an immediate rerun both passed.

## WSL2-only and untested

- Every timing number is from WSL2 on a shared desktop CPU with other load,
  4 KiB pages and no frequency control. Run-to-run spread is several ns.
- Not tested: hugepages, NICs, NUMA (one node here), cross-NUMA egress, 100G,
  the 5-lcore layout under NIC traffic, the two scheduler owners on separate
  cores at full rate (the owner benchmark runs them one after another on one
  core; `ring_rate --split` runs them concurrently but is limited by RX/TX),
  and a single RX lcore serving two 100G ports.
- The pool calculator was checked only with 4 KiB pages; the 2 MiB result
  uses the same page rule, unverified.
- The `null-layouts` test failed once during this work without output (likely
  the 0.25 s startup delay before SIGINT under load) and passed on five reruns.
  It predates this branch.

## Round 4: per-packet cost of one scheduler owner (2026-10-30)

Goal: p50 <= 60 ns per packet for each owner. **Result: not reached, and no
change in this round moved the p50 by more than the noise.** WSL2 on a shared
desktop CPU (AMD Ryzen 7 5800X, 16 logical CPUs, 4 KiB pages, `--no-huge`),
owners pinned to CPUs 2/6/10, other workloads running throughout. No number
here is evidence for 100 Gbit/s.

### Baseline (sorted p50 per run, 9 runs per cell, `hng-sched-bench`)

| Fixture | Owner | p50 ns/packet, sorted | median | p99 median (range) |
|---|---|---|---:|---:|
| small | single (all lines) | 52.75 55.64 55.68 55.87 55.97 57.65 58.96 59.14 68.49 | 55.97 | 96 (85-116) |
| small | port 0 | 55.78 56.00 57.61 58.82 59.36 59.49 59.52 60.40 70.00 | 59.36 | 103 (81-129) |
| small | port 1 | 58.15 58.78 60.27 61.41 61.56 61.69 65.09 66.47 71.68 | 61.56 | 107 (94-126) |
| production-sized | single | 55.93 57.81 57.87 58.00 58.99 60.02 60.38 60.50 61.84 | 58.99 | 102 (91-120) |
| production-sized | port 0 | 60.49 60.79 60.89 61.50 61.65 61.69 66.28 71.89 74.49 | 61.65 | 108 (99-134) |
| production-sized | port 1 | 61.42 61.66 62.11 62.73 62.90 64.23 70.25 71.16 73.07 | 62.90 | 115 (102-123) |

One-minute load average 16.5 at the start and 12.4 at the end. A second run of
the same binary at load 6.2 gave production-sized port 0 / port 1 medians of
62.11 / 62.63 ns (small: 56.20 / 57.29). The production-sized owners therefore
sit at 61-63 ns, about 2-3 ns over the gate.

### Where the time goes

Temporary rdtsc timers around the phases of `worker_step_at()` (not
committed; the timers add overhead, and the means include the warm-up fill).
Production-sized fixture, port 0 owner, three runs, ns per packet:

| Phase | Range over 3 runs |
|---|---:|
| classify | 7.6-14.7 |
| core admission (`hng_lines_offer_burst`) | 36.4-73.8 |
| annotate the mbuf | 2.9-6.8 |
| frontier snapshot | 0.8-2.1 |
| merge and dequeue (`hng_lines_pop_merged`, per packet) | 27.3-48.5 |

(The third run was at higher load; all phases grew together.) Admission and
merge/dequeue are about 85% of the cost. There is no `perf` on this machine.

### Method for the changes below

The earlier method (separate runs of 9, then compare medians) cannot resolve
2 ns when the load average is 17-60. Each change was built into its own
binary and library. One round runs both binaries back to back, in alternating
order, on the same CPUs, for both fixtures; the figure is the median over
rounds of (new - old) p50, with the range. Load average was 16-43 during the
divmod comparison and 28-60 during the prefetch comparisons, so absolute p50s
in those runs were about 100 ns and are not comparable with the baseline
above. Only the differences are used.

### Changes tried

| Change | Rounds | Result (new - old p50, median; range over rounds) | Kept |
|---|---:|---|---|
| Burst pop: one `hng_lines_pop_merged_burst` call per step instead of one call per packet (Core + `worker_step_at`) | one pair of 9-run sets (before the A/B method) | production-sized port 0 62.11 -> 62.23 ns, port 1 62.63 -> 62.44; no difference | no; Core test and equivalence held, then reverted |
| Admission: one 128-bit division, remainder from the quotient (`0c0e0d0`) | 15 | small port 0 +0.60 (-8.25..+4.29), port 1 +0.46 (-7.14..+6.75); production-sized port 0 -1.51 (-18.19..+6.07), port 1 -3.25 (-15.16..+5.51); new faster in 12/15 and 14/15 production-sized rounds | yes: exact (new test pins the ceiling for rates that do not divide), saves a division, but the gain is inside the noise |
| Prefetch input mbuf header, metadata and data line for the whole burst before classify (DPDS) | 15 and 25 | first 15: small -2.70 / -1.42, production-sized +1.90 / +8.36; then 25 rounds: small +1.30 / +0.78, production-sized -1.54 / +0.16; signs flip between sets | no: no effect distinguishable from zero |

The divmod change keeps `service_ns` identical: the new test
`service_time_is_the_exact_ceiling_for_rates_that_do_not_divide` compares it to
`ceil(finish / c_bps)` for three rates, 6000 packets, with a numerator above
64 bits; it passed before and after the change.

The earlier prefetch of input mbufs (round 2) also showed nothing. Only the
core-side prefetch of the FIFO slots helped.

### Remaining options (not done)

- Core admission is the largest phase. Each accepted packet updates about ten
  statistics counters, two high-water marks (one computes the sum of both
  FIFO lengths) and a 32-byte ledger entry. Counters that can be derived in
  `stats()` (as `dequeued` and `accepted_wire_bytes` already are) could be
  removed from the packet path; this changes no output but was not measured.
- A smaller or split ledger entry: `finish` is a `u128`; a `u64` base plus
  offset would halve it.
- Merge and dequeue run `ready()` and then `pop_ready()`, each computing
  `ordinary_first()` and expanding the stored entry. One pass that returns the
  popped entry directly would remove that duplicate work. The merged pop is
  one core call per packet; a burst call did not help.
- Transparent huge pages for the FIFOs, and measurement on an idle or isolated
  fixed-frequency core: the remaining gap (about 2-3 ns) is smaller than the
  run-to-run spread on this machine, so the gate can only be judged there.

## Round 5: counter, ledger and merged-dequeue experiments (2026-10-30)

Starting revisions: DPDS `6b9880d`, DPDS-Core `2f10bdf`. Same shared WSL2
machine, 4 KiB pages, no frequency control or isolation; other builds could
run concurrently. These are software reference measurements, not evidence
for 100 Gbit/s. The baseline full checks passed in both repositories (all
30 host tests ran; the core Python suite retained its one expected skip).

Each variant snapshots the unchanged host binary and its own core library.
One round runs old and new back to back in alternating order, for both
fixtures and owners on CPUs 2/6/10, with at least 15 rounds per comparison.
Differences below are paired p50 **new minus old**, in ns/packet. The load
range is the one-minute load sampled after every benchmark invocation.

### 1. Derive redundant statistics off the packet path

Tried deriving accepted/dequeued L2 bytes from payload bytes, packet counts
and separately accumulated padding for frames shorter than 60 mbuf bytes.
Admission watermark and input-complete status also came from existing state.
The C statistics layout and all values stayed unchanged. A characterization
test pinned padding, partial dequeues, Q/storage/invalid rejections, high-water
marks, sealing and ledger drain before the implementation changed.

15 paired rounds; load **3.31-5.14**:

| Fixture | Owner | Old p50 median | New p50 median | Paired difference median (range) | New faster |
|---|---|---:|---:|---:|---:|
| small | port 0 | 55.94 | 55.15 | -0.79 (-7.00..+12.09) | 11/15 |
| small | port 1 | 57.14 | 56.46 | -0.51 (-6.50..+26.59) | 8/15 |
| production-sized | port 0 | 61.05 | 60.16 | -0.60 (-4.13..+18.62) | 10/15 |
| production-sized | port 1 | 61.68 | 61.31 | -0.55 (-1.95..+18.95) | 10/15 |

**Not kept:** the sub-nanosecond differences are inside the observed noise.
The candidate passed the full Rust test suite and all 30 host tests,
including the split golden and integrity oracles, using its own library.
The implementation and characterization test are saved as a local patch.

### 2. Store integer completion time in the service ledger

The main service clock remains exact `u128` arithmetic. A ledger entry now
stores the already computed `u64 service_ns = ceil(finish / C)`, instead of
the rational `u128 finish`. For any integer clock tick,
`ceil(finish / C) <= now` iff `finish <= now * C`, so Q releases at exactly
the same tick without a base/offset representation. This also removes the
128-bit multiplication in `release_q()`. Ledger entries shrink from 32 to
16 bytes; the metadata query and pool calculator use 144 instead of 160
bytes per slot. This is an allocation-size change, not a packet-timing change.

Before the change, the new characterization test passed against the old
implementation. It checks 72 combinations of four rates (including a
non-dividing rate and `u64::MAX`), three clocks (including near `u64::MAX`),
sealed/open input, and accepted/trace-loss/trace-outage packets. It compares
Q occupancy just before and at each integer completion with a rational
oracle. The metadata test failed at 160 vs 144 bytes before the change and
passed afterwards.

As a noise control, identical copies of the baseline ran 15 A/A rounds at
load 4.09-5.77. Production-sized paired medians were +0.54 / +0.45 ns
(ranges -5.43..+6.66 / -7.06..+12.63); small medians were -1.12 / -0.45 ns
(-4.31..+4.70 / -2.72..+2.32). A single sub-nanosecond result is insufficient.

Ledger A/B ran twice, 15 rounds each. The first set, load 2.89-9.21, gave
production-sized paired medians -2.24 / -2.34 ns, faster in 12/15 and 10/15.
The second, load 4.09-6.09, gave -2.82 / -2.67 ns, faster in 12/15 for both
owners. Combined **30 rounds**, retaining every outlier, load **2.89-9.21**:

| Fixture | Owner | Old p50 median | New p50 median | Paired difference median (range) | New faster |
|---|---|---:|---:|---:|---:|
| small | port 0 | 55.45 | 53.18 | -2.00 (-30.88..+38.61) | 21/30 |
| small | port 1 | 56.43 | 54.81 | -1.63 (-30.51..+34.75) | 18/30 |
| production-sized | port 0 | 61.68 | 58.89 | -2.50 (-40.07..+13.11) | 24/30 |
| production-sized | port 1 | 62.52 | 59.86 | -2.49 (-39.93..+22.73) | 22/30 |

**Kept:** both production-sized owners improve in both 15-round sets, by
more than the A/A median offsets. The full Rust suite and all 30 host tests
passed against the candidate library, including golden/oracle and pool
calculator checks. The production-sized variant's p50 medians are below
60 ns, but its per-run p50 ranges are 56.47-94.52 / 57.68-95.20 ns on this
shared machine. This does not establish a stable 60 ns bound or 100G operation.
