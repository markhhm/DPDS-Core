# HNG P1 scheduling engine

Status: implementation contract for `implementation worker`, including the 19:55 trace
addition; design and offline checks only.
Author: worker, 2026-10-29. Hardware execution requires Mark's approval.

## 1. Scope and evidence

P1 reuses DPDS's DPDK I/O and Rust/C boundary. It replaces the impairment path
with a finite FIFO bottleneck followed by a delay policy. A *line* is cable or
LEO, selected by VLAN. A *direction state* means one `(line, direction)` pair.
Every flow in that pair shares its C and Q. The reverse direction and the other
line have independent states: two lines bidirectionally require four states.
Cable uses fixed t and the seeded late subset. LEO uses monotone trace replay.
Both run in this engine, replacing kernel TheaterQ for Step 2.

The controlling research records snapshot is `78d949f6cf25c9d00a5a9f5c60515c2d1ee928cb`
(pulled again after the added user instruction):

- `research/agents/tasks/2026-10-29-design-reviewer.md` specifies this deliverable.
- `research/plans/active/step2-routing-demo-design.md`, decisions 15:30/15:40,
  19:10 and notes 19:20/19:35/19:55: shared C/Q, G2, fork DPDS, existing
  cross-NUMA NICs, 32 GiB hugepage reservation, **10G then 100G**, no 40G gate,
  VLAN-separated cable/LEO, trace delay/loss/outages. Trace splicing follows
  05:15 and its 10:00 correction, detailed in section 5a below.
- `research/reviews/100g-emulator-p0-2026-10-29/report.md`, read in full:
  E08–E18 are code evidence; sections III/IV/VI and P1-0 supply the model,
  resource estimates and measurement requirements. Its earlier 40G proposal
  and pending-P1 wording are superseded by the decisions above.

The source baselines inspected are DPDS host
`9d19ad663ada2c0e8bcc85f3f3fb6d97d93cbe38` and DPDS-Core
`91766e281beb7b94f008ab8909752f5c3ed4574b`. Source paths below refer to those SHAs.
These are evidence for current behavior, not a claim that HNG is implemented.

P1 measures 100G delay fidelity and shared-queue congestion with C below ingress
capacity (e.g. 80–90 Gbit/s). A single 100G ingress cannot sustain overload of
C = 100G. Do not call P1 “full 100G stress”. The 1G research line has priority.
No live unregistered parameter changes, per-flow scheduling, arbitrary
per-packet trace late amounts, hardware deployment or hardware purchase belongs
to this change. Time-indexed trace delay/loss/outages are in scope. Section 5a
extends the fixed-delay contract in sections 3–5; it is not an optional follow-up.

## 2. Required changes and integration surface

| Baseline behavior | Required HNG behavior | Files/callers to update during implementation |
|---|---|---|
| One worker Buffer receives both ports (E12–E14) | Explicit ingress-port-to-direction mapping before changing the egress port; one state per direction | host `core/receive.*`, `core/worker.*`, `core/combined.c`, `dpds.c`, `common/common.h` |
| Delay precedes limiter (E09–E10) | Byte admission and C service precede t and δ | core `src/buffer.rs`, `src/limiter.rs` or a dedicated HNG module exported by `src/lib.rs` |
| Spacer/TBF and packet-count storage are not the requested finite byte Q | One drop-tail Q including the packet in service, released at service completion; storage capacity separately checked | core configuration, admission, service ledger and counters |
| Heap/FIFO comparator uses time only (E08/E11) | Two monotone FIFO subsequences; merge by `(deadline_ns, ingress_seq)` | core scheduler, head/pop exports and tests |
| Random distribution/dropper is not fixed t plus seeded Iδ | Explicit reproducible per-packet selector; no legacy random loss in HNG mode | core configuration and selector; `src/delayer.rs`/`src/dropper.rs` remain legacy-only |
| Batch shares a floating-point timestamp (E13) | Per-packet ingress sequence and declared arrival clock; integer relative time; batch partition must not alter replay | host receive/metadata/handoff; core FFI |
| Shared pool on `rte_socket_id()`, ~2.097M objects for two ports (E15–E16) | Sized pools per ingress NUMA node/direction, explicit total memory and reserves | host `dpds.c`, `common/config.h`, `common/common.c` |
| `buffer_push` returns one bool | Distinct Q rejection, storage failure and invalid input; explicit pointer ownership | core FFI, generated header, all host push callers and stats |
| Short TX/ring enqueue frees unsent packets | Retain unsent suffix in order, bounded retry/backlog; account terminal failures separately | host `core/transmit.*`, `core/worker.c`, `core/combined.c` |
| Shutdown depends on flags; TX loop can exit with ring entries left | Stop RX, drain all handoffs and scheduler, flush pending TX, reclaim mbufs, reconcile counts | all three host layouts and cleanup paths |
| Upstream core submodule URL and legacy JSON | Point host submodule to `markhhm/DPDS-Core`, pin reviewed core SHA; explicit HNG schema and examples | host `.gitmodules`, submodule gitlink, `config.json`, `meson.build`, READMEs; core `build.rs`, `cbindgen.toml`, examples/tests as needed |
| Two ports have no substrate classifier | VLAN lookup selects one of four line/direction states; ordered merge onto each physical egress | host receive classifier, port mapping, worker and TX metadata; VLAN fixtures |
| No trace policy | Service-time trace lookup, monotone deadline clamp, trace loss/outage policy and provenance | core delay policy/configuration, normalized trace parser, independent oracle and step-up/step-down tests |

Preserve the legacy interface as a separate mode if retained. Never reinterpret
`limit_bit`, `buffer_capacity`, `reordering` or a legacy JSON as HNG parameters.
HNG construction must reject incomplete/mixed-mode configurations before RX
starts. Generate the C header with the existing cbindgen build; do not maintain
a conflicting hand-written ABI. Preserve Apache-2.0 LICENSE, upstream README
credit and Cargo author metadata. Retain any upstream NOTICE if one is added;
neither inspected baseline contains a NOTICE file.

### Selected approach

The approved two-FIFO design needs O(1) head selection and amortized O(1)
admission bookkeeping per packet. A stable min-heap is a useful independent
test oracle but costs O(log N) per operation at millions of stored packets.
Per-worker C/Q instances would change competition between flows and are invalid.
The builder may choose the bounded ring representation and optimize arithmetic
after proving equivalence. The contract below does not require a new framework.

## 3. Packet, byte and time definitions

For each physical ingress direction, assign a monotonically increasing `u64 ingress_seq` to
**every packet returned by RX**, before Q admission. A dropped packet consumes
a sequence number. Classify VLAN before model admission and assign a separate
`selection_seq` in the chosen line/direction state; every classified packet,
including Q drops, consumes its draw. This keeps a line's random selection
independent of traffic on the other line. Preserve original ingress port and
VLAN in metadata. A flow's sequence is its subsequence of ingress order.

