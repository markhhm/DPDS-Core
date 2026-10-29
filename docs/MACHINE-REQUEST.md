<!-- SPDX-License-Identifier: Apache-2.0 -->

# P1 accuracy and throughput acceptance on the existing testbed

Date: 2026-10-29. Testbed edition; replaces the earlier new-hardware request.
Status: requirements and measurement plan only; no hardware accessed or configured.

## 1. Decision and boundary

**Decision (project owner, 2026-10-29):** no new or borrowed hardware. P1 uses only the equipment that already exists in the shared P4 testbed. The earlier three-machine/two-machine proposal is kept only as [Appendix A](#appendix-a-future-hardware-not-available).

Functional and correctness work (M1–M4) stays on the development workstation with DPDK virtual devices (section 13). The testbed is used only for delay-accuracy and throughput acceptance. Its engine contract and G2 thresholds come from [HNG-DESIGN.md](HNG-DESIGN.md) and [HNG-OBSERVER.md](HNG-OBSERVER.md).

Evidence labels used throughout:

- **Decision:** a project-owner decision of 2026-10-29, or roadmap revision `4b07aea`, M1–M7 and OS1–OS6.
- **Testbed record:** the testbed owner's private inventory, copied here only at role level (no names, addresses or accounts).
- **Code:** behavior read at a pinned source revision, not a performance measurement.
- **Specification:** linked primary vendor or DPDK documentation.
- **Estimate:** calculation or engineering recommendation that has not been measured.
- ***unverified***: a capability that no opened document confirms; section 9 lists the check.
- **TBD / open question:** information required before qualification (section 8).

## 2. Testbed equipment (role level)

| Role | What the testbed record shows | Relevance to P1 |
|---|---|---|
| Host A (Proxmox hypervisor) | Two single-port ConnectX-4 100GbE adapters (CX455A family; one `MCX455A-ECAT`-class, one OEM part), each in its own PCIe Gen3 x16 slot, one on each NUMA node; 64 logical CPUs; also 10G ports. Adapter A1 is cabled 100G directly to Tofino switch 1. Adapter A0 is cabled 100G to the aggregation switch; that link is administratively down on both sides. The host also runs other teams' VMs | DUT candidate: most cores and the only host with two 100G adapters |
| Host B (Proxmox hypervisor) | One ConnectX-4 100GbE adapter, cabled directly to Tofino switch 2; 32 logical CPUs | Not on Tofino switch 1; reachable from it only through the aggregation switch |
| Host C (Proxmox hypervisor) | One ConnectX-4 `MCX455A-ECAT` 100GbE adapter (firmware 12.28.2302), cabled directly to Tofino switch 1; 6 physical cores. After each VM start its PCIe link trained at 2.5 GT/s (RDMA about 27.5 Gbit/s) until a root-port retrain restored 8 GT/s (about 98 Gbit/s) | Traffic generator / sink candidate on the same switch as the DUT |
| Tofino switch 1 and 2 | Wedge100BF-32X (Tofino 1), 100G ports, RS FEC on the host links; each has one 100G uplink to the aggregation switch. No direct link between the two Tofino switches | In-path switch; Tofino switch 1 can provide one hardware clock for both DUT boundaries |
| Aggregation switch | 100G Broadcom-based SONiC switch joining the two Tofino uplinks, the host A0 link and 10G breakouts | Only path for a second DUT port (variant B) |

**Testbed record:** a 2026-10-28 RDMA write test between host C and host A1 through Tofino switch 1 reached 97.95 Gbit/s. This shows that the link and PCIe path can carry about 100G; it is not packet-rate or emulator evidence. All hosts use NTP within the lab; no PTP grandmaster or GPS reference appears in the testbed records.

## 3. Placement: emulator, generator and sink (a)

**Variant A (primary, no cabling or link change).** DUT = host A, using only adapter A1 as a one-port loop. Traffic generator and sink = host C. Tofino switch 1 forwards by VLAN.

```text
host C (TG + sink)                Tofino switch 1                 host A (DUT)
  CX-4 port  ── 100G ──  port P_TG          port P_DUT  ── 100G ──  CX-4 A1
                           TG -> P_DUT       stamp "before" on egress to DUT
                           P_DUT -> TG       stamp "after" on ingress from DUT
                                         DUT: RX A1 -> engine (cable/LEO by VLAN) -> TX A1
```

- The DUT receives on A1, schedules by VLAN line, and transmits back on A1 with the registered egress VLAN. Each egress has one ingress source (the same port), so the HNG-OBSERVER v1 direction rule holds; register it as a physical one-port loop topology.
- PCIe: Gen3 x16 gives `8 GT/s × 16 × 128/130 ÷ 8 = 15.754 GB/s` per direction before overhead ([PCIe encoding][pcie]), about 126 Gbit/s. That covers 100G RX writes plus 100G TX reads on one adapter (*estimate*), but not two 100G directions.
- Capacity: one 100G wire direction into the DUT and one out. The modeled "forward" and "reverse" lines can both be exercised, but they share that 100G.
- Host B is not used at 100G. It can serve as a low-rate control or spare host.

**Variant B (needs the testbed owner to enable the down 100G link).** DUT uses both adapters: in on A1 (from Tofino switch 1), out on A0 (to the aggregation switch, then the Tofino switch 1 uplink, then host C). This exercises a real two-port, cross-NUMA DUT path, still one 100G direction at a time, because host C has one 100G port and the Tofino switch 1 uplink carries one 100G stream. Both observation points remain on Tofino switch 1 (egress to A1, ingress from the uplink), so they share one clock, but the aggregation switch transit sits inside the measured OWD and must be bounded (section 5). Use variant B only after variant A passes.

## 4. Timestamps and clocks (b)

| Source | Capability (evidence) | Use |
|---|---|---|
| Tofino switch 1 data-plane timestamps | A P4 program can read the ingress MAC timestamp and an egress-pipeline timestamp on one switch clock. Exact timestamp points, resolution and whether the loaded SDE/NOS exposes them: ***unverified*** | **Primary OWD source.** `before` = egress timestamp toward P_DUT, `after` = ingress MAC timestamp from P_DUT. The P4 program overwrites a reserved, fixed-length field of the test packet (length unchanged), so the sink receives both values with every packet |
| ConnectX-4 RX hardware timestamps (host C, host A) | DPDK 26.03 mlx5 lists ConnectX-4, firmware ≥12.21.1000, as the minimum for RX timestamp offload ([mlx5 guide][mlx5]); the testbed card reports firmware 12.28.2302. Default clock is a free-running counter; whether the real-time clock mode (`REAL_TIME_CLOCK_ENABLE`) exists on CX-4: ***unverified*** | Secondary: RX/RX pairs on host C's single adapter clock when Tofino switch 1 mirrors before/after copies to it (10G stage only, section 5). Also sink-side diagnostics |
| ConnectX-4 TX timestamps | None at wire level. DPDK TX send scheduling needs ConnectX-6 Dx or later ([mlx5 guide][mlx5]); NVIDIA documents TX-port timestamps for ConnectX-6 Dx and later ([timestamps][timestamps]). CX-4 completion timestamps are not wire times | Not usable for OWD. The TG's send time is never one end of a gate measurement |
| DUT software time (TSC) | RX observation and TX-submit bracket in the engine clock E (code) | Diagnostic residence only; never a substitute for an observed egress |

**Clock synchronization.** The primary method needs no synchronization between hosts: both ends of every OWD come from one Tofino counter. It still needs:

1. **Rate calibration of the Tofino clock.** A rate error ε turns into an OWD error of `OWD × ε`. At the 462 ms (142 + 320 ms) example, ±100 ppm (*estimate*, a typical oscillator tolerance class, not a measured value) gives 46.2 µs, above the 10 µs budget; 20 ppm gives 9.24 µs. Calibrate against an NTP-disciplined host clock over a ≥1 h baseline: with ≤1 ms error at each end (*estimate*), `2 ms / 3600 s = 0.56 ppm`, so 0.26 µs at 462 ms and 0.28 µs at 512 ms. Also confirm that no software steps or slews the Tofino counter during a run.
2. **A coarse mapping from the Tofino clock O to the engine clock E**, registered with its bound, to assign samples to fill/steady/transition/drain windows and for diagnostics. NTP-level (ms) accuracy is enough for this if packets within the mapping bound of a window edge are reported as boundary samples, not silently moved.
3. For the secondary mirror method, both copies use host C's one adapter clock, so no host-to-host synchronization is needed either. Host-to-host PTP over the Tofino switches is not documented in the testbed and is not required; any OWD that subtracts two hosts' clocks is not accepted.

## 5. Verifying the accuracy targets without a passive tap (c)

Targets (unchanged, [HNG-DESIGN.md](HNG-DESIGN.md) section 9 and [HNG-OBSERVER.md](HNG-OBSERVER.md) section 7): per direction, line and required cell, `median(|OWD_i − expected_i|) ≤ 0.1 ms` and `p99(|OWD_i − expected_i|) ≤ 0.5 ms`; combined observation uncertainty `u ≤ 10 µs`; a gate passes only if its interval upper bound plus u is below the threshold.

Method:

1. **Observation boundary.** Declare the boundary as Tofino switch 1's port facing the DUT. Measured OWD then includes the two short cable runs and the DUT NIC/PHY, which belong to the DUT path and are not subtracted.
2. **Stamp-point calibration.** If the Tofino SDE supports a port loopback on P_DUT, send registered frames through it with no DUT and measure the `before`→`after` offset at each frame size and at the target loads. If no loopback is available, do not subtract anything; carry the worst-case stamp-point offset in u.
3. **Rate calibration** as in section 4, before and after each session; a step or drift outside the bound invalidates the affected cells.
4. **Cross-check at the 10G stage.** Configure a mirror of P_DUT in both directions to host C and timestamp both copies with host C's CX-4 RX clock (the "calibrated mirror" alternative in HNG-OBSERVER section 2.1). Three streams of 10G offered load fit on host C's 100G port; at 100G they do not (300G on one 100G port), so this cross-check exists only at 10G. Require the two methods to agree within their combined bounds.
5. **Full-flow evidence.** The sink on host C receives every packet with its identity and both Tofino timestamps; the streaming oracle (HNG-OBSERVER section 10.1) checks order, loss and Q/trace decisions on all packets and keeps the 1/1024 main sample in full.
6. **Variant B only:** bound the aggregation switch transit by a bypass run through the same path with the DUT forwarding at zero configured delay, and report it; do not subtract the DUT's own zero-delay residence.

Error budget per OWD pair (all *estimate*, to be replaced by measured bounds in `calibration.json`):

| Term | Bound | Basis |
|---|---:|---|
| Stamp-point offset, egress-pipeline vs ingress-MAC, uncorrected | 2 µs | Includes 123.4 ns serialization of 1,542 line bytes at 100G (`1542 × 8 / 100e9`); pipeline/MAC latency ***unverified*** |
| Clock rate after calibration, up to 512 ms OWD | 0.3 µs | Section 4 arithmetic; longer trace delays scale linearly |
| Quantization | 0.1 µs | Assumes ns-scale counter resolution, ***unverified*** |
| Load-dependent MAC/FIFO delay at the stamp points | 2 µs | Measured at target loads in step 2 |
| Residual/unknown | 2 µs | Reserve |
| **Total** | **6.4 µs** | ≤ 10 µs target; 6.4% of the 0.1 ms median threshold, 1.3% of the 0.5 ms p99 threshold |

Variant B adds the aggregation switch transit term; the mirror cross-check adds the mirror-queue difference between the two copies. Both must be measured before use.

## 6. Claims this testbed cannot support (d)

- **Bidirectional 100G line rate** (100G in each direction at once). Host C and the DUT path each have one 100G port; variant A carries at most 100G into and 100G out of the DUT in total. The two modeled directions can only share that 100G.
- **The 200G-aggregate PCIe/NUMA case.** Variant A uses one Gen3 x16 adapter; variant B is one direction at a time.
- **An independent passive observer.** The Tofino switch is independent of the DUT but is an active, in-path device that also forwards the generator traffic. Report "switch-hardware timestamps at the DUT-facing port", not passive tap observation.
- **Nanosecond-level or sub-microsecond accuracy.** The budget is microsecond-scale (6.4 µs estimate, ≤10 µs target).
- **Generator-side OWD.** CX-4 has no TX wire timestamp, so no OWD from generator send to sink receive.
- **Bare-metal results.** All three hosts are hypervisors shared with other workloads; the DUT runs in a VM with PCI passthrough (section 15). Results are for that registered VM topology.
- **100G with the full 462–512 ms long-delay storage, until hugepages are reserved.** One 100G direction at 462 ms needs `8.106e6 × 0.462 = 3.745 M` mbufs, `× 2304 B = 8.63 GB`; at 512 ms 9.56 GB. Host A's DUT-adapter node yielded only 2,587 of 4,096 requested 2 MiB pages (5.05 GiB) after compaction in the testbed record. A boot-time reservation by the testbed owner is needed first.
- **100G generator capacity is not established.** Host C has 6 physical cores; whether it can generate, receive and record 100G of 1,542-byte frames (8.106 Mpps) is *unverified* until its bypass run passes. If it cannot, `R_valid` is limited by the generator and is reported as such.
- Unchanged from the design: no small-packet 100G claim, no overload of C = 100G from one 100G ingress, and no UTC-traceable absolute time.

## 7. Storage (recomputed with the streaming-oracle default)

Default retention is the streaming oracle of [HNG-OBSERVER.md](HNG-OBSERVER.md) section 10.1: online statistics, histograms, capped mismatch records and full records only for the 1/1024 main sample (524 B per sampled packet worst case). On this testbed (one 100G direction, 1,542 line bytes, 600 s):

- Samples: `8.106355e6 / 1024 × 600 = 4.750e6`; records `4.750e6 × 524 B = 2.489 GB`; plus the 0.280 GB mismatch cap = 2.769 GB; with 25% margin `3.46 GB`. **Provision 4 GB per ten-minute 100G cell.**
- 10G stage: `0.249 + 0.280 = 0.529 GB`, `× 1.25 = 0.66 GB`; provision 1 GB per cell. The mismatch cap dominates.
- Optional debug window (full per-packet recording, at most 2 s per cell): ≤12 GB each at the bidirectional-100G reference load, so also ≤12 GB here.
- Total = 4 GB × (100G cells) + 1 GB × (10G cells) + 12 GB × (debug windows), on the host that runs the oracle (host C in variant A), plus about 100 GB on the DUT for builds and logs (*estimate*).

Full per-packet recording for a whole cell would be `280.36 B × 8.106e6/s × 600 s = 1.364 TB` per one-direction ten-minute cell; it is not planned.

## 8. Scheduling and open questions (e)

- The testbed belongs to and is operated by another team (the testbed owner). Only one operator works on it at a time; P1 sessions never connect to it.
- **P1 gets no testbed time until the Stage 2 main scenario finishes.** The testbed owner schedules M5 and later after that; the earliest window is estimated at **mid-to-late November 2026** (*estimate*).
- The Tofino switches are also used under a separate loan arrangement; any P4 program swap on Tofino switch 1 must fit that arrangement.

Open questions for the testbed owner:

1. Free disk space on host A and host C (and in the VMs that would run the DUT and the oracle); section 7 gives the need per cell.
2. RAM per NUMA node on host A, and whether a boot-time reservation of node-local 1 GiB or 2 MiB hugepages (32 GiB requested, section 11) is possible on the DUT-adapter node.
3. Whether the DUT may run bare metal on host A or only as a VM with PCI passthrough, and which cores can be reserved (other teams' VMs share the host).
4. Whether a P1 timestamping P4 program, or the Stage 2 program if it already exposes these timestamps, may run on Tofino switch 1 during P1 windows; and whether SPAN mirroring of the DUT-facing port is allowed for the 10G cross-check.
5. Whether the Tofino SDE/NOS exposes ingress MAC and egress timestamps and a port loopback mode (section 9, item 7).
6. For variant B: whether the down 100G link from host A's second adapter to the aggregation switch may be enabled.
7. Whether host C's PCIe retrain after VM start is reliable at every session start.
8. Window length per session and how many sessions; the matrix of section 14 is sized to fit them.

## 9. Pre-flight checklist (read-only, for the testbed owner)

All checks only read state; none changes configuration. Replace `<...>` with local values; keep the results in the private deployment manifest.

1. Timestamp capability per DUT and TG port: `ethtool -T <port>` (hardware receive/transmit, PHC index).
2. Adapter identity and firmware: `ethtool -i <port>`, `devlink dev info <pci>`; real-time clock setting if present: `mlxconfig -d <pci> query` (look for `REAL_TIME_CLOCK_ENABLE`).
3. PCIe link: `lspci -vv -s <pci>` (`LnkCap`, `LnkSta`; expect 8 GT/s x16, watch for 2.5 GT/s).
4. NUMA and memory: `cat /sys/bus/pci/devices/<pci>/numa_node`, `numactl -H`, `grep -i huge /proc/meminfo`, per-node `free_hugepages`.
5. CPU and disk: `lscpu`, `df -h <data path>` on DUT and oracle hosts and VMs.
6. Link: `ethtool <port>` (speed, FEC), a baseline of `ethtool -S <port>` counters.
7. Tofino switch 1: NOS/SDE version; whether the P4 toolchain exposes ingress MAC and egress timestamps and port loopback; current mirror sessions (read-only listing).
8. Clocks: `chronyc tracking` or `timedatectl timesync-status` on the DUT and TG hosts; whether `ptp4l`/`phc2sys` run.
9. Software: `uname -r`, installed rdma-core and DPDK versions.
10. Hypervisor: which VM currently holds each 100G adapter by PCI passthrough (read the VM configuration only).

## 10. Source baseline and implementation limits

| Source | Pinned revision and inspected material | Consequence |
|---|---|---|
| HNG contract | This branch's `d5df0b7`, [HNG-DESIGN.md](HNG-DESIGN.md), including trace/VLAN additions | Four independent `(line, direction)` C/Q states, long-delay storage and full-flow observation requirements |
| [DPDS host][host] | `5cd3b6b23307d39a7a0a4dc48c54520faa5cc17b`; README, `dpds.c`, common configuration, receive/transmit paths, Meson and Dockerfile | **Code:** one or two ports, 2–4 lcores, one RX/TX queue per port. One scheduler worker owns up to four line/direction states. Additional CPU cores do not automatically parallelize the engine |
| [DPDS-Core][core] | `9531a95e15b13801e117d2a0441f8d26d8d16ee4`; Cargo metadata, README and HNG engine/trace/buffer sources | **Code:** Rust scheduling is independent of DPDK and physical NICs. Two packet FIFOs and a service ledger reserve capacity through the ordinary allocator; mbuf hugepages are not the whole memory requirement |
| Stage 1 target-check reference (not public; location in the private deployment manifest) | `de035a63f5d0135ee70b0bceb27d5051897b85a1`; target-check document/runner and `C2/harness/target_rate_metrics.py`, `target_rate_check.py`, fetched and read without execution | **Code:** capture before/after the emulator in one VM clock domain, pair by flow and sequence, compare with t plus the selected-late increment. This revision is awaiting a second review; pin it as a method reference, not an approved hardware result |

**Code:** the host records relative software RX time from `host_now_ns()` and TX-submit lateness. These are not per-packet wire timestamps. The complete independent observation/export/pairing path required in section 14 is still a software-stage deliverable.

**Estimate and M4 requirement:** bidirectional 100G at 1,542 line bytes is `2 × 8.106 Mpps`, leaving only **61.68 ns/packet** for the current single scheduler. A 60 ns median microbenchmark has almost no aggregate-rate margin, even before tails and host work. The eight-core reservation assumes a subsequent **one-scheduler-per-direction** layout, each owning that direction's cable and LEO states and ordered egress. Those states are independent; this split preserves each shared C/Q. It is not implemented by the inspected revision. M4 must measure end-to-end and tail costs and may revise the core count/layout; do not present the single-owner 60 ns target as proof of bidirectional 100G readiness.

**Code:** the small default configuration is not a 100G configuration. Pool sizing uses configured storage and the adjusted RX/TX descriptor counts, and startup reports object footprint. It does not reserve 32 GiB automatically. Defaults include 256 RX/TX descriptors, 1,024-entry handoff rings and 256 pending TX packets per line; none is a demonstrated 100G setting.

**Code:** retry defaults to 1 s, with 5 s limits on actual TX hold/shutdown. Long outages and drain intervals need software validation and, where necessary, configuration or implementation changes before hardware use. Resource timeout must not silently become trace loss.

## 11. DUT CPU, NUMA, memory and hugepages

**Testbed note:** on host A the DUT adapter A1 sits on one NUMA node; place the data-plane cores, pools and Rust metadata there. Today that node does not provide 32 GiB of free hugepages (section 6); the figures below are the requirement to agree with the testbed owner.

| Item | Requirement or check | Evidence |
|---|---|---|
| Physical cores | Reserve eight data-plane cores plus control/observer cores; recommend at least 16 total. This assumes M4 later provides one scheduler per direction, with independent RX/TX work as needed. Current host layouts use only 2–4 lcores; M4 measurements may change the reservation. Do not count SMT threads as physical cores | Decision, estimate and code; aggregate-rate budget in section 10 |
| CPU frequency | Prefer a server CPU sustaining roughly 2.5–3 GHz or higher; record frequency, throttling and power policy | Estimate; no measured minimum CPU model or GHz |
| NUMA placement | Keep both data ports, host lcores, mbuf pools and Rust metadata on the same node where possible; place observer work on separate cores with adequate local memory bandwidth | [DPDK NUMA guidance][numa]; placement is an estimate |
| Cross-node alternative | Local RX pools do not prevent remote egress DMA through the other node. Register this as a different topology and report its cost | Test design; acceptance of supplied topology TBD |
| Total RAM | At least 64 GiB; preferably 64 GiB available on the target node. A dual-socket system with 32 GiB per node does not satisfy 32 GiB local hugepages plus ordinary-memory headroom | Estimate; actual DIMM distribution TBD |
| Memory channels | Populate the target node's memory channels according to the CPU/system manual; record DIMM population and rate | [DPDK performance guidance][numa]; exact DIMM count TBD |
| Preferred hugepages | 32 × 1 GiB pages on the selected node, where supported; reserve through an administrator-approved boot/maintenance procedure | [DPDK hugepage guidance][sysreq]; 32 GiB is the project starting allocation |
| Alternative hugepages | 16,384 × 2 MiB pages = 32 GiB. If a registered topology needs 16 GiB on each of two nodes, allocate 8,192 × 2 MiB per node | Specification and arithmetic estimate; split follows actual pool demand |
| Ordinary RAM | Budget Rust FIFO/ledger reservations, OS, trace data and observer rings/caches outside hugetlb. Sum all four states' configured reservations, not only live occupancy | Code |
| Competing work | Exclude unrelated work on reserved cores/SMT siblings, swapping and uncontrolled VM overcommit; keep IRQ/control work away from the scheduler owner | Test design; no such settings have been applied |

**Estimate:** at 100G, a 1500-byte IP packet without VLAN occupies 1,538 bytes on the wire, approximately 8.127 Mpps per direction. With one retained VLAN tag, L2 including FCS is 1,522 bytes and line accounting is 1,542 bytes, approximately **8.106 Mpps**.

For `142 ms base delay + 320 ms late increment + illustrative 50 ms Q residence`, a conservative bidirectional bound is **8.301 million packets**. At an illustrative **2,304 bytes per mbuf object**, that is **19.125 GB = 17.812 GiB**, before pool-private metadata/alignment, Rust allocations, rings, caches and observation. This is the preliminary P0-style envelope, not the measured stride of the current build. M4 must check actual allocation and startup footprint against its calculator before hardware qualification.

**Estimate limit:** 32 GiB does not guarantee zero storage loss for small packets at 100G and long delay. Minimum 64-byte L2 frames occupy 84 line bytes; the same bidirectional 512 ms bound is 152.381 million packets, or **351.086 GB** at 2,304 bytes/object alone. Start 100G qualification with registered MTU-1500 synthetic traffic; report a separate achievable-rate and memory envelope for ACK/small-packet mixes. A 64-byte minimum accounting setting does not make a large-packet pool calculation safe for all traffic.

**Usable planning boundary (estimate):** reserve 20% of the 32 GiB hugetlb budget for pool overhead, descriptors/rings and sizing error; use at most `floor(0.8 × 32 GiB / 2304) = 11,930,464` long-delay mbufs. With equal directional budgets and all packets conservatively held for `142+320=462 ms`, the ceiling is **12.912 Mpps/direction**. At 100G this requires packet-count-weighted mean line size ≥**968.11 B**, or mean L2 including FCS ≥**948.11 B**. With the illustrative additional 50 ms Q residence, use **11.651 Mpps/direction**, mean line ≥**1072.89 B** / L2 ≥**1052.89 B**. At minimum 84 line bytes, the latter is only **7.829 Gbit/s/direction**. Without the 20% reserve, the absolute mbuf-only 462 ms ceiling is 16.140 Mpps/direction / 774.49 line bytes; it is not an operational limit.

Use the general bound `pps_dir <= floor((hugetlb_bytes - measured_reserves)/measured_stride)/(2 × maximum_residence_seconds)`. These are steady-rate planning limits, not guarantees from a mean alone: a small-packet burst must also fit the maximum packet count in every residence-time window, and each line's configured pool share must suffice. Rust allocations remain outside this budget. Recompute with M4's actual stride, registered Q/outage residence and burst envelope before M7.

**Design requirement:** include every line/direction's Q and storage limits. Trace storage uses the registered maximum trace delay and outage hold bound, not the fixed-mode 462 ms delay bound. Resolve long-outage capacity/drain issues in the software stage. Report Q both in L2 bytes and drain time at C; include VLAN, padding and FCS, excluding preamble/IFG. The final shallow/deep Q values are **TBD**, not fixed to the illustrative 50 ms.

**Estimate:** one DMA write and read per packet already approaches 50 GB/s of memory traffic for bidirectional 100G. One additional full packet copy adds roughly another 50 GB/s. Observation traffic must be budgeted too. Storage needs depend on the streaming-oracle versus full-event-retention choice; the default is streaming (section 7), and recording every payload is not required.

## 12. OS, kernel, DPDK and driver constraints

| Item | Constraint or candidate | Evidence |
|---|---|---|
| OS candidate | Ubuntu Server 24.04 LTS x86-64, GA kernel 6.8; freeze exact package revisions | [Ubuntu kernel specification][ubuntu]; candidate selection is an estimate, not a qualified combination |
| DPDK | Pin **26.03** for this build and qualification. Verify pkg-config and actual linked libraries | Code: host README names 26.03, but Meson's `dependency('libdpdk')` itself has no version constraint |
| Global kernel floor | DPDK 26.03 requires **Linux ≥5.4**; individual features/PMDs can require more | [DPDK requirements][sysreq]; a minimum is not a performance recommendation |
| mlx5 upstream prerequisites | Common driver documentation lists kernel ≥4.14 and rdma-core ≥16 on 64-bit systems. Combined with the global floor, 4.14 is not a valid kernel choice for this qualification | [mlx5 common specification][mlx5-common] |
| Tested-platform evidence | The 26.03 NVIDIA list includes Ubuntu 24.04, upstream kernel 6.18.0+, rdma-core 60.0+ and a DOCA 3.3.0-088000+ entry. `MCX623106AN-CDAT` is listed with firmware 22.48.1000+. CX-4/CX-5 are not in that tested-NIC subsection | [DPDK release notes][release]; neither proof for GA 6.8 with arbitrary verbs packages nor universal minimum requirements |
| CX-4 vendor-stack limit | DOCA 2.9.0 and its LTS branch are the last DOCA-Host line supporting CX-4. Do not select DOCA 3.x for it based on the newer-NIC test list. An upstream kernel/rdma-core route is a separate candidate to qualify | [NVIDIA DOCA specification][doca-cx4]; exact stack/firmware TBD |
| Repository container | Dockerfile declares Ubuntu 26.04, DPDK 26.03 and Rust 1.96. Explicit packages omit libibverbs-dev/rdma-core; verify the actual built driver inventory includes `net_mlx5` | [Code][docker]; no image was inspected, so driver presence/absence is not established. Container userland does not replace the host kernel |
| Build/runtime dependencies | C compiler, Meson, Ninja, pkg-config, Python/pyelftools, libnuma, Rust/Cargo/cbindgen; libpcap for pcap tests; libibverbs/libmlx5 and kernel modules for mlx5 | Code and [DPDK][sysreq]/[mlx5][mlx5-common] specifications; record tool versions, Cargo.lock and build flags |

**Specification:** mlx5 uses a bifurcated driver. Keep `mlx5_core`, `mlx5_ib`, `ib_uverbs` and the userspace verbs/provider path. Do not automatically unbind these ports and attach them to vfio-pci. [mlx5 guide][mlx5-common], [DPDK driver guide][drivers]

**TBD:** freeze an exact supported kernel/rdma-core or vendor-stack/firmware combination for the supplied OPN. Driver-family support and a tested platform are different evidence. This request claims no on-machine validation of either.

## 13. Stage A: software gate before physical hardware

**Decision:** M1–M4 functional development, correctness fixes, trace conversion and observation-tool tests stay on the development workstation. Hardware is for accuracy and throughput qualification after this gate.

| Software test | Required exercise | Pass evidence and basis |
|---|---|---|
| Core fake clock and independent oracle | C/Q before delay, byte exact-fit/overflow, Q release at service completion, direction/line isolation, fractional service, stable ties, seed replay and input frontier | Exact independent-reference agreement; fixed late increment is exactly δ at the scheduling level; HNG-DESIGN section 8 |
| Trace policy | Delay step UP and DOWN; equal clamped deadlines after a decrease retain ingress order; loss, outage drop/hold, terminal HOLD, coverage and VLAN mapping | All fixtures pass with separate loss reasons; arbitrary per-packet late amounts remain a deferred interface and unsupported configurations are rejected |
| `net_null` | EAL, pool lifecycle, 2/3/4-lcore layouts and cleanup; valid tagged-mbuf fixtures cover paths that zero-content packets cannot | Null-generated RX and discarded TX do not establish packet forwarding correctness. [Null PMD specification][null] |
| `net_ring` | Known VLAN/sequence packets through ethdev, captured output, ring pressure, backpressure and order | M1: at least 10^7 packets with four lcores; independent-reference checks across at least 32 seeds. Native rte_ring unit tests alone are not net_ring PMD integration. [Ring PMD specification][ring] |
| `net_pcap` | Two virtual ports, tagged fixtures in both directions; bytes, VLAN mapping, identities, ordering and drain | Independent model agreement across registered fixtures/seeds. Original pcap timestamps do not automatically provide pacing; inject model time explicitly. Write to new output files. [Pcap PMD specification][pcap] |
| `af_packet` | Low-rate bidirectional I/O on isolated local namespaces/veth; VLAN and host mapping | No physical testbed dependency. CAP_NET_RAW/namespace permissions arranged locally; missing permissions are BLOCKED/SKIP, not PASS. [AF_PACKET specification][afpacket] |
| Faults and lifecycle | Zero/partial/full fake TX returns, exhausted rings/pools, pending late packets at stop, trace drops with outstanding service entries | No leaks/double frees/duplicate TX; packet/byte/mbuf conservation and explicit layer counters |
| Trace conversion, M3 | Provenance/hash and registered delay, direction, loss/outage semantics; compare the same source segment against the Stage 2 reference | Independent deadline agreement, downward-step order preserved; no unregistered trace splicing; only redistributable or synthetic fixtures in the repository |
| Observation tools, M2 | Known-time fixtures, identity loss/duplicates, clock errors, ordinary packet swap, storage loss, injected 0.2 ms delay and 1% observer-event loss | Detect each fault; uncertainty/missing evidence gives INCONCLUSIVE. At least 10^6 events compared with an independent model. A production-computed deadline is not an independent oracle |
| Capacity/performance preparation, M4 | Measure allocation/stride, validate the pool calculator, reassess TX pending/retry/hold limits; fixed/trace scheduling microbenchmarks | Roadmap targets: calculator within 5% of small allocations and scheduler p50 ≤60 ns/packet. Check the 61.68 ns aggregate budget and proposed per-direction split separately; these workstation targets are not physical throughput evidence |

**Gate A evidence:** host/core SHA, Cargo.lock, DPDK/PMD versions, logs for the necessary paths of all four virtual devices, trace/config/fixture hashes and conservation results. Complete M1–M4 and the applicable OS1–OS6 checks. Unimplemented, skipped or permission-blocked work cannot be reported as a passed path; resolve it before claiming this complete gate.

**Code status:** the inspected host suite has injected-TX, real-mbuf/ring and null-layout tests; its README treats pcap replay and hardware checks as separate work. That does not prove all required net_ring/pcap/af_packet/observer paths have passed. This documentation task did not rerun or sign off those suites.

## 14. Stage B: qualification sequence and thresholds

### 14.1 Qualification sequence

These are **test-design requirements for later execution**, not actions performed by this task. On this testbed, "both directions simultaneously" means both modeled directions sharing the one 100G DUT path (section 3); per-direction 100G is tested one direction at a time.

1. Confirm Gate A, dedicated resources and access. Freeze software, firmware, topology, pool size and frame envelope. Record private environment values outside source/defaults.
2. Qualify generator/receiver bypass and zero-added-delay observation at the target loads. Require zero bypass drops. Measure offset, drift, timestamp brackets and observer/oracle loss; roadmap M5 requires clock uncertainty **≤10 µs**. Do not fit away emulator tails after the run.
3. Start at **10G offered rate**, each direction separately and then simultaneously, with cable and LEO VLANs. Primary cells are the registered Stage 2 operating points: modelled corridor A cable 65 ms and LEO about 74–80 ms; corridor B cable 100 ms (80/120 ms sensitivity) and LEO about 135–141 ms. Use the Stage 1 late-increment grid for fixed-delay cells. Also cover P0 boundaries: t=65/142 ms, p=0, p=0.01/0.10 with δ=5/320 ms, δ=0 and p=1. Trace cells use registered replay semantics, including UP/DOWN and loss/outage profiles already tested in software; do not combine trace with unsupported arbitrary late amounts.
4. First use no deliberate queue buildup, then C=8–9G shared-Q stress at 10G offered load. Hardware counters and the oracle establish validity of accuracy/throughput results; functional development is already complete.
5. Warm up longer than the maximum delay/hold/Q residence. Measure at least 10 minutes of steady state per cell with independent repeats; three repeats are the initial estimate. Retain fill, transition and drain windows separately. Register exact Q values, matrix, seeds and durations before running.
6. Only after all required 10G cells pass accuracy and validity, increase to **100G offered rate** with the same enabled observation, first registered MTU-1500 synthetic traffic, then separately bounded mixed-packet/multi-flow workloads. Repeat one-way and simultaneous two-way tests; C=80–90G is the stress example. No mandatory 40G stage.
7. Report `R_valid`, the maximum rate satisfying accuracy and loss/order validity together. Throughput with failed accuracy is not usable 100G qualification.

### 14.2 Thresholds and expected delay

**Decision:** evaluate each direction, line and registered required cell separately.

| Metric | Requirement |
|---|---|
| Median error | `median(abs(OWD_i - expected_i)) <= 0.1 ms` |
| Tail error | `p99(abs(OWD_i - expected_i)) <= 0.5 ms`; not p99 of signed error |
| Measurement validity | Clock uncertainty ≤10 µs and cannot change the verdict; unbounded missing samples/clock errors produce INCONCLUSIVE |
| Loss/order validity | Zero delay-storage drops and zero unintended reordering. Report intended Q/trace loss separately. Non-model NIC/pool/TX loss prevents full acceptance |
| Throughput | Per-direction offered/delivered L1, L2 and pps, plus L7 where applicable. Bidirectional 100G means 100G in each direction, not 100G aggregate; this testbed cannot show it (section 6) |

**Test design:** without deliberate queueing, ordinary `expected=t`; selected-late `expected=t+δ`. Report both the overall distribution and ordinary/late groups. Define timestamp boundaries before testing and retain raw measurements. Additional egress serialization when late and ordinary packets collide **remains in G2 error**; also report that component separately, without moving the deadline.

For stress, an independent C/Q oracle supplies `expected_i=e_i-a_i+t+Iδ`, where `a_i` is ingress time and `e_i` is service completion. For trace, compute `deadline_i=max(e_i+t(e_i), deadline_{i-1})`, apply the registered outage action to obtain `release_i`, then use `expected_i=release_i-a_i`. Use the same declared observation boundaries and clock mapping throughout. Report monotone-clamp delay separately from implementation error; comparing directly to raw trace t would misclassify legitimate clamp behavior.

**Code compatibility:** Stage 1 revision `de035a6` already computes the median and p99 of absolute errors against `t + selected_late * delta`. Its quantiles linearly interpolate sorted finite samples at `(n-1)*q`; use the same convention in the P1 reporter. Do not substitute `abs(median(OWD)-t)`, which is a different statistic.

**Code and test design:** retain Stage 1's PASS/FAIL/INVALID distinction: invalid evidence takes precedence within a row; an overall valid failing row gives FAIL, otherwise missing/invalid required rows give INVALID. P1's **INCONCLUSIVE** measurement state maps to **INVALID** in that compatible report, with explicit reasons; it never becomes PASS. The referenced target check has no intended shared C/Q and records that layer as not applicable, not a fabricated zero. P1 must supply real Q/trace counters and independent oracle evidence instead. The reference uses low-rate probes under concurrent load; P1's target-load and full-flow requirements still apply. Reconcile any changes from the pending Stage 1 second review before freezing the tooling.

## 15. Access, configuration and bring-up boundary

| Item | Required arrangement | Evidence |
|---|---|---|
| Operator | Only the testbed owner's operator works on the testbed, one session at a time; P1 development sessions never log in. P1 supplies builds, manifests and the section 9 checklist | Decision |
| Management network | Separate management NIC/BMC and subnet; SSH does not traverse the emulated queue/outage path | Estimate; subnet assigned by the administrator |
| Data network | Direct point-to-point L2 is sufficient. TG supplies configured VLANs; IP flows use an isolated experiment subnet | Estimate; VLAN IDs and IP ranges are parameters, not embedded defaults |
| Root/sudo | Scoped administrator assistance for hugepages/mounts, memlock/device permissions, affinity/IRQ, drivers/firmware, timestamps and reboot. Ordinary account for build/analysis | Specification and estimate; direct root SSH/password is not required |
| Bare-metal mlx5 | Keep the mlx5/verbs path; arrange device/capability/memlock permissions for non-root execution | [mlx5 specification][mlx5-common]; do not apply generic devbind instructions blindly |
| IOMMU/VFIO | Bare-metal mlx5 does not require vfio-pci. Other VFIO PMDs use an IOMMU-backed setup. PF passthrough to a VM needs host IOMMU and safe groups; the guest uses the appropriate PMD driver | [DPDK specification][drivers]; no-IOMMU workarounds are not the requested baseline |
| VM fallback | PF passthrough, dedicated vCPU/RAM/vNUMA, host hugepages and no ballooning/overcommit; retain host and guest manifests | Estimate; a different qualification topology, not an assumed equivalent to bare metal |

**Decision, OS1–OS6:** preserve Apache-2.0 and upstream notices, add SPDX to new files, keep scheduling independent of DPDK and document public interfaces. Machine PCI addresses, CPU lists, hugepage locations, credentials and network values belong in operator-supplied configuration/parameters. Examples must use documented placeholders or synthetic values. No site-specific deployment script or live configuration is added by this request.

**Later bring-up checklist:** an authorized operator inventories the supplied topology and versions, reserves the node-local resources, confirms the required PMDs exist, checks link/FEC/MTU and counters, qualifies bypass and clock uncertainty, then freezes the manifest before section 14. Detailed commands must follow the selected driver and observer, with all machine values parameterized. This document provides the order and acceptance evidence; it does not claim that a runnable provisioning procedure exists for an unknown machine.

**Task boundary:** documentation only. No remote login, inventory, firmware/kernel/BIOS changes, hugepage allocation or hardware tests were performed. No new engine API, configuration schema or runtime dependency was introduced.

## Appendix A. Future hardware (not available)

Kept for a possible later request; **none of this exists or is requested now.** Primary: three machines (DUT, dual-port traffic generator/receiver, passive observer) with ConnectX-6 Dx `MCX623106AN-CDAT` (2×100GbE, Gen4 x16) adapters ([spec][cx6]), two passive duplex single-mode optical taps on two 100G optical links, and two observer cards each pairing the before/after monitor streams of one direction on one clock. Backup: two machines, where the generator's CX-6 Dx provides hardware TX-port and RX timestamps ([timestamps][timestamps]). Alternative DUT adapters: ConnectX-5 Ex `MCX516A-CDAT` ([spec][cx5]) or two single-port ConnectX-4 `MCX455A-ECAT` on independent Gen3 x16 paths ([spec][cx4]). Sizing estimates were ≥64 GiB RAM / ≥16 cores for DUT and generator and ≥128 GiB for the observer. This would enable bidirectional 100G and passive observation, which section 6 excludes on the testbed.

## Sources

Vendor/DPDK sources checked on 2026-10-29. Pinned code, project decisions and calculations are distinguished above. No third-party code or datasets are redistributed by this document.

[host]: https://github.com/markhhm/DPDS/tree/5cd3b6b23307d39a7a0a4dc48c54520faa5cc17b
[core]: https://github.com/markhhm/DPDS-Core/tree/9531a95e15b13801e117d2a0441f8d26d8d16ee4
[docker]: https://github.com/markhhm/DPDS/blob/5cd3b6b23307d39a7a0a4dc48c54520faa5cc17b/.devcontainer/Dockerfile
[cx6]: https://networking-docs.nvidia.com/connectx6dxhw/introduction
[cx5]: https://docs.nvidia.com/networking/display/nvidia-connectx-5-ethernet-adapter-cards-user-manual.pdf
[cx4]: https://networking-docs.nvidia.com/connectx4vpihw/specifications
[pcie]: https://pcisig.com/faq?field_category_value%5B%5D=pci_express_3.0
[mlx5]: https://doc.dpdk.org/guides-26.03/nics/mlx5.html
[mlx5-common]: https://doc.dpdk.org/guides-26.03/platform/mlx5.html
[sysreq]: https://doc.dpdk.org/guides-26.03/linux_gsg/sys_reqs.html
[numa]: https://doc.dpdk.org/guides-26.03/linux_gsg/nic_perf_intel_platform.html
[drivers]: https://doc.dpdk.org/guides-26.03/linux_gsg/linux_drivers.html
[release]: https://doc.dpdk.org/guides-26.03/rel_notes/release_26_03.html
[doca-cx4]: https://networking-docs.nvidia.com/doca/archive/2-9-4/changes-and-new-features
[ubuntu]: https://ubuntu.com/kernel/lifecycle
[timestamps]: https://networking-docs.nvidia.com/mlnxenswum/24070610/time-stamping
[null]: https://doc.dpdk.org/guides-26.03/nics/null.html
[ring]: https://doc.dpdk.org/guides-26.03/nics/ring.html
[pcap]: https://doc.dpdk.org/guides-26.03/nics/pcap.html
[afpacket]: https://doc.dpdk.org/guides-26.03/nics/af_packet.html

