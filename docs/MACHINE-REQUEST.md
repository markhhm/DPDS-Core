<!-- SPDX-License-Identifier: Apache-2.0 -->

# P1 hardware request: delay accuracy and throughput acceptance

Date: 2026-10-29. Revised after the M5a review.
Status: requirements and measurement plan only; no hardware accessed or configured.

## Equipment to prepare

This is the **estimated starting bill of materials**, with the primary observation scheme below. Existing compatible equipment can replace the named candidates after qualification.

| Resource | Primary request | Alternative / evidence |
|---|---|---|
| Machines | Three: one DUT, one dual-port traffic generator/receiver (TG), one passive observer | Two machines if the TG hardware-TX timestamp backup qualifies; no existing testbed allocation |
| DUT NIC | One CX-6 Dx `MCX623106AN-CDAT`, 2×100GbE, Gen4 x16 | One CX-5 Ex `MCX516A-CDAT`, or two single-port CX-4 `MCX455A-ECAT` on independent Gen3 x16 paths; vendor specifications in section 3 |
| TG NIC | One `MCX623106AN-CDAT`, 2×100GbE, Gen4 x16 | Same bus budget as DUT; hardware timestamp backup specifically needs CX-6 Dx or a qualified equivalent |
| Observer NICs | Two `MCX623106AN-CDAT`, each 2×100GbE, each Gen4 x16; one card per direction, its two RX ports observing before/after DUT | Four simultaneous 100G monitor streams. No subtraction across the two cards' raw clocks; same-card clock sharing and capture capacity must be verified |
| Links | Two 100GbE optical data links with two passive duplex single-mode taps, four unaggregated monitor outputs, compatible 100G-LR4 optics (four endpoint plus four monitor RX modules), OS2 LC patch leads; start with 3 m leads, final lengths and split/loss budget checked by supplier | For the two-machine backup: two 100GbE QSFP28-compatible DACs, 3 m, or AOCs of registered length. PCIe generation applies to the cards/slots, not cables. Sealed DAC/AOC links cannot accept the primary passive optical taps |
| RAM / CPU | DUT: ≥64 GiB RAM, 32 GiB local hugepages, ≥16 physical cores with eight reserved for data plane. TG: initially ≥64 GiB / ≥16 cores. Observer: initially ≥128 GiB / ≥16 cores, split by its two NIC nodes | Estimates, not throughput guarantees. TG/observer have no long-delay store but need bounded join/capture buffers; M4 and timestamp-on bypass determine final needs |
| Working storage | DUT: 100 GB free for builds/logs. TG/observer: ≥1 TB free each if retaining full events, with schema-based write-rate sizing; streaming-only: initially 100 GB free each | Estimates: streaming retains bounded mismatches, summaries and sampled timing, not full payload/event history. Exact M2 record widths may require more than 1 TB |
| Observation | Primary: four passive monitor streams into two same-clock RX pairs. Backup: TG CX-6 Dx hardware TX-port/RX timestamps | Both target ≤10 µs combined uncertainty; neither has been measured. Section 8.1 defines equipment, calibration and rejection conditions |

## 1. Request and two-stage boundary

**Complete functional and correctness tests on the development workstation using DPDK virtual devices. Then use a separate physical machine only for delay accuracy and throughput acceptance.** P1 must not occupy the existing Stage 2 testbed before its main scenarios finish. This supersedes the equipment-sharing assumption in [HNG-DESIGN.md](HNG-DESIGN.md) section 9; its engine contract and G2 thresholds remain applicable.

Evidence labels used throughout:

- **Decision:** the project instructions of 2026-10-29, including the 20:00 split between software testing and hardware acceptance, and roadmap revision `4b07aea`, M1–M7 and OS1–OS6.
- **Code:** behavior read at a pinned source revision, not a performance measurement.
- **Specification:** linked primary vendor or DPDK documentation.
- **Estimate:** calculation or engineering recommendation that has not been measured.
- **Test design:** a proposed procedure derived from the HNG contract; final run parameters must be registered.
- **TBD:** information or evidence required before qualification.