Use one RX queue and one TX queue per physical direction for the correctness baseline.
RX descriptor order defines ingress order, including equal timestamps. Do not
merge independent RSS queues by polling order and claim wire-arrival FIFO.
Future parallel RX requires an explicit, verified ordering/watermark mechanism.
One owner may service the four states with bounded alternating work; the 100G
candidate can use one owner per physical direction, owning both line states.
No flow-specific limiter exists. Port output ordering is specified in section 5a.

| Quantity | Definition |
|---|---|
| `a_i` | Integer ns since run epoch at the declared RX observation point; nondecreasing in ingress order |
| `q_i` | L2 frame bytes including padding and FCS: `max(pkt_len + 4, 64)` under the baseline offload policy |
| `w_i` | Line-accounted bytes: `q_i + 20` (8-byte preamble/SFD and 12-byte IFG) |
| C | Integer bits/s, charged using `8*w_i`; not payload goodput |
| Q | Integer L2 bytes, charged using `q_i`, including the packet currently being served |
| `s_i` | Virtual completion time of bottleneck service for an admitted packet |
| `I_i` | 0 for ordinary; 1 for selected late; independent of Q outcome |
| `d_i` | Ideal release deadline: `s_i + t + I_i*delta` |

Baseline policy: RX strips FCS, retains VLAN bytes in `pkt_len`, and disables
VLAN strip/insert, TSO/GSO, LRO/GRO and packet coalescing. Declare offloads in the
manifest and reject incompatible settings. The first hardware profile uses
single-segment MTU-1500 packets; unsupported chains/jumbo packets are an explicit
input error, not silently miscounted. If chains are later supported, count all
segments for pool demand and use total packet length for C/Q. For an untagged
1500-byte IP packet, `pkt_len=1514`, `q_i=1518`, `w_i=1538`. FCS is not “FEC”.

Store payload bytes, q bytes, wire-accounted bytes, packet counts and mbuf
segments as separate metrics. The host must verify these length assumptions
against its PMD; never apply `+4` twice. Q is not a packet-count limit or the
amount of RAM reserved for delay. A frame with `q_i > Q` is always Q-dropped.

Use a monotonic local clock for software runs, expressed relative to one epoch.
Hardware runs need a documented conversion of RX timestamps into that domain;
store raw timestamps and mapping versions. A software timestamp must be taken
at RX, not later in the worker after handoff. Software batching error remains
observable; software RX time alone cannot establish wire-time G2. Reject a
backward timestamp or sequence wrap as an invalid run; do not clamp and hide it.

### Service arithmetic without cumulative rounding drift

Use exact rational virtual service time, or an equivalent fixed-point/carry
implementation. One reference representation uses `u128` units of `ns*C`:

```text
A_i = a_i * C
S_i = max(A_i, S_previous_admitted) + 8*w_i*1_000_000_000
s_ns_i = ceil(S_i / C)
deadline_ns_i = s_ns_i + t_ns + I_i*delta_ns
```

Do not round each packet's service interval upward and accumulate it: 100G
packets have fractional-ns service intervals and that method changes C.
Round only the absolute service completion for deadline comparison. The error
is less than 1 ns; integer t and δ retain their exact differences. Release Q
using `S_i <= a*C`, not an inconsistent second rounding rule. Use checked
arithmetic and reject overflowing time/configuration values. No float cast or
silent saturation may change a deadline.

## 4. State, admission and dequeue

Each direction contains:

- Immutable validated `(C, Q, t_ns, delta_ns, seed, selection threshold)`.
- Next ingress sequence, previous arrival time, virtual service tail S.
- Line-local selection sequence and an input frontier W: a proven lower bound
  on every not-yet-admitted arrival timestamp, including RX/handoff backlog.
- Q occupancy in L2 bytes and a FIFO **service ledger** of `(S_i, q_i)`.
- Ordinary and late bounded FIFO storage of
  `(packet_ptr, ingress_seq, deadline_ns, q_i, wire_bytes)`; retain or export the
  complete ingress events required by the independent oracle in section 9.
- One combined storage packet budget, counters and high-water marks.
- Host-side per-direction pending TX suffix and handoff queues, if used.

The service ledger contains accounting records, not another packet copy. A
packet can have future service completion and already occupy its final ordinary
or late FIFO; its deadline prevents early release. This avoids a third payload
queue. The ordinary and late queues together hold one pointer per admitted
packet. Reserve their storage before admitting traffic, with a recorded total
allocation. If each queue reserves the full bound, budget both allocations;
do not allocate p*N late slots and assume the random tail cannot exceed them.

### Admission, in ingress order

```text
1. Assign seq; compute I from (line/direction seed, selection_seq), even if rejected.
2. Validate arrival/length/sequence; release ledger heads with S <= a*C.
   Subtract their q bytes from q_used. Record the admission-time watermark a.
3. If q_i > Q - q_used: count Q_DROP, return pointer ownership to host.
   Do not update S_tail or storage. Later accepted packets get no phantom work.
4. Reserve one storage slot and one service-ledger slot. If unavailable:
   count the specific STORAGE_DROP, invalidate run, return ownership to host.
   Do not commit Q bytes or service time. Stop further admission safely.
5. Compute S_i, deadline_ns_i with checked arithmetic. Commit q_used += q_i,
   S_tail = S_i and ledger entry. Append packet to ordinary or late by I.
6. Return ACCEPTED; the host cannot free or resubmit that pointer until popped.
```

At equal arrival/service completion, release completed bytes **before** admission.
Q=0 admits no packets, including on an idle link. Permit it for a deliberate
all-drop test; require Q large enough for the registered frame sizes in G2 runs.
A Q-dropped packet cannot change the late selection of subsequent packets.

The ledger must not be destructively aged to wall-clock `now` while older
arrival timestamps can still be in RX/handoff queues. Otherwise later processing
of an old packet would see a falsely empty Q. Age it on ordered admissions;
reclaim at drain only after the input watermark guarantees no older arrival can
appear. Expose the ledger watermark with Q snapshots. A live current-time Q
estimate may scan/filter a snapshot but must not mutate admission history.

### Dequeue rule

```text
eligible_head = min(ordinary.front, late.front,
                    key=(deadline_ns, ingress_seq))  # ignore empty queues
if no head or eligible_head.deadline_ns > now_ns: return EMPTY_OR_NOT_DUE
if input is not complete and eligible_head.deadline_ns > W + t_ns:
    return WAIT_FOR_INPUT_FRONTIER
otherwise pop that head and transfer its pointer to the host pending-TX path
```

Select the minimum again for every packet, including within a burst. Do not
drain the ordinary queue first and then late. `now == deadline` is due. The
dequeue clock never changes s or d; a stalled owner releases overdue packets in
their original deadline order and reports lateness. No allocation is needed in
the fast path after startup. Each ledger entry is appended/removed once, so
ledger work is amortized O(1); explicitly measure long catch-up iterations.

**The input frontier is required even when Q accounting is correct.** A due
late head can precede an ordinary packet still in an RX handoff with an earlier
deadline. Under monotone arrivals, the last admitted arrival is a conservative
W; unseen packets can have that same arrival, but later ingress sequences. A
tighter proven lower bound on unseen deadlines is an equivalent optimization.

