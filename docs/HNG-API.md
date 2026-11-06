<!-- SPDX-License-Identifier: Apache-2.0 -->

# HNG software API and configuration

The contract is [HNG-DESIGN.md](HNG-DESIGN.md). This implementation preserves
DPDS's original distribution/limiter API and adds `hng_engine_*` separately.
The host uses HNG JSON exclusively. Legacy and mixed configurations fail before RX.

## Build and check

```sh
cargo test --locked
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo build --locked --release
```

The existing cbindgen build produces `target/libdpds_core.h`; compile the host
against that header and the matching library. `hng.config.example.json` is a
small four-state software example, not a 100G hardware allocation.

## Model and byte convention

Each `lines` entry owns one finite shared C/Q for one VLAN and direction.
`model.c_bps` is wire bits/s; `q_bytes` includes the whole in-service L2 frame.
For an mbuf that excludes FCS, `q=max(pkt_len+4,64)` and `wire=q+20` bytes.
Absolute service completions use exact `u128` rational units and are rounded up
once to nanoseconds. Ordinary and late FIFO heads merge by release time and
physical ingress sequence. Per-line selection ordinals advance independently.

`p` is a decimal **string** with up to 18 fractional digits. The recorded
threshold is `floor(p*2^64)`, with separate p=0/1 handling. The selector is
`splitmix64-counter-v1` from the design. The model never uses the wall clock
as a random seed.

All model fields, envelope fields and mapping fields in the example are required.
Only `trace` may be null. Unknown fields and unsupported policies fail parsing.
`envelope` declares `min_q_bytes`, `max_q_bytes` and a positive
`burst_bound_packets`. Startup checks packet and stored-byte capacities against
Q, worst-case delay and the registered burst envelope. It also reserves both
FIFOs and the service ledger before traffic. The host adds actual PMD, ring,
cache and pending-TX reserves to its mbuf pools. These are resource checks,
not a proof that a particular CPU, packet mix or NUMA layout sustains 100G.
`hng_slot_metadata_bytes()` returns the core metadata reserved per storage slot
(two FIFO entries and one ledger entry), and `hng_engine_metadata_bytes(engine)`
the bytes actually reserved by all lines. The host pool calculator uses both.

## Trace files

Replace a line's null `trace` with:

```json
{"path":"traces/leo.json","sha256":"64-hex-digit-digest-of-that-file"}
```

The path is relative to the engine configuration file. Set model `t_ns=0`,
`p="0"` and `delta_ns=0`. The normalized document format is demonstrated by
[`tests/fixtures/trace.json`](../tests/fixtures/trace.json), with its synthetic
source CSV beside it. `sha256sum` is required at startup. The loader hashes the
exact bytes it parses, validates provenance, and loads all intervals before RX.
It performs no file access in the packet loop.

The document requires:

- `schema="hng-trace-v1"`, source ID, direction, source SHA-256 and conversion recipe.
- `units="ns_owd"`, `trace_epoch_ns=0`, `time_scale=1`.
- `splice_policy`: `natural_hold`, `paired_splice` or `unspliced_phases`.
- A profile with strictly increasing `at_ns` rows from zero and a finite `end_ns`.
- Per-row `t_ns`, decimal-string `loss` probability and explicit `outage` boolean.
- Independent `loss_seed`; `outage_action="drop"` or `"hold"`.
- `terminal="error"` with null horizon, or `"hold_last_sample"` with a finite
  `terminal_horizon_ns` greater than the original end.

Intervals are sample-and-hold, left closed and right open. Delay and loss lookup
use rounded C/Q service completion e. Retained packets use
`d=max(e+t(e),previous_retained_d)`; the floor survives empty queues. A lost
packet still consumes its C service and Q bytes. Trace loss and outage drop have
separate counters. Clamp packet count, total and maximum extra delay are exposed.

Outages act at the candidate d: drop discards it; hold maps its release to the
next up instant. Actual-time TX gates must be checked again on every retry.
A modeled survivor discarded because actual TX time is down is a non-model
`missed_deadline_outage_drop` and fails the run. A held LEO line can be bypassed
by eligible cable traffic, while each line preserves its own order.

`hold_last_sample` extends the last trace values only through the declared
horizon. Before RX, `Engine::from_json` requires the extension to cover
`max(t) + delta + queue_drain`, using the actual line rather than trusting a
converter allowance. The conservative bound in ns is
`ceil((q_bytes + 20*floor(q_bytes/min_q_bytes))*8e9/c_bps)`:
Q contains L2 bytes including the in-service packet, and each admitted frame
adds 20 wire bytes. Integer ceiling preserves fractional-ns drain time. For
Q=80 bytes, minimum frame=80 bytes and C=8 Gbit/s, drain is 100 ns; a zero
allowance fails. The horizon remains exclusive, so stop admissions at the
registered end and drain before it. This check is different from outage hold.
Coverage exhaustion is invalid;
the engine never wraps or silently concatenates traces. Source conversion and
the 05:15/10:00 splice protocol remain experiment responsibilities. The clamp
does not validate splicing or reproduce source packet-specific reordering.