| Item | Requested configuration and limits | Evidence |
|---|---|---|
| Device under test (DUT) | One dedicated Linux x86-64 bare-metal machine; two exclusive 100GbE data ports and separate management networking. Start with 10G offered traffic, then 100G | Decision; bare metal is an estimate to reduce scheduling interference |
| NIC and PCIe | Prefer one existing dual-100G Gen4 x16 adapter; alternatively two single-100G adapters, each with an independent Gen3 x16 path, preferably on the same NUMA node | Specification and estimate, section 3 |
| CPU | Recommend at least 16 physical cores overall. Reserve eight physical data-plane cores on the NIC node, plus separate control/observer cores. The current host uses at most four lcores | Decision: roadmap M5a reserves eight data-plane cores; estimate: 16 total; code: current layout limit |
| RAM and hugepages | At least 64 GiB RAM, preferably 64 GiB available on the selected node. Initially reserve 32 GiB hugetlb: 32 × 1 GiB pages, or 16,384 × 2 MiB pages | Decision and estimate; workload bounds and accounting in section 4 |
| Software candidate | Ubuntu Server 24.04 LTS with GA 6.8 kernel; pin DPDK 26.03. Qualify and freeze the exact kernel, verbs/provider and firmware combination | Estimate and code; not an already validated platform, section 5 |
| Traffic and observation | A separate dual-100G generator/receiver, or equivalent tester, with calibrated ingress/egress timestamps in one clock domain | Test design; a DUT with null PMD alone cannot qualify physical 100G |
| Access | SSH keys and an ordinary account; administrator support for scoped sudo, hugepages, affinity, drivers and maintenance/reboots | Estimate; management subnet, account and access window are TBD |

The supplied machine, slot topology, NIC ordering part number (OPN), firmware, traffic source and observer are **TBD**. A source-supported NIC capability is not evidence that this implementation meets G2.

## 2. Source baseline and actual implementation limits

| Source | Pinned revision and inspected material | Consequence |
|---|---|---|
| HNG contract | This branch's `d5df0b7`, [HNG-DESIGN.md](HNG-DESIGN.md), including trace/VLAN additions | Four independent `(line, direction)` C/Q states, long-delay storage and full-flow observation requirements |
| [DPDS host][host] | `5cd3b6b23307d39a7a0a4dc48c54520faa5cc17b`; README, `dpds.c`, common configuration, receive/transmit paths, Meson and Dockerfile | **Code:** one or two ports, 2–4 lcores, one RX/TX queue per port. One scheduler worker owns up to four line/direction states. Additional CPU cores do not automatically parallelize the engine |
| [DPDS-Core][core] | `9531a95e15b13801e117d2a0441f8d26d8d16ee4`; Cargo metadata, README and HNG engine/trace/buffer sources | **Code:** Rust scheduling is independent of DPDK and physical NICs. Two packet FIFOs and a service ledger reserve capacity through the ordinary allocator; mbuf hugepages are not the whole memory requirement |
| [Stage 1 target-check reference][stage1] | `de035a63f5d0135ee70b0bceb27d5051897b85a1`; target-check document/runner and `C2/harness/target_rate_metrics.py`, `target_rate_check.py`, fetched and read without execution | **Code:** capture before/after the emulator in one VM clock domain, pair by flow and sequence, compare with t plus the selected-late increment. This revision is awaiting a second review; pin it as a method reference, not an approved hardware result |

**Code:** the host records relative software RX time from `host_now_ns()` and TX-submit lateness. These are not per-packet wire timestamps. The complete independent observation/export/pairing path required in section 8 is still a software-stage deliverable.

**Estimate and M4 requirement:** bidirectional 100G at 1,542 line bytes is `2 × 8.106 Mpps`, leaving only **61.68 ns/packet** for the current single scheduler. A 60 ns median microbenchmark has almost no aggregate-rate margin, even before tails and host work. The eight-core reservation assumes a subsequent **one-scheduler-per-direction** layout, each owning that direction's cable and LEO states and ordered egress. Those states are independent; this split preserves each shared C/Q. It is not implemented by the inspected revision. M4 must measure end-to-end and tail costs and may revise the core count/layout; do not present the single-owner 60 ns target as proof of bidirectional 100G readiness.

**Code:** the small default configuration is not a 100G configuration. Pool sizing uses configured storage and the adjusted RX/TX descriptor counts, and startup reports object footprint. It does not reserve 32 GiB automatically. Defaults include 256 RX/TX descriptors, 1,024-entry handoff rings and 256 pending TX packets per line; none is a demonstrated 100G setting.