To advance W while idle, RX must establish a lower bound on future arrival
timestamps after draining its queue, then publish a frontier marker **after**
all preceding records. The scheduler consumes those records before the marker.
A CPU clock read while timestamped NIC backlog may remain proves nothing about
W. Hardware clock conversion must include uncertainty in the bound. If the PMD
cannot establish a bound, validate a conservative acquisition protocol or mark
that timestamp mode unsupported. A single-thread software-RX baseline can
establish W after an empty poll in its declared arrival clock. End-of-input
after RX/handoffs drain removes this restriction. Stale W can cause timing
failure; it cannot excuse wrong order. Trace mode uses its minimum trace delay
instead of fixed t in this bound, as specified in section 5a.

### Order and δ proof sketch

Assume immutable parameters, positive packet lengths/C, nondecreasing arrivals,
one input order and no pointer loss/duplication. Among admitted packets S is
strictly increasing. `ceil(S/C)` is nondecreasing; integer quantization can tie.
Each ordinary deadline adds the same t. Each late deadline adds the same t+δ.
Thus both queues are monotone in `(deadline_ns, ingress_seq)`. Inductively, the
smallest admitted key is always one of their heads. Every unseen deadline is
at least `a+t >= W+t`. A head with `deadline <= W+t` cannot be overtaken by an
unseen key: equal deadlines have later ingress sequences. With this restriction,
the merge emits the stable global sort, including packets admitted in later polls.

For same-flow packets i before j, both ordinary or both late implies i before j
at output. Ordinary i then late j also cannot invert. The only possible inversion
is **earlier selected-late i overtaken by later ordinary j** with
`s_ns_j < s_ns_i + delta_ns`. Equality preserves i first by ingress sequence.
Remove the selected-late subset and each flow retains its ingress order. This
does not mean that an ordinary packet cannot overtake a selected packet, or
that every selected packet will actually arrive out of order.

Holding arrivals, lengths, C and Q fixed, admission and S are independent of I.
For each surviving selected packet, its deadline minus its counterfactual
ordinary deadline is **exactly `delta_ns`**. Its propagation increment over
rounded service completion is exactly `t_ns + delta_ns`. Drops remove packets;
they do not justify unexpected permutations among survivors.

These are scheduler proofs. The host must preserve the merged order through
one ordered TX queue per direction. A physical link serializes packets; late
and ordinary releases can collide. Therefore actual departure is not promised
to have an exact extra δ. TX queueing, polling and wire timing are measured as
realization error. Do not shift deadlines forward and then claim they were met.
At C=physical line rate, even a correct merger can have transient output backlog.

## 5. Reproducible selection and configuration

Use a counter-derived, versioned selection function so burst size, polling,
flow count and Q drops do not advance hidden RNG state differently. The following
`splitmix64-counter-v1` is a proposed concrete noncryptographic selector (a design
choice, not a claim of truly independent randomness):

```text
# All operations wrap modulo 2^64; selection_seq starts at zero.
z = seed_line_direction + (selection_seq + 1) * 0x9e3779b97f4a7c15
z = (z xor (z >> 30)) * 0xbf58476d1ce4e5b9
z = (z xor (z >> 27)) * 0x94d049bb133111eb
u = z xor (z >> 31)
I = (u < threshold), with p=0 -> false and p=1 -> true
threshold = floor(p * 2^64)  # parse p as an exact decimal, once at startup
```

Record independent explicit line/direction seeds; never use wall time or a global
thread RNG. `p` denotes Bernoulli-style packet selection, not observed OOO rate.
For 0<p<1 the implemented marginal is `threshold/2^64`; record that threshold
and the requested p. Validate endpoint cases without overflowing u64. Seed 0,
selection_seq 0 must give `u=0xe220a8397b1dcdaf`. A separately versioned/pinned generator
with published vectors is acceptable only as an explicit contract amendment,
with updated tests and manifest. Timing-sampling uses a separate seed/domain.

Fixed-policy parameter example (illustrative values, not a complete host run;
section 5a adds required VLAN/line and trace configuration):

```json
{
  "schema": "hng-v1",
  "directions": [
    {"ingress_port": 0, "egress_port": 1, "t_ns": 142000000,
     "delta_ns": 320000000, "p": "0.10", "seed": 2026102901,
     "C_bps": 90000000000, "Q_bytes": 562500000},
    {"ingress_port": 1, "egress_port": 0, "t_ns": 65000000,
     "delta_ns": 5000000, "p": "0.01", "seed": 2026102902,
     "C_bps": 90000000000, "Q_bytes": 562500000}
  ]
}
```

The illustrative Q corresponds only approximately to 50 ms at 90G because Q
uses L2 bytes and C uses line bytes. Use the registered frame mix to calculate
the exact residence bound. Storage, PMD, clock and observation resources are
host configuration, separate from the mathematical C/Q/t/δ model; they are
required resolved manifest fields, not guessed from this example.

Reject C=0, negative/nonfinite/out-of-range parameters, invalid port mappings,
duplicate `(ingress_port, VLAN)` mappings, unknown schema/fields, insufficient allocation,
unsupported offloads and unsafe integer ranges. Fixed t/δ can include zero for
tests. C/Q/t/δ/p/seeds stay constant until input stops and all state drains.
Changing them starts a new run/epoch. A registered trace is immutable while its
time-indexed values advance. C=0 is not a “down” mode; trace outages are explicit.

The resolved run manifest must include:

| Group | Required fields |
|---|---|
| Identity | run ID, UTC start/end, config SHA-256, schema/model/selector versions, per-line/direction seeds, p/threshold, ingress and selection sequence initial values, input fixture/profile hash |
| Model | VLAN/port/direction/substrate mapping, t/δ or trace policy, C bit/s, Q L2 bytes, inclusion of in-service bytes, length/offload policy, rounding rule, input frontier and queue admission watermark definitions |
| Code | host/core commit SHAs, submodule SHA, dirty state, Cargo.lock hash, compiler/build flags, Rust/C toolchain versions |
| I/O | actual DPDK version/build/PMD, rdma-core or OFED/DOCA version, kernel, NIC PCI IDs/firmware, negotiated link and PCIe width/speed, RX/TX descriptors and queue mapping |
| Resources | core/vCPU/NUMA placement, per-node hugepage reservation AND successful allocation, pool object/segment counts and measured stride, FIFO/ledger/ring/cache reserves, VM RAM and host contention |
| Time/observation | clock IDs, raw-to-common-clock mappings and refresh interval, tick resolution, timestamp mode/CQE compression, sampling algorithm/seed/rate, full-sequence checker version, observer uncertainty and loss |
| Workload/result | frame mix, per-direction offered and delivered L1/L2/L7/pps, warm-up/window/repetitions, all counters/high-water marks, drain outcome, per-gate PASS/FAIL/INCONCLUSIVE |