Only probabilistic interval loss is currently supported. The offline
[Garcia converter](TRACE-TOOLS.md) produces delay-only intervals with source
hashes, recipe and late-run diagnostics. It does not reconstruct outages or
packet-specific loss. Exact identity masks, Choi/OneWeb conversion and arbitrary
per-packet late amounts require separate input adapters/policies. Unsupported
fields/policies are rejected, rather than treated as FIFO replay.

## Custody and shutdown

Call `hng_engine_load` before starting RX, inspect line information, and create
independent `hng_tx_policy_new` handles for the TX owner. The Engine has a single
mutable owner. TX policies are immutable independent copies and may be read
concurrently; destroy them only after TX exits.

`hng_engine_offer` takes the original RX arrival ns and physical ingress ordinal.
Only `Accepted` lends the pointer to the core. All other outcomes leave the
pointer with the host to free exactly once. Peeking never transfers custody.
A successful pop returns the pointer and its ideal deadline, release time,
service completion, selection ordinal and byte accounting.

The caller must supply a certified input frontier W: all earlier arrivals have
been processed. Both ready/peek and pop require release <= now and release <=
W+minimum_delay. For shared ports, the host must check that bound for every line
feeding the port. A wall-clock sample cannot stand in for an unconsumed RX
frontier. Q releases only on ordered admissions while input remains open.

After RX stops and every handoff drains, call `hng_engine_end_input`. Then call
`hng_engine_drain_advance` as the clock advances, continue output, and wait until
`hng_engine_has_backlog` is false. This includes future service-ledger entries
for already trace-dropped payloads. Normal destroy refuses payload or ledger
backlog. On an invalidating forced abort, extract/free every remaining pointer,
snapshot Q and shutdown counters, and only then use `hng_engine_abort_destroy`.
No Rust destructor frees an opaque mbuf payload.

Snapshots distinguish payload, L2 and wire bytes, admission and ledger
watermarks, storage occupancy, model loss, trace loss and invalid input. Host
ring/NIC/mempool/TX counters remain separate. PMD TX acceptance is not proof of
wire delivery. This API and its offline tests do not provide the independent
full-ingress/receiver observer or hardware G2 certification.

## Line handles for per-direction scheduler owners

`hng_engine_line(engine, i)` returns a handle for line `i`. The `hng_line_*`
functions (offer, peek, pop, abort_pop, stats, min_delay, end_input,
drain_advance, has_backlog) have the same contracts as the matching
`hng_engine_*` functions, but borrow only that line. Two owner threads may use
handles for disjoint lines of one engine at the same time, for example one
owner per ingress port. Each handle has one owner at a time. No `hng_engine_*`
call may run while a handle is in use, because those calls borrow the whole
engine. Take the handles before launching the owners.