**Code:** retry defaults to 1 s, with 5 s limits on actual TX hold/shutdown. Long outages and drain intervals need software validation and, where necessary, configuration or implementation changes before hardware use. Resource timeout must not silently become trace loss.

## 3. NIC model, rate, count and PCIe

### 3.1 Acceptable candidates per DUT

| Candidate | Count and interfaces | Qualification condition | Evidence |
|---|---|---|---|
| ConnectX-6 Dx `MCX623106AN-CDAT` | One adapter, 2 × 100GbE, QSFP56, PCIe Gen4 x16 | Preferred candidate if available; negotiate actual Gen4 x16 on the selected node | [NVIDIA specification][cx6]; preference is an estimate |
| ConnectX-5 Ex EN `MCX516A-CDAT` | One adapter, 2 × 100GbE, QSFP28, PCIe Gen4 x16 | Viable alternative. Ordinary CX-5 `MCX516A-CCAT` is Gen3 x16. Verify the exact host/card combination; the manual's Gen4 capability is not certification of this machine | [NVIDIA specification][cx5]; platform combination TBD |
| ConnectX-4 `MCX455A-ECAT` | Two adapters, each 1 × 100GbE, QSFP28 and Gen3 x16 | Independent full-width paths without an undersized shared upstream link; prefer both on one node. `MCX455A-FCAT` is not this 100G model | [NVIDIA specification][cx4]; topology requirement is an estimate |
| Another DPDK-supported 100GbE NIC | Two ports in total, with adequate bus bandwidth | Qualify its PMD, VLAN retention, buffer behavior and observation method | **Code:** generic ethdev host interface; exact OPN and compatibility TBD |

**Code and estimate:** neither CX-4 nor CX-5 is mandatory. The engine has no ConnectX dependency. These mlx5 candidates have specific vendor and DPDK documentation that can be checked. CX-4 Lx must not be confused with the 100G CX-4 models. CX-6 Dx is an option, not a purchase mandate.

**Specification and TBD:** use Ethernet mode and two compatible 100GbE links. Record connector/module or DAC/AOC type, length, FEC, MTU and negotiated speed. A 100G link can carry 10G offered traffic; the first acceptance step does not require a 10G physical link mode.

### 3.2 Bus budget

**Estimate:** PCIe Gen3 x16 provides `8 GT/s × 16 × 128/130 ÷ 8 = 15.754 GB/s` in each PCIe direction after encoding, before transaction/descriptor overhead. Gen4 x16 provides `31.508 GB/s` on the same basis. [Encoding specification][pcie]

An inline emulator forwarding two simultaneous 100G network directions needs approximately **25 GB/s of PCIe writes and 25 GB/s of PCIe reads**. Payload traffic is slightly below line rate, but a shared Gen3 x16 bus is still insufficient. Its theoretical symmetric limit is approximately 63G per network direction before further overhead. Adding the two PCIe directions does not remove either bottleneck.

A dual-port Gen3 x16 card can support a single-direction 100G candidate test or bidirectional 10G, but must not be requested as a complete bidirectional 100G configuration. Two independent single-port Gen3 x16 cards distribute approximately 100G RX and 100G TX to each card. Check upstream topology, not just slot shape.

**Specification and estimate:** a Gen4 adapter in a Gen3 slot, or one negotiated at x8, loses the assumed bus margin. Record actual speed and width at handover.

**Design limit:** cable and LEO share the two physical ports through VLAN classification; they do not require four DUT ports. Each physical egress still has only 100G capacity. Cable 100G plus LEO 10G cannot both fit there. P1 tests delay accuracy and shared-Q stress with C below ingress capacity, such as C=80–90G; it does not claim sustained overload of C=100G from a single 100G ingress.

## 4. CPU, NUMA, memory and hugepages

| Item | Requirement or check | Evidence |
|---|---|---|
| Physical cores | Reserve eight data-plane cores plus control/observer cores; recommend at least 16 total. This assumes M4 later provides one scheduler per direction, with independent RX/TX work as needed. Current host layouts use only 2–4 lcores; M4 measurements may change the reservation. Do not count SMT threads as physical cores | Decision, estimate and code; aggregate-rate budget in section 2 |
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