The host README requests DPDK 26.03, while P0 cites other versions for specific
capabilities. Do not infer CX-4 compatibility from that request or transplant
version-specific timestamp claims. Software-PMD CI records firmware as
`not-applicable`; a hardware manifest must contain observed values, not null
placeholders. Same seed reproduces selection for the same observed line-local
selection sequence; it does not reproduce live packet timing or RX losses.

## 5a. VLAN lines and trace replay (19:55 addition)

### Four model states, two physical ports

Tofino places a configured VLAN ID on each packet according to its substrate
decision. Validate a complete table `(ingress_port, vlan_id) -> (line_id,
egress_port, direction_state)`. For example, VID 100 may select cable and VID
200 LEO on both ingress ports; these are illustrative IDs, not reserved testbed
values. Use one 802.1Q tag for the first profile. Reject untagged, unknown VID,
malformed and stacked-tag packets with a classifier counter unless a future
profile explicitly supports them. PCP/DEI must not accidentally become line IDs.
Retain the tag unchanged; Tofino owns tag insertion/removal. Account the VLAN's
four bytes in L2/wire lengths and verify the configured MTU accepts it.

All flows assigned to cable A→B share cable A→B's C/Q, and likewise for the
other three states. Trace transitions, queue pressure and random selection on
LEO must not change cable's model state. There is no limiter per VLAN flow.
VLANs identify configured lines, not dynamically created independent limiters.

Both lines feed one ordered physical TX queue per direction. Compare their
eligible heads by `(release_not_before_ns, ingress_seq)`, where ingress_seq is
the common physical-ingress ordinal. For fixed mode, release_not_before is d;
trace outage holding is defined below. Before emitting a candidate, its key
must also be safe against unseen input for **both** line states. A conservative
bound is `W_line + minimum_delay_line` for each; a trace's validated global
minimum t is sufficient. An unavailable frontier on one line cannot silently
be treated as infinity. A physical RX frontier marker advances both lines after
all earlier classified records have been processed. End-of-input closes both.

This is a constant-size merge of at most four fixed-mode FIFO heads per port,
or a fixed-mode pair plus a trace FIFO. It adds no second experimental C/Q.
Physical egress still serializes aggregate traffic and can become a bottleneck:
cable 100G plus LEO 10G cannot both exit one 100G port at their nominal maxima.
Register aggregate offered/admitted load and require measured port backpressure
within G2 limits. Port queueing/drops are infrastructure effects, not either
line's intentional Q loss. Preserve line counters despite shared pool/port
resources. Same-flow ordering guarantees apply while a flow stays on a line;
cross-line migration can legitimately reorder through different path delays
and must be evaluated by the routing/trace oracle, not concealed by flow pinning.

### Trace lookup and monotone deadline

Define `e_i = s_ns_i`, the rounded virtual **completion of C/Q service**, not
RX arrival, worker wall time or TX time. This fixes the meaning of e in the
requested formula. Read trace t at that e relative to a common registered run
epoch. A normalized trace contains ordered half-open intervals `[start,end)`
with a nonnegative integer `delay_ns`, loss descriptor and link/outage events.
At a boundary, the new interval applies. The first version uses sample-and-hold
values; no linear interpolation, implicit wrap, invented gap filling or silent
RTT-to-OWD conversion. Missing/invalid intervals fail profile validation.

For packets surviving intentional trace loss/outage drop, in service order:

```text
raw_deadline_i = e_i + t(e_i)
deadline_i = max(raw_deadline_i, deadline_previous_retained)
clamp_extra_i = deadline_i - raw_deadline_i
```

Initialize the previous deadline as absent, so the first packet uses its raw
deadline. Keep it across empty queues, delay steps and splice boundaries until
the run drains and resets. Do not reset it just because no packet is currently
stored. All equal deadlines use ingress sequence. The resulting FIFO is
nondecreasing by induction even when t falls. It therefore preserves ingress
order among survivors; a downward delay step produces a plateau and drain,
not overtaking. Report the clamp count, total/max extra delay and plateau size:
the realized configured delay can exceed the instantaneous t from the trace.

Trace mode uses one FIFO (the ordinary FIFO can be reused) and requires
`p=0, delta_ns=0`. The fixed two-point mode and its exact δ proof remain available
for cable and calibration. Combining trace with a synthetic late subset is
not implicitly enabled. Trace FIFO monotonicity does not establish that every
fine-grained reordering feature of the source was reproduced.

The input-frontier proof still holds: raw deadline is at least `a + min_trace_t`,
and both monotone clamping and outage holding only increase it. The port merger
must therefore wait when an unseen packet could precede its candidate.

### Trace loss and outage semantics

Normalize the source before the hot path. Preserve trace/source ID, direction,
units, original sample intervals, missingness, conversion recipe and hashes.
A probe loss fraction is not a known loss mask for unrelated live packets.
If replay uses per-interval loss probability, label that reconstruction and
sample it with a separate versioned `loss_seed`/domain and line selection_seq.
If an exact replay fixture contains matching packet identities, its explicit
loss mask can be used instead. Never infer an outage solely from a missing
delay sample. Unknown source semantics remain an invalid profile until mapped.

Evaluate loss at e, **after** finite-Q admission/service. A trace-dropped packet
consumes C service and Q bytes until its service completion; it does not become
a Q drop or refund service to later packets. It needs a service-ledger entry
but no propagation-storage slot once its drop is known. Compute the action and
reserve needed ledger/storage resources transactionally before committing S/Q.
The host may free the payload immediately once that deterministic future drop
is established, while the ledger retains its future byte-release event.

Outage behavior must be declared in the normalized profile, not guessed by the
engine. Use intervals in the same run-time domain and distinguish:

| Profile action | Required behavior |
|---|---|
| `drop` | A candidate monotone deadline in a down interval is an intentional `trace_outage_drop`; retain C/Q service accounting, do not append a packet or advance the retained-deadline floor |
| `hold` / stop sending | Map the monotone deadline to the first up instant at or after it: `release_not_before=next_up(deadline)`. Retain packet storage; no TX admission while this line is down |

The previous-retained floor uses the computed monotone deadline, not the hold
time. `next_up` is nondecreasing, so hold-mode release order remains FIFO.
Apply loss before the outage action; use disjoint `trace_loss_drop` and
`trace_outage_drop` reasons. For drop-mode survivors, advance the retained floor
only after accepting storage custody. Thus a discarded packet does not create
a phantom delay plateau for later survivors. Both actions are explicit trace
effects, separate from Q loss and resource loss.

Also gate TX admission using the line's state at actual `now`: a packet made
late by CPU/TX stalls cannot be sent while the profile says down. In hold mode
keep it until up and record realization lateness against the original deadline.
In drop mode, a modeled survivor discarded only because actual now is down is
a non-model `missed_deadline_outage_drop` and makes the run FAIL. The independent
oracle still expects that survivor: never add it to intentional trace losses
or rewrite its deadline. Do not roll back the already-committed deadline floor
after a terminal TX failure; later scheduled packets retain their deadlines.
A paused LEO head must not prevent
cable from transmitting safe due packets. Re-evaluate per-line gates on every
retry. Packets already handed to the NIC cannot be recalled; observe boundary
leakage/serialization and timestamp uncertainty instead of promising exact
wire silence from a software gate. Hardware acceptance requires the registered
outage realization/observer checks as well as the ordinary timing tests.