`hng_line_offer_record` is `hng_line_offer` that also returns the admitted
record, so the host can annotate the packet while it is cached.
`hng_lines_pop_merged(lines, watermarks, n, now, limit, out)` pops the smallest
`(release_ns, ingress_seq)` ready head among `n` lines of one egress port when
its release is at most `limit` (the host's bound against unseen input), and
returns the index of that line, or -1. It replaces a peek per line and a pop.

Line storage is reserved and touched (prefaulted) at load, so no page fault
lands in the packet path. At the production envelope this is about 0.6 GiB per
line, done serially before RX. Before reserving, load compares the total with
`MemAvailable` from `/proc/meminfo` and fails with a configuration error if it
does not fit. That is a snapshot: memory taken by other processes afterwards
can still lead to an out-of-memory kill under overcommit. Where
`/proc/meminfo` is missing, only the allocation itself is checked.

## Explicit M7 registered storage

A fixed-model line can opt into `m7-hybrid-v1` with an optional `m7_storage`
object. An absent field preserves the original minimum-frame envelope,
model decisions and C ABI. Explicit `null` is rejected. Trace models retain
their existing path and cannot use this registration.

```json
"m7_storage": {
  "mode": "m7-hybrid-v1",
  "threshold_l2": 1024,
  "hold_ns": 10000000,
  "window_ns": 10000000,
  "offered_bps": 100000000000,
  "margin_numerator": 5,
  "margin_denominator": 4,
  "composition": [
    {"l2_bytes": 64, "weight": 1},
    {"l2_bytes": 512, "weight": 1},
    {"l2_bytes": 1522, "weight": 1}
  ]
}
```

Sizes are charged L2 bytes including FCS; stored bytes are L-4 and wire
bytes L+20. Threshold defaults to 1024 and hold to 10 ms. H is a registered
failure threshold, not a measured scheduling bound. W is fixed at 10 ms
and the physical reserve margin at 5/4. Register 1..16 distinct sizes in
64..1522, with positive integer weights totaling at most 65535. C must
be positive and no greater than the registered offered rate. Production M7
inputs register 100G; software rate fixtures can register a synthetic rate.
Unknown keys, invalid ranges and unrepresentable quota arithmetic fail
before constructing the payload queues.

`hng_m7_storage_plan` calculates without constructing or prefaulting an
engine. `hng_engine_m7_storage_info` returns an immutable startup plan for
one line; legacy lines return false and leave the output unchanged. The
existing `LineInfo` layout remains unchanged.

For delay D=t+delta and hold H, let B=C*(D+H)/8e9, mean charged size muL,
and mean wire size muW=muL+20; times are integer ns. For a per-packet resource
cost r and its weighted average meanR, reserve:

```text
physical(r) = ceil(5/4 * (B*meanR/muW + Q*meanR/muL) + burst*max(r))
bD = min(B, offered_bps*W/8e9)
bQ = min(Q, offered_bps*W/8e9)
window(r) = ceil((B-bD)*meanR/muW + bD*max(r/(L+20))
               +(Q-bQ)*meanR/muL + bQ*max(r/L) + burst*max(r))
live_limit(r) = min(physical(r), window(r))
```

Use r=1 for slots, the compact indicator for records, ceil((L-4)/64) for
compact cells, and the noncompact indicator for retained normal mbufs.
Physical reserves use 5/4, with W supplying additional logical occupancy
limits. This is the intersection of both requirements. W is one cumulative
skew allowance per residence domain, not a fresh allowance in every W-sized
interval. Long-run traffic averages alone do not establish this resident
composition guarantee. Exceeding a limit is a non-model storage failure,
never an additional Q loss.

The model's declared packet/frame capacities must cover the calculated
physical slots; a host intending the smaller memory budget supplies exactly
the derived counts. Core still prefaults its declared metadata allocation,
then enforces the logical slot limit. Registered sizes are checked before
selection. The host enforces record, cell and retained-mbuf limits.

`hng_lines_offer_burst_storage` invokes its storage callback only for
registered lines, after all model rejection gates and before committing
service/Q or payload custody. Model Q drops never invoke it. The callback
receives a call-scoped immutable `HngPacket`; it must copy required metadata
and must not re-enter the engine. A non-NULL replacement pointer transfers
payload custody until pop/abort. NULL leaves the source caller-owned and
returns `StorageDrop`, without committing service/Q. Selection ordinals
remain consumed, as for other storage failures. Classic lines in a mixed
burst follow their original path. The original offer functions remain
available for clients that own their payload storage directly.

For materialization, first reserve TX handoff space, then use
`hng_lines_peek_merged`. Failure leaves model custody intact. With one owner,
the same explicit clock/watermarks/limit and no intervening model mutation,
`hng_lines_pop_merged` returns exactly that head. The host recycles its stable
storage tag only after this pop. Materialization failure must retain the
record for honest INVALID/abort cleanup. It must not reset deadlines, hide
loss as a model decision, or spin indefinitely. Existing retry, drain and
ownership gates remain in force.

The observer event schemas and native binary/JSONL contract do not change.
Hosts report additional M7 storage resource counters separately from the
fixed observer resource vector.

### Software-only population-bounded rate fixture

`hng_engine_load_for_perf` is a separate loader for a closed-loop software
harness. The production binary never calls it. It validates the original
fixture's conservative Core capacities and leaves C, Q, t, delta, p, seed,
and Core metadata allocation unchanged. Only external storage reserves and
logical live limits are capped by a caller-enforced packet population.
`perf_population_bound` is nonzero in its returned plan and must be reported.
This is a bounded working set for CPU comparison, not a production memory
plan or R_valid evidence. The traffic generator must count logical packets,
including compact copies; freeing a source mbuf creates no new traffic credit.
The production loader continues to require the full registered reserve.

### M7 single-selection output ticket

`hng_lines_peek_head` performs the same eligible `(release_ns, ingress_seq)`
merge as the existing peek/pop functions but returns only the selected line,
opaque payload, physical sequence, length, late FIFO and captured clock in
`HngHead`. It does not mutate any line. No-head/invalid-array failures leave
its output unchanged. The host materializes only after its TX free-slot budget.

`hng_head_pop` consumes that selected FIFO head directly, without a second
merged search or another expansion into the full `HngPacket`. It updates the
same dequeue/storage counters and, for sealed input, releases service Q at the
captured clock. Success clears the ticket's line pointer; duplicate or stale
consumption fails before changing Q or counters. A sequence/payload/length
mismatch is rejected. Existing functions and the classic host path are unchanged.

The owner must keep every group handle live and serialize all access. It must
not modify a ticket or mutate **any** group line between peek and pop (no
admission, END, drain, abort, other pop or callback re-entry). Materializing
host storage is permitted. The ticket is not transferable between owners.
A host copy-out failure leaves the model head and ticket unconsumed for abort.