**Estimate:** one DMA write and read per packet already approaches 50 GB/s of memory traffic for bidirectional 100G. One additional full packet copy adds roughly another 50 GB/s. Observation traffic must be budgeted too. Storage needs depend on the streaming-oracle versus full-event-retention choice in section 8; recording every payload is not required.

## 5. OS, kernel, DPDK and driver constraints

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

## 6. Number of machines and topology

**Test design:** the traffic path uses two machines: DUT and one dual-port traffic generator/receiver (TG). The primary timestamp scheme adds a third, passive observer machine. Two directional traffic streams run simultaneously.

```text
TG port A  ── 100G link ── DUT port 0
                              │
                     cable / LEO by VLAN
                              │
TG port B  ── 100G link ── DUT port 1

A TX → DUT 0 RX → engine → DUT 1 TX → B RX
B TX → DUT 1 RX → engine → DUT 0 TX → A RX
Management uses separate interfaces on both machines.
```

The primary scheme taps both links: the observer's first card receives the forward ingress/egress copies, and its second receives the reverse ingress/egress copies. Each card measures one OWD using only its own validated clock domain. Do not aggregate two 100G monitor streams into a single 100G observer port.

| Arrangement | What it can validate | What it cannot establish | Evidence |
|---|---|---|---|
| Software workstation, fake clock and net_null/net_ring/pcap/af_packet | Model, ABI, VLAN mapping, trace, order, drop attribution, ownership and drain | Physical NIC/PCIe/NUMA accuracy or 100G throughput; a virtual link-speed indication is not measurement | Decision and code; PMD sources in section 7 |
| One DUT with its two forwarding ports cabled together | NIC/link smoke or controlled loopback with a dedicated harness | Complete external offered/delivered traffic and OWD; direct forwarding loops may just recirculate packets | Estimate |
| One machine with four physical ports: two DUT, two TG | Physical link exercise with separate process/port/core/pool ownership | Independence from shared CPU, DRAM and PCIe contention; observer calibration is still required | Estimate; needs more hardware than a single dual-port adapter |
| **DUT plus dual-port TG/receiver** | Independent load, two links, bidirectional counts and external observation | A TG software send timestamp is not automatically its wire TX timestamp | Recommended test design; bypass capacity and timestamps must be qualified |
| DUT plus hardware tester or common-clock tap observer | OWD and throughput at declared, calibrated boundaries | Accuracy without evidence for the exact instrument, clock, timestamp point and target load | Specification TBD and test design |

**Estimate and TBD:** if only the DUT is supplied, an independent TG/tester outside the reserved testbed is still needed. Its NIC/PCIe must also sustain 200G aggregate TX and 200G aggregate RX; one dual-port Gen3 x16 adapter cannot be presumed adequate. Qualify TG bypass first so its ceiling is not attributed to the DUT.

## 7. Stage A: software gate before physical hardware

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

## 8. Stage B: accuracy and throughput acceptance

### 8.1 Same-clock method consistent with Stage 1

**Code/record:** Stage 1 pairs a packet observed at emulator ingress and egress within one clock domain. Retain that principle; do not subtract unsynchronized endpoint clocks or assume kernel tcpdump sees all DPDK traffic simply because a kernel netdev remains present.

**Test design:** retain `(run, direction, line, packet_id, ingress_ts, egress_ts, clock_id)` and compute **`OWD_i = egress_ts_i - ingress_ts_i`**. Declare whether each point is software observation, NIC completion, port-wire time or external RX tap. Same host or working NTP does not prove that two raw NIC clocks are directly comparable.

**Code compatibility:** the inspected Stage 1 probe carries a network-order 64-bit sequence, 32-bit magic and one-byte selected-late flag; its metrics pair `(flow, seq)` and reject duplicate/unmatched identities or changed flags. Preserve that logical identity/flag contract in M2 fixtures. P1 adds full ingress/oracle evidence for Q and trace behavior; it must not copy the reference runner's deployment addresses, interface names or fixed load into public defaults.