Consequently the fixed global-sort proof applies within each line; physical
cross-line output also depends on the actual-time outage gate. The independent
port oracle retains original modeled deadlines and checks per-line order, then
checks arbitration against eligibility at each recorded TX attempt/retry time.
For example, with LEO d=190, cable d=220, outage=[200,300), and a scheduler
resuming at now=250, cable can be sent before the held LEO packet. That gated
cross-line bypass is permitted; LEO's added wait is still realization error
against d=190, not a newly invented modeled deadline. Without a gate exception,
ready packets are compared by the registered release key. Record every port
arbiter/TX attempt needed by the checker (time, line/seq IDs, accepted prefix
and gate state); the reference derives gate state independently from the trace.
Missing arbiter evidence makes cross-line order verification INCONCLUSIVE.

Outage hold storage must cover its registered maximum duration and admitted
traffic; an indefinite down interval cannot be a bounded zero-loss hold test.
Require a finite end/recovery or an explicit drain/abort policy. If a trace ends
before all admitted e/deadlines can be evaluated, apply its explicitly declared
terminal `hold_last_sample` interval or stop admission early and drain within coverage. Never
wrap to the first sample. Exceeding coverage without a policy invalidates the run.

`hold_last_sample` is trace-end extension, distinct from outage action `hold`:
it extends the last delay, loss and link-state values to a declared finite
`terminal_horizon_ns`. It does not turn an up link into a stopped link. Record
the original end, extension duration and inherited values. Stop ingress early
enough for service, delay, outage recovery and drain to fit within this horizon.
A last down/hold sample with no recovery cannot complete a lossless drain;
reject that profile for G2, or register an explicitly invalidating abort.
The single-natural-segment HOLD rule below uses this trace-end policy, not an
indefinite outage. No new source segment is introduced by the extension.

### Configuration seam and deferred packet-specific late amounts

Extend the host schema with `vlan_lines`, each containing a VLAN/port mapping,
two direction parameter sets, C/Q and a delay-policy tag. The fixed policy uses
section 5. The trace policy requires `trace_path`, `trace_sha256`, `trace_epoch`,
`time_scale=1`, delay/loss/outage basis and action, loss seed or exact mask mode,
terminal policy, min/max delay and max outage hold bounds. Hash the normalized
trace and its source/conversion metadata in the run manifest. Load and validate
all intervals before RX; no filesystem I/O in the packet loop. Use an advancing
interval cursor because e is monotone, including across Q drops.

Keep the internal delay-policy seam at the service-completion boundary:
`(line, seq, e, trace cursor) -> intentional_drop | release_record`.
Expose a capability such as `ordering = monotone_fifo | arbitrary_deadline` in
the design; only fixed-two-point and monotone-trace implementations are active.
Requests for packet-specific late amounts return `UNSUPPORTED_POLICY` before
RX starts. A future Choi-style packet-late policy requires a stable heap/timing
wheel, a new order oracle and revised memory/performance validation. Do not
implement a dormant heap now, clamp away measured reordering and call it exact
replay, or route arbitrary deadlines into the two monotone FIFOs.

### Splicing and new required tests

Follow the 05:15 decision and 10:00 correction without claiming that this new
clamp validates splicing:

- #6 and experiments of at most 45 s use one natural segment with HOLD; no splice.
- #7's 270 s primary comparison replays identical segment order, offsets, seeds,
  loss/outage decisions and clamp rules for schedule-driven and INT-driven
  routing. Report the paired difference; absolute goodput and the union-flag
  exclusion analysis remain secondary. Retain failed splice-validation evidence.
- Keep the unspliced robustness comparison: independent natural 45 s runs for
  each phase, with boundaries aligned to phase transitions. Shared distortion
  cancels only insofar as both controllers respond equally; an INT response to
  an artificial boundary can still bias the paired difference. Report this
  limitation and source-run/repeat counts, not only numbers of gate judgments.

Additional unit/integration fixtures, using fake e values in ns:

| Test | Expected result |
|---|---|
| Delay step UP | e=(10,20,30), t=(100,200,200): raw and clamped deadlines=(110,220,230), order=(0,1,2) |
| Delay step DOWN | e=(10,20,30), t=(200,100,100): raw=(210,120,130), clamped=(210,210,210); stable order=(0,1,2), zero overtakes |
| Boundary/idle | Exact interval start uses new value; delay floor survives an empty FIFO and splice; invalid gaps/end coverage rejected |
| Loss/drop | Seeded loss p=0/1 and exact mask; a lost packet consumes service, releases Q at e, has its own reason, and never enters propagation FIFO |
| Outage hold | Down=[200,300), base deadlines=(210,220,310) -> releases=(300,300,310); no admission to TX in down interval; cable continues |
| Outage drop | Same interval/deadlines drops first two with outage counters; no Q refund; retry crossing outage rechecks state; dropping an expected survivor is `missed_deadline_outage_drop` and FAIL |
| VLAN isolation | Interleaved cable/LEO in both physical directions produces four independent C/Q timelines and RNG sequences; reject unknown/stacked/untagged input |
| Port merge | Compare both line heads stably, honor both input frontiers, retain partial TX suffix and line identity; report shared-port overload separately |
| Deferred policy | Per-packet arbitrary late amounts rejected at configuration time; trace p/δ combination rejected |

The hardware trace oracle uses clamped deadlines and explicit outage actions,
not raw t alone. Compare clamp distortion separately from implementation error.
Validate both upward/downward steps and loss/outage boundaries at 10G before
100G, with simultaneous cable/LEO load. Trace min/max delay, VLAN frame overhead
and outage hold extend the resource budget below; a 142+320 ms fixed-mode bound
must not be reused for a longer OneWeb trace without recalculation.

## 6. Storage and NUMA budget

For one fixed-policy line bidirectionally, P0's 1500-byte IP, untagged assumption gives
`pps = 100e9 / (8*1538) = 8.127438 million/s/direction`. The initial 32 GiB
hugepage request is a budget, not an observed allocation or proof of zero loss.

| P0 sizing case, both directions | Estimate |
|---|---:|
| t=142 ms, p=0, payload-rate approximation 100G/direction | 3.55 GB raw stored data |
| t=142 ms, δ=320 ms, every packet late | 11.55 GB raw data |
| Above plus illustrative 50 ms Q residence | 12.8 GB raw data |
| 8.127438 Mpps, 512 ms, illustrative 2304-byte object | 8.322497M objects, 19.175033 GB (17.8581 GiB) |

The last row excludes ledger/FIFO metadata, pool alignment/headers beyond the
illustrative object, RX/TX descriptors, handoff rings, caches, retry storage,
logging and the OS. Measure actual allocation stride at startup; `2304` is
not a measured `sizeof` for the deployed build. The upstream two-port pool's
~2.097M objects already falls short of ~2.308M packets at t=142 ms and p=0.

