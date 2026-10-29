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
  -m 512 --file-prefix=hng-sched -- -r 3
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