**Primary scheme — passive RX/RX observation (design choice):** prepare the third host, two dual-port CX-6 Dx `MCX623106AN-CDAT` cards and optical taps listed above. Pair both forward observation points on one card and both reverse points on the other. Validate the two ports' hardware clock relation and RX timestamp point; record the actual PHC/counter identity. A shared oscillator/PHC reduces relative clock uncertainty but does not remove unequal tap/receiver delays. Calibrate with bypass and swapped monitor paths before/after the run at both target loads, then bound offset, drift, timestamp granularity and load-dependent error. The target is **≤10 µs per OWD pair**, not a vendor guarantee. A missing bound or monitor loss prevents qualification. Two directions need not share a global clock for their separate OWDs; mapping to engine event time still needs its own calibrated uncertainty.

**Backup — hardware-timestamped TG (design choice):** use one dual-port CX-6 Dx `MCX623106AN-CDAT` on the TG, with the two DAC/AOC links and no tap host. Capture actual TX-port timestamps and opposite-port RX timestamps in a validated common clock for selected UDP packets. NVIDIA's documented Linux `tx_port_ts` path supports CX-6 Dx and later and eligible UDP/PTP packets; this is distinct from a DPDK launch-time request or CQE completion. The generator/collector integration and sampling coverage must qualify at load; low-rate dedicated probes alone cannot replace the required traffic sample. Calibrate the known cable/path offsets with bypass, retain raw results, and require the same ≤10 µs bound. **Bypass does not turn an unmeasured software send time into a per-packet TX-wire timestamp.** If the generator cannot export the required hardware timestamps, this backup cannot sign off physical accuracy. [Timestamp specification][timestamps]

| Timestamp method | Use and limits | Evidence |
|---|---|---|
| DUT software RX observation and successful TX-submit bracket, using one monotonic/TSC clock | Internal residence diagnostic corresponding to software observation boundaries. Validate epoch/frequency and cross-core consistency; retain before/after-submit uncertainty. Never substitute scheduled deadline for observed egress | Code and test design; complete export/pairer still required |
| External ingress and egress observations in one calibrated clock domain | Preferred physical OWD acceptance. For example, tap both boundaries into a common-clock RX observer; measure tap/path offsets, drift, loss and load-dependent tails | Test design; exact observer/tap/clock TBD |
| One dual-port TG's hardware TX/RX timestamps | Acceptable only with a validated common clock and explicit TX timestamp point; distinguish cable/TG boundary effects from internal DUT residence | Specification TBD and test design; older TX CQE timestamps are not automatically wire timestamps |

**Test design:** RX-read to TX-submit excludes some NIC RX backlog, TX queues and wire serialization. Passing that diagnostic alone cannot sign off physical OWD at 100G. Without calibrated external or hardware timestamp evidence, report internal diagnostics and throughput with accuracy **INCONCLUSIVE**.

**Specification:** NVIDIA distinguishes TX port timestamps from earlier CQE timestamps. Existence of nanosecond units, hardware timestamps or launch-time support does not mean this application implements them. [NVIDIA timestamp documentation][timestamps], [DPDK mlx5 guide][mlx5]

### 8.2 Initial physical qualification sequence

These are **test-design requirements for later execution**, not actions performed by this task.

1. Confirm Gate A, dedicated resources and access. Freeze software, firmware, topology, pool size and frame envelope. Record private environment values outside source/defaults.
2. Qualify generator/receiver bypass and zero-added-delay observation at the target loads. Require zero bypass drops. Measure offset, drift, timestamp brackets and observer/oracle loss; roadmap M5 requires clock uncertainty **≤10 µs**. Do not fit away emulator tails after the run.
3. Start at **10G offered rate**, each direction separately and then simultaneously, with cable and LEO VLANs. Primary cells are the registered Stage 2 operating points: modelled corridor A cable 65 ms and LEO about 74–80 ms; corridor B cable 100 ms (80/120 ms sensitivity) and LEO about 135–141 ms. Use the Stage 1 late-increment grid for fixed-delay cells. Also cover P0 boundaries: t=65/142 ms, p=0, p=0.01/0.10 with δ=5/320 ms, δ=0 and p=1. Trace cells use registered replay semantics, including UP/DOWN and loss/outage profiles already tested in software; do not combine trace with unsupported arbitrary late amounts.
4. First use no deliberate queue buildup, then C=8–9G shared-Q stress at 10G offered load. Hardware counters and the oracle establish validity of accuracy/throughput results; functional development is already complete.
5. Warm up longer than the maximum delay/hold/Q residence. Measure at least 10 minutes of steady state per cell with independent repeats; three repeats are the initial estimate. Retain fill, transition and drain windows separately. Register exact Q values, matrix, seeds and durations before running.
6. Only after all required 10G cells pass accuracy and validity, increase to **100G offered rate** with the same enabled observation, first registered MTU-1500 synthetic traffic, then separately bounded mixed-packet/multi-flow workloads. Repeat one-way and simultaneous two-way tests; C=80–90G is the stress example. No mandatory 40G stage.
7. Report `R_valid`, the maximum rate satisfying accuracy and loss/order validity together. Throughput with failed accuracy is not usable 100G qualification.