For each line/direction, construct a conservative packet budget from the registered
traffic envelope, not average `t+p*delta`. One safe decomposition is:

```text
N_Q <= floor(Q_bytes / minimum_q_bytes)
N_delay <= burst_bound_packets
           + ceil(C_bps * (t_ns + delta_ns) / (8*minimum_wire_bytes*1e9))
N_pool >= N_Q + N_delay + RX_descriptors + TX_descriptors
          + handoff_slots + pending_TX_bound + inaccessible_cache_reserve
```

For trace mode replace `t_ns+delta_ns` by `max_trace_delay_ns+max_outage_hold_ns`.
Sum all four states' Q/storage demands. Add shared physical RX/TX descriptors
and caches once per actual pool/port. VLAN bytes change pps/lengths from the
untagged illustration.

Include a packetization margin in `burst_bound_packets`. It must cover the
registered arrival/service burst, not be an unexplained zero. Include multi-
segment expansion if supported. Sum per-direction allocations and actual byte
footprints for pools, both FIFOs and the ledger. Additional host-held backlog
caused by scheduler stalls needs an explicit finite reserve and is a failure
if exhausted. There is no finite zero-loss guarantee for arbitrarily long CPU
stalls or an unbounded failed TX device.

For the all-MTU example, 512 ms yields about 4.161249M packets per direction.
Small packets/ACKs increase demand sharply; 32 GiB is not a guarantee for any
packet mix. Startup must reject an undersized declared envelope or downgrade
the supported profile explicitly. After startup, allocation/storage exhaustion
is an infrastructure failure, never an intentional Q drop.

Allocate each RX pool near its ingress NIC and bind its owner appropriately.
The existing two NICs sit on different NUMA nodes: remote egress DMA remains.
Record locality rather than claiming it is eliminated. Preserve mbuf ownership
and move pointers. P0 estimates minimum payload memory traffic of 50 GB/s for
bidirectional 100G; copying each payload adds about another 50 GB/s. Performance
and actual per-node hugepage availability remain unverified.

## 7. Ownership, drops and TX completion

The Rust core borrows opaque host-owned packet pointers while queued. Required
FFI semantics (names may follow the existing API style):

- Construct returns a handle or explicit configuration/allocation error.
- Offer returns `ACCEPTED`, `Q_DROP`, `TRACE_LOSS_DROP`, `TRACE_OUTAGE_DROP`,
  `STORAGE_DROP` or `INVALID_INPUT`. If policy evaluation is deferred to virtual
  service time, return that later trace outcome through the drain API.
  Only ACCEPTED transfers temporary custody to the core; other results leave
  the pointer with the host to free exactly once. No Rust panic crosses FFI.
- Try-pop returns a record only when the minimum key is due; empty/not-due is
  explicit. The host also supplies/advances the proven input frontier (or marks
  end-of-input); pop respects both bounds. The record carries seq/deadline for
  tracing and TX order checks.
- Counter snapshot reports packets AND applicable bytes with its watermark.
- Normal destroy requires empty payload queues and a drained service ledger,
  including future accounting entries for already trace-dropped payloads.
  Forced abort returns/enumerates all
  retained pointers for host freeing and counts shutdown discards. Dropping a
  container of opaque pointers is not mbuf reclamation.

For a short `rte_eth_tx_burst` return, only the accepted prefix belongs to the
PMD. Keep unsent packets at the front of their line/direction pending TX lists;
retry before any newer packet for that line/direction. Bound retry memory and time,
report backpressure, and invalidate on terminal failure. Other directions can
continue draining. At a shared port, retain line identity on unsent packets and
reapply the port arbiter/outage gates; held LEO must not block safe cable heads.
This changes cross-line eligibility only, never order within a line.
Do not roll a failed TX packet back into model Q or recompute
its deadline. A full handoff ring follows the same order-preserving policy or
causes an explicitly counted invalid run. A successful TX call is not proof of
wire departure; verify receiver/port observations independently.

| Counter family | Examples and interpretation |
|---|---|
| Intended model loss | `q_drop_packets/q_drop_l2_bytes`; only byte-admission rejection, never a storage shortage |
| Trace loss/outage | `trace_loss_drop`, `trace_outage_drop`, outage-held packets/bytes and clamp extras; independent of Q/resource counters |
| Core storage | FIFO/ledger reserve failure, `delay_storage_drop`, per-queue/combined occupancy high-water marks; any drop invalidates G2 |
| Mempool | software allocation failures, PMD `rx_nombuf`, pool free/used high-water; exhaustion cannot be reclassified as Q loss |
| NIC | PMD `imissed`, `ierrors`, `oerrors` and supported port xstats (e.g. physical discards), start/end/reset evidence |
| Handoff/TX | RX/worker ring loss, TX ring loss, short-TX retries (events, not losses), terminal unsent discards, `missed_deadline_outage_drop`, pending peak and deadline lateness |
| Input/lifecycle | unsupported frames, invalid timestamps, forced-shutdown discards, duplicates and sequence-check errors |
| Observation | logger/capture loss, missing pairs, ambiguous identity, invalid clocks; these affect evidence validity, not model Q |

Counters are per direction and use nonoverlapping terminal software loss reasons.
Some NIC/mempool counters overlap in hardware; report raw values and documented
semantics instead of adding them as independent packet losses. No unsupported
counter may be silently treated as zero. Any gap in full-path conservation
must be explained or yields INCONCLUSIVE/FAIL, not PASS.

At software quiescence (excluding packets never returned by RX):

```text
rx_returned = q_drop + trace_loss_drop + trace_outage_drop
              + invalid_input_drop + storage_drop + handoff_drop
              + terminal_tx_drop + missed_deadline_outage_drop
              + shutdown_drop + tx_accepted + still_owned
```

For a successful drained run, `still_owned=0`, non-model software drops=0, and all
mbufs are reclaimed after PMD completion/port cleanup. Also reconcile generator,
NIC and receiver counts, with Q/trace drops and observation coverage separated.
`tx_accepted` is not a substitute for receiver delivery. Count every accepted
packet exactly once, including those waiting in RX rings, core or pending TX.

Shutdown protocol: stop traffic/RX admission; publish producer completion only
after enqueuing its last batch; drain RX handoffs, core deadlines, output rings
and pending TX; wait/reclaim PMD-owned mbufs; snapshot counters; destroy empty
states. Timeout/forced shutdown records outstanding IDs/counts and invalidates
the run. Cover both fused and split host layouts, including zero traffic and a
last partial burst. A stop flag alone is not evidence that a ring is empty.

## 8. Offline test plan and acceptance oracles

Core tests use a fake integer clock and integer packet IDs/opaque dummy pointers;
they need no DPDK or NIC. Use a separate finite-Q model plus stable global sort
as the oracle, not the production two-FIFO functions. Leave the baseline legacy
tests intact. Proposed execution is `cargo test --locked` plus a small C FFI
smoke test against the generated header. These are implementation acceptance
commands, not tests already run by this design task.

