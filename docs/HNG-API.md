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
horizon. It is different from outage hold. Coverage exhaustion is invalid;
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