### 8.3 Thresholds and expected delay

**Decision:** evaluate each direction, line and registered required cell separately.

| Metric | Requirement |
|---|---|
| Median error | `median(abs(OWD_i - expected_i)) <= 0.1 ms` |
| Tail error | `p99(abs(OWD_i - expected_i)) <= 0.5 ms`; not p99 of signed error |
| Measurement validity | Clock uncertainty ≤10 µs and cannot change the verdict; unbounded missing samples/clock errors produce INCONCLUSIVE |
| Loss/order validity | Zero delay-storage drops and zero unintended reordering. Report intended Q/trace loss separately. Non-model NIC/pool/TX loss prevents full acceptance |
| Throughput | Per-direction offered/delivered L1, L2 and pps, plus L7 where applicable. Bidirectional 100G means 100G in each direction, not 100G aggregate |

**Test design:** without deliberate queueing, ordinary `expected=t`; selected-late `expected=t+δ`. Report both the overall distribution and ordinary/late groups. Define timestamp boundaries before testing and retain raw measurements. Additional egress serialization when late and ordinary packets collide **remains in G2 error**; also report that component separately, without moving the deadline.

For stress, an independent C/Q oracle supplies `expected_i=e_i-a_i+t+Iδ`, where `a_i` is ingress time and `e_i` is service completion. For trace, compute `deadline_i=max(e_i+t(e_i), deadline_{i-1})`, apply the registered outage action to obtain `release_i`, then use `expected_i=release_i-a_i`. Use the same declared observation boundaries and clock mapping throughout. Report monotone-clamp delay separately from implementation error; comparing directly to raw trace t would misclassify legitimate clamp behavior.

**Code compatibility:** Stage 1 revision `de035a6` already computes the median and p99 of absolute errors against `t + selected_late * delta`. Its quantiles linearly interpolate sorted finite samples at `(n-1)*q`; use the same convention in the P1 reporter. Do not substitute `abs(median(OWD)-t)`, which is a different statistic.

**Code and test design:** retain Stage 1's PASS/FAIL/INVALID distinction: invalid evidence takes precedence within a row; an overall valid failing row gives FAIL, otherwise missing/invalid required rows give INVALID. P1's **INCONCLUSIVE** measurement state maps to **INVALID** in that compatible report, with explicit reasons; it never becomes PASS. The referenced target check has no intended shared C/Q and records that layer as not applicable, not a fabricated zero. P1 must supply real Q/trace counters and independent oracle evidence instead. The reference uses low-rate probes under concurrent load; P1's target-load and full-flow requirements still apply. Reconcile any changes from the pending Stage 1 second review before freezing the tooling.

### 8.4 Observer cost and retained evidence

**Test design:** use complete ingress events and a full receiver sequence checker. The independent oracle needs all input events, including packets later dropped by Q or trace; it computes service/deadlines itself. Timing export may use registered identity-hash sampling, with late/application oversampling reported separately. Timing samples alone cannot establish zero full-flow loss/reorder. Include sample counts and confidence intervals in the report.

Retain arbitration/TX-attempt evidence for actual-time trace gating. Observer CPU, memory, rings and storage must be active during qualification. Capture-disabled throughput is a separate result.

**Estimate:** bidirectional 100G with 1,542-byte line accounting and illustrative 32-byte ingress events produces approximately **0.519 GB/s, or 311.3 GB per 10 minutes**, before egress/index data. Prefer a validated streaming reference with retained mismatches, summaries and timing samples. If retaining every event, request at least **1 TB of free working storage**, then calculate actual required space and sustained write bandwidth from the final schema. This is a data-volume estimate, not a disk performance claim.