| Test | Required assertion |
|---|---|
| Direction isolation | Saturate A→B; B→A's Q/S/selection/deadlines do not change; asymmetric parameters and interleaved batches work |
| Shared bottleneck | Multiple flows contend for one Q and service tail; changing flow IDs leaves results unchanged; adding flows cannot multiply C/Q |
| Queue-before-delay | Identical arrivals/C/Q with different t/δ/p have identical admission decisions and service completions |
| Finite byte Q | Exact-fit accepted, overflow rejected, oversized frame/Q=0 rejected, release at equality accepted; in-service bytes count; no phantom S after drop |
| Storage separation | Service-completed packets can stay in delay without consuming Q; below configured capacity no storage drop; forced shortage reports storage, frees once and invalidates |
| Accounting | Minimum frame, 1514-byte mbuf, tagged frame with strip disabled, mixed sizes; Q charged L2 and C charged wire bytes exactly once; reject unsupported chain/offload |
| Fixed service arithmetic | Fractional service intervals do not accumulate rounding drift; nondivisible C, burst arrivals, idle reset, near-overflow and backward time |
| Stable merge | Empty/one-empty queues, `now<d`, `now=d`, multiple due packets, δ=0 and cross-queue equal deadlines; compare every emitted ID to stable-sort oracle |
| Same-flow order | Multi-flow random arrivals/lengths, all ordinary, all late, alternating selections; only earlier-late/later-ordinary inversions predicted by oracle |
| Exact scheduled δ | For same admitted input with forced I=0/1, deadline difference equals δ; late propagation equals t+δ; compare before TX |
| Reproducibility | Fixed selector vectors and seeds, p=0/1, identical input across burst sizes and interleaved directions; Q drops still consume seq/draw |
| Ledger watermark | Delay processing of older RX records while fake wall time advances; admission matches event-time oracle rather than falsely empty wall-time Q |
| Input completeness | Interleave admission/dequeue with old RX/handoff backlog; due late head waits for unseen earlier-deadline ordinary packet; test markers, idle advancement, equal arrivals, bounded batches and end-of-input |
| TX ownership/order | Inject returns 0, partial, then full; no duplicate/free of accepted prefix, no overtake of pending suffix, retry exhaustion counted separately |
| Drain | Stop with nonempty RX/TX rings, both FIFOs and partial TX batch; all custody resolved or explicit abort count; zero-load and repeated create/destroy |
| Config/ABI | Invalid/mixed JSON fails before RX; checked arithmetic; no FFI panic; C/header/layout match; every offer outcome exercised |

Worked oracle fixture: C=8 Gbit/s, every mbuf 76 B (`q=80`, `w=100`, service
100 ns), Q=160 B, t=10 ns, δ=100 ns. Offer seq 0 late and seq 1 ordinary at
a=0: S completions are 100 and 200 ns; both deadlines are 210 ns. Seq 2 at a=0
is Q-dropped. At a=100, seq 3 ordinary is accepted after seq 0's Q bytes release;
its completion/deadline are 300/310 ns. Output IDs are **0, 1, 3**. With δ=101
the first two deadlines become 211/210 and output is **1, 0, 3**. This tests a
tie, permitted inversion, byte release and lack of service charge on a drop.
Online edge: hold seq 1 in RX after admitting only seq 0 with δ=101. At now=211,
W=0 and W+t=10, seq 0 must not pop. Admit seq 1, then consume an ordered
end-of-input marker; seq 1 is emitted before seq 0. An implementation that emits
seq 0 just because it is due fails, even if all offline sorting tests pass.

### Software PMDs: integration, not hardware timing proof

Use isolated process prefixes, small pools and no physical PCI device access.
For tests on development host, request `--no-pci --no-huge` where supported by the selected
DPDK build; do not change system hugepages or bind any physical interface.
All output pcaps go to new per-run paths because pcap TX overwrites files.

- **null PMD:** allocation, wiring, counters and sustained-loop smoke tests.
  It generates zero-filled packets and discards transmitted packets. It cannot
  establish output identity/order or hardware loss/timing; use an explicit mock
  for short-TX failure injection. [DPDK null PMD][null-pmd]
- **pcap PMD:** two virtual ports with direction-specific fixtures and
  `rx_pcap`/`tx_pcap`; verify packet bytes, mapping, stable output IDs and drain.
  Do not assume pcap input timestamps pace reception. Inject fixture arrival
  times into the test clock for model comparisons; pcap wall time is not G2.
  [DPDK pcap PMD][pcap-pmd]
- **af_packet PMD:** optional local namespace/veth smoke for packet I/O and
  low-rate bidirectional behavior, only on disposable test interfaces. It uses
  Linux AF_PACKET and requires local namespace/network capabilities. Missing
  privileges mean SKIP with reason, not a reason to access testbed. Kernel
  timing cannot validate CX-4 precision. [DPDK af_packet PMD][af-packet-pmd]

The first two are sufficient for hardware-free integration when their build
dependencies are available. Log missing compiler/PMD/libpcap as an environment
block; never skip hooks to push. A null-PMD pass alone cannot pass the property
matrix. No software test result is a 10G/100G throughput claim.

## 9. G2 plan: 10G, then 100G, after approval

This section is a test plan, not permission to run hardware. experiment coordinator
arranges PF handover/restoration and Mark approves the time slot. development host sessions
do not connect to testbed. Keep existing Stage 1/1G work and borrowers intact.

1. Freeze P1-0 manifest: exact versions, C/Q/bytes, seeds, cross-NUMA placement,
   memory bound, clock conversion and observation uncertainty. Prove generator
   bypass capacity and receiver/capture correctness before testing the emulator.
2. At each offered-rate stage, test each direction and both simultaneously:
   t=65/142 ms; p=0 then proposed p=0.01/0.10; δ=5/320 ms. Include δ=0 and p=1
   as correctness/resource edges, at safe admitted load. Start with numbered
   MTU-1500 synthetic traffic, then mixed sizes/multiple flows, then ASTF and
   production apps. Register the precise matrix before executing it.
3. Use no-intended-queue calibration first; then shared-Q stress with C below
   offered load (illustratively 8–9G at the 10G stage, 80–90G at 100G). Report
   physical link rate separately from offered rate. Record Q occupancy/drops
   and ensure an external merger has not already discarded the offered load.
4. Warm up beyond t+δ and Q fill, then observe steady state. P0 proposes at
   least 10 minutes per cell with independent repeats; experiment coordinator
   registers durations/repeats within approved access. Include fill, transitions
   and final drain. Do not discard tails/missing records to improve a gate.
5. Pass every required cell at 10G before proceeding to 100G. No 40G gate.
   Keep timestamps, full-sequence checking and registered logging enabled in
   the qualifying run. Record CPU cost, NUMA traffic, occupancy and pool peaks.

