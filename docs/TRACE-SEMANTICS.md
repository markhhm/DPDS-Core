<!-- SPDX-License-Identifier: Apache-2.0 -->

# Registered M3 trace semantics

Registration: research decision of 2026-10-29 20:55, items 1-6.
This supersedes the unresolved-policy discussion in the dated inventory.

## Configured equality and engine differences

M3 requires equal configured interval delays. P1 retains its monotone deadline
clamp across every delay step; no `route_fifo` policy is added. TheaterQ changes
route IDs at detected steps and preserves order within each route. A downward
step can therefore produce different packet deadlines/order despite identical
configured intervals. Configuration equality does not certify deadline equality.

| Operation | P1 | TheaterQ reference |
| --- | --- | --- |
| Delay lookup | Rounded virtual C/Q service completion | Active row at dequeue wall time |
| Trace loss | After finite-Q admission/service; lost packets consume service | At enqueue, before service |
| Ordering | Global monotone deadline clamp per line | FIFO within the current route ID |

Cross-engine packet-level comparisons are restricted to nonqueueing conditions
and must state the respective release rules. Even there, global-clamp and
cross-route overtaking deadlines need not be equal. Under stress, compare
statistics and disclose these lookup/loss/order differences; do not infer
packet-level equivalence. The independent P1 oracle still checks P1 deadlines.

## Propagation conversion

`OWD_LEO = OWD_Garcia - 11 ms + 4 ms + d_PoP`.
Use the exact registered values, without whole-millisecond approximation:

| Corridor | d_PoP | Resulting offset |
| --- | --- | --- |
| A | 52.6 ms | +45.6 ms = 45,600,000 ns |
| B | 113.8 ms | +106.8 ms = 106,800,000 ns |

The conversion recipe records the formula and its components. Source-native
latency quantization remains unchanged, followed by the exact integer offset.
The interval comparator applies that same declared offset to native reference
rows. Explicit-offset diagnostic conversion remains available.

## Canonical source eligibility

Exclude a window intersecting a received-sample gap strictly greater than
500,000,000 ns. Exactly 0.5 s does not trigger this rule. Missing sequence
numbers are missing samples, not primary-profile packet loss.

Exclude paired runs without a detectable reconfiguration phase. Derive the
common phase from DL as in the reference generator: samples exceeding median
OWD + 15 ms, at least 21 such samples, 300 phase bins over 15 s, and the first
maximum after neighboring-bin summation. Both directions use the same start.
Use one natural segment of at most 45 s with 0.5 s source margins; never splice.
Preserve original ZIP hashes and explicit reasons for all excluded windows/runs.

## Finite terminal HOLD and outages

Register the converted maximum propagation delay `t_max`, added late allowance
`delta_ns`, and maximum C/Q drain time `queue_drain_ns`. The finite extension
must be at least `t_max + delta_ns + queue_drain_ns`. Record every component,
the extension and final horizon in the conversion recipe/manifest. A trace's
synthetic late delta is zero; a nonzero allowance only increases coverage and
does not insert late packets. Caller-provided queue drain must cover the actual
run configuration. Terminal coverage is half-open, and admission/drain must fit.

Outages are separately registered synthetic profiles, one `drop` and one `hold`;
never infer them from Garcia gaps. Outage hold delays release until recovery;
terminal HOLD extends the last sample. These are distinct operations.

Registered conversion fixture: natural duration 15 ms, native maximum delay
50 ms, corridor A offset 45.6 ms, so `t_max=95.6 ms`. With `delta=0` and
`queue_drain=50 ms`, terminal extension is 145.6 ms and horizon is 160.6 ms.
The separate allowance test uses delta 5 ms, extension 150.6 ms and horizon
165.6 ms. These are fixture values, not defaults for an arbitrary real queue.

OneWeb and Choi adapters await source-subset and feasibility decisions. Their
raw data is not included. Source licenses and attribution remain as recorded
in the inventory; code remains Apache-2.0.