## 9. Access, configuration and bring-up boundary

| Item | Required arrangement | Evidence |
|---|---|---|
| SSH | Ordinary account with SSH key; administrator supplies address, port and VPN/jump-host procedure. Repository/package access or offline packages | Estimate; exact values TBD and kept private |
| Management network | Separate management NIC/BMC and subnet; SSH does not traverse the emulated queue/outage path | Estimate; subnet assigned by the administrator |
| Data network | Direct point-to-point L2 is sufficient. TG supplies configured VLANs; IP flows use an isolated experiment subnet | Estimate; VLAN IDs and IP ranges are parameters, not embedded defaults |
| Root/sudo | Scoped administrator assistance for hugepages/mounts, memlock/device permissions, affinity/IRQ, drivers/firmware, timestamps and reboot. Ordinary account for build/analysis | Specification and estimate; direct root SSH/password is not required |
| Bare-metal mlx5 | Keep the mlx5/verbs path; arrange device/capability/memlock permissions for non-root execution | [mlx5 specification][mlx5-common]; do not apply generic devbind instructions blindly |
| IOMMU/VFIO | Bare-metal mlx5 does not require vfio-pci. Other VFIO PMDs use an IOMMU-backed setup. PF passthrough to a VM needs host IOMMU and safe groups; the guest uses the appropriate PMD driver | [DPDK specification][drivers]; no-IOMMU workarounds are not the requested baseline |
| VM fallback | PF passthrough, dedicated vCPU/RAM/vNUMA, host hugepages and no ballooning/overcommit; retain host and guest manifests | Estimate; a different qualification topology, not an assumed equivalent to bare metal |

**Decision, OS1–OS6:** preserve Apache-2.0 and upstream notices, add SPDX to new files, keep scheduling independent of DPDK and document public interfaces. Machine PCI addresses, CPU lists, hugepage locations, credentials and network values belong in operator-supplied configuration/parameters. Examples must use documented placeholders or synthetic values. No site-specific deployment script or live configuration is added by this request.

**Later bring-up checklist:** an authorized operator inventories the supplied topology and versions, reserves the node-local resources, confirms the required PMDs exist, checks link/FEC/MTU and counters, qualifies bypass and clock uncertainty, then freezes the manifest before section 8. Detailed commands must follow the selected driver and observer, with all machine values parameterized. This document provides the order and acceptance evidence; it does not claim that a runnable provisioning procedure exists for an unknown machine.

**Task boundary:** documentation only. No remote login, inventory, firmware/kernel/BIOS changes, hugepage allocation or hardware tests were performed. No new engine API, configuration schema or runtime dependency was introduced.

## 10. Information needed before handover

All entries below are **TBD**, not resources already provided:

- DUT inventory and management access, kept in the private deployment manifest; bare metal or registered VM fallback.
- Exact NIC OPN/PSID, port count/mode, firmware, negotiated PCIe width/speed and upstream NUMA topology; compatible links/modules.
- Physical core reservation, SMT policy, DIMM distribution and local RAM available for 32 GiB hugepages plus ordinary allocations.
- Frozen OS/kernel, DPDK 26.03, rdma-core/provider or compatible vendor stack, firmware and build toolchain.
- Independent TG/tester outside the reserved testbed, with bypass capacity at each target rate.
- Same-clock observation architecture, timestamp points, ≤10 µs uncertainty evidence and data retention plan.
- Completed software gate, final trace/frame/Q/storage bounds, benchmark/calculator evidence and registered physical matrix.

## Sources

Vendor/DPDK sources checked on 2026-10-29. Pinned code, project decisions and calculations are distinguished above. No third-party code or datasets are redistributed by this document.

[host]: https://github.com/markhhm/DPDS/tree/5cd3b6b23307d39a7a0a4dc48c54520faa5cc17b
[core]: https://github.com/markhhm/DPDS-Core/tree/9531a95e15b13801e117d2a0441f8d26d8d16ee4
[stage1]: https://example.invalid/stage1/tree/de035a63f5d0135ee70b0bceb27d5051897b85a1
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