Define measurement endpoints and serialization convention before testing.
For no-queue ordinary calibration, use the plan's raw
`abs(measured_OWD - t)`; for selected packets use `abs(measured_OWD-(t+δ))`.
Do not subtract a fitted emulator baseline to make this pass. Serialization
and observer overhead must be reported with clock uncertainty. For stress,
compare to the **independent preregistered** reference `s_i-a_i+t+I_i*δ`, with
the chosen timestamp/serialization convention, not measured excess relabeled
as “queue delay”. Include any known endpoint offset in the preregistered
measurement model and report both raw and modeled timing results.

For each direction, rate and required cell:

| Gate | Acceptance |
|---|---|
| Timing center | median absolute OWD error ≤ 0.1 ms |
| Timing tail | p99 of that absolute error ≤ 0.5 ms, not p99 of signed error |
| Order | Zero unexpected reorder events against the late-selection/finite-Q oracle; ordinary subsequence order and allowed overtakes checked separately |
| Storage | Zero delay-storage drops; intended Q and trace losses listed separately; all other loss layers and drain conservation resolved |

The independent finite-Q/order oracle requires **every ordered ingress event**,
not only timing samples. Supply `(run, ingress_port, line, ingress_seq,
selection_seq, external_packet_identity, arrival_ns, packet_length/length_flags)`
before Q/loss admission, plus input-frontier/end markers and the registered
trace/configuration. Either persist the complete event stream or feed an
independent streaming reference every event. The reference computes its own
Q decisions, service times, selection and deadlines; production-computed
deadlines/outcomes are comparison outputs, never its sole input. Reconcile
expected survivors and intentional losses with full receiver identities,
actual-time port arbitration evidence (section 5a) and terminal host outcomes.
An unsampled packet can change all subsequent service
times, so sampling cannot omit its input to this model.

Bound and measure the oracle/logging queues, processing capacity and storage
bandwidth in P1-0. Any dropped oracle event or unavailable identity invalidates
the full-model evidence; record the gap and report INCONCLUSIVE. A slow observer
must not backpressure the emulator into a hidden bottleneck. Save reference
version, event counts/digests, mismatch records and all observation-loss counts.
If complete evidence cannot be obtained at 100G, do not claim complete G2 there.

Require full-traffic synthetic sequence checking for zero-reorder/loss claims.
Timing can use independent hash sampling (P0 proposes 1/1024), with counts,
sampling seed, confidence intervals and time-window diagnostics. Keep the
uniform main sample separate from oversampled late/app strata. Log missing
pairs and clock invalidity; unresolved bias or uncertainty at a threshold is
INCONCLUSIVE. Production fingerprints must handle retransmission/duplicate
ambiguity; an unverifiable mix does not receive full G2 PASS.

CX-4 RX timestamp capability is not proof of wire-time accuracy. Two cards'
raw counters require a validated common-time conversion; NTP on the host is
insufficient. Do not assume per-packet TX wire timestamps or launch-time
scheduling. P0 VI documents these limitations and timestamp-related CQE
compression costs. Poll/submit time and sampled pcap logs cannot replace an
independent receiver oracle. Any non-model loss, unexplained reorder or failed
timing gate stops escalation. `R_valid` is the largest fully passed rate, not
the highest rate whose throughput counter moved.

## 10. Risks, decisions to register and review handoff

The engineering defaults in this document are explicit proposals: Q includes
in-service L2 bytes, C includes wire overhead, event-time admission, integer
rounding, the named counter selector, service-time trace lookup, retained-packet
clamp and explicit release-time outage actions. They make implementation possible now.
experiment coordinator/research records must register them in the experiment manifest
before interpreting results; changing them requires changed fixtures/oracle.

| Owner | Remaining decision or evidence | Consequence |
|---|---|---|
| experiment coordinator + research records | Register Q byte convention, C/Q values, workload/burst envelope and exact G2 matrix/repetitions | Prevents comparing different bottleneck models under one label |
| experiment coordinator | Validate RX observation point, clock mapping/uncertainty and full-flow identity coverage | Scheduler proof cannot establish measured G2; unresolved observer error means INCONCLUSIVE |
| implementation worker + design review | Demonstrate serialized owner throughput, exact fractional arithmetic cost, bounded memory and TX suffix handling | Single owner may bottleneck; more per-flow workers cannot silently replace shared C/Q |
| experiment coordinator | Actual pool stride, hugepages per NUMA node, cross-socket DMA/CPU interference | 32 GiB and theoretical memory bandwidth are estimates, not capacity certification |
| experiment coordinator | Pin a working CX-4/DPDK/rdma-core/firmware combination; generator/observer timestamp-on capacity | Upstream README and P0 version snippets do not prove this combination |
| experiment coordinator + research records | Register normalized trace direction/OWD conversion, loss reconstruction, outage drop/hold semantics, terminal policy and segment provenance | Missing samples are not automatically outages; faithful per-packet late replay remains deferred |
| research records | Interpretation of realization error when late releases collide at full line rate | Exact scheduled δ remains true, but physical extra delay can exceed δ; report errors, do not redefine deadlines |
| Mark via experiment coordinator | Hardware slot/VM/PF access after Stage 1 or approved idle daytime; any later hardware change | No testbed access or hardware changes in this task |

When implementation worker requests review, provide host/core SHAs, diff base, test output,
resolved example config and pending gaps. Review every row of sections 2/8,
especially finite Q release, input watermark, direction mapping, stable ties,
fractional service time, selection replay, mbuf custody, short TX and shutdown.
Core unit tests alone do not sign off the host or G2. Return blocking findings
with file/line, a reproducing input and the violated invariant; recheck fixes
against new SHAs. Push only work branches; never merge/push main from this task.

## 11. Validation of this design document

Offline checks on 2026-10-29:

- A separate Python `Fraction` reference and a two-FIFO sketch agreed on 1,000
  traces of 100 events each, checking finite-Q admission, fractional service,
  stable output, both subsequence orders and exact scheduled δ.
- Another 1,000 traces of 100 variable-delay events preserved FIFO order after
  the clamp. Explicit UP/DOWN, stable tie, Q release/drop, input-frontier
  counterexample, outage hold/drop and P0 sizing fixtures passed.
- `cargo test --locked` on this documentation branch: **9 passed, 0 failed**.
  These are upstream legacy core tests, not HNG implementation acceptance.
- An independent builder review found missing input-completeness and
  full-oracle-evidence requirements; both were added. Its trace review clarified
  actual-time outage losses, cross-line arbitration and terminal HOLD semantics.

The arithmetic script is an intermediate artifact at
`<shared-outbox>/2026-10-29-dpds-design-check-v1.py`.
These checks validate formulas and examples, not the DPDS host or a deployed
HNG engine. No DPDK PMD integration or hardware/G2 test was run by this task.

[null-pmd]: https://doc.dpdk.org/guides-25.11/nics/null.html
[pcap-pmd]: https://doc.dpdk.org/guides-25.11/nics/pcap_ring.html
[af-packet-pmd]: https://doc.dpdk.org/guides-25.11/nics/af_packet.html
