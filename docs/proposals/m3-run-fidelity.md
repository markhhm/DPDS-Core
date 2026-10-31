<!-- SPDX-License-Identifier: Apache-2.0 -->

# Realized late-run fidelity specification

Status: **ADOPTED, 2026-11-01**. Policy identifier: `m3-fidelity-v1`.
This path is retained for existing links. Historical reports keep their original
pending verdicts; replay with this version to obtain an adopted-policy verdict.

The adopted converter subdivision tolerance is **0.01 ms (10,000 ns)**,
recorded as `recipe.adaptive_ns`. Boundary and latency quantization remain
1,000 ns. Match the independent TheaterQ reference with `adaptive_ms=0.01`;
the previous 0.05 ms sequence is a historical comparison, not the same recipe.
This changes the converter representation, not any fidelity budget below.
The [canonical assessment](../reports/m3-canonical.md) records the selected
56-case replay, interval counts, JSON sizes and measured Engine load times.

Compare measured source and actual HngBuffer output on identical received packet
identities, ordered by original send/ingress time. Output OWD is modeled release
minus arrival and includes serialization and clamp; subtract the declared
propagation offset as an integer. Use the full-source, first-send-anchored 15 s
median baseline and strict excess >20 ms. Sequence gaps and send gaps >3 times
the original median cadence split runs. Missing samples do not imply loss.

All three inclusive budgets must pass per window and direction:

- Late-packet count: `abs(L_engine-L_source) <= max(2, ceil(L_source/20))`.
- Changed runs: `changed_runs <= max(1, ceil(R_source/20))`.
- Absolute `P(next late | late)` difference: <=1 percentage point.

Run matching uses packet identity. Represent each run by its inclusive first and
last packet in the common ingress cohort, after applying both gap rules. Build
connected components of source/output runs sharing at least one packet. Identical
runs cost zero; every changed component costs the larger of its source and output
run counts. Thus an extended/shortened run costs one, an insertion/deletion costs
one, a one-to-two split or two-to-one merge costs two. Disjoint runs cost separately.
Identical histograms with different packet membership can still count as changed.
The run-length CDF distance remains descriptive and is not an acceptance gate.

Evaluate ceilings using integer arithmetic `(count+19)//20`. Evaluate probability
by cross multiplication of the observed J/E ratios, without floating rounding.
E counts eligible late-to-next pairs, J counts eligible late-to-late pairs. E is
neither L-1 nor J. When E=0 the probability and its check are null.

Minimum evidence in **both source and output** is L>=100, R>=20 and E>=100.
Check these six conditions separately before a fidelity verdict. Deficient
windows remain in the inventory as INCONCLUSIVE, including their observed
budget checks. Among windows meeting minima, any exceeded budget is FAIL and
all three passing is PASS. Canonical M3 fidelity requires every sufficient
window to pass; insufficient windows are listed without deletion or pooling.
An empty sufficient set cannot establish acceptance. Packet identity/count or
independent deadline-oracle errors are correctness failures, even with low counts.
A fidelity PASS alone does not establish canonical membership or PMD correctness.

The approved additional natural windows are:

| Paired source timestamp | Window, seconds |
| --- | --- |
| 2025-04-16-09-26-16 | [38,49), 11 s |
| 2025-04-16-09-26-16 | [8,48), 40 s |
| 2025-04-19-01-16-16 | [8.95,48.95), 40 s |

Register paired ZIP/member hashes, phase and window manifests **before** replay.
Preserve the full 11 s inventory and exclusion reasons. Evaluate each direction
and corridor separately; repeats and overlaps do not add independent source runs.

Fixed two-point delta/p mode supplies Stage 1 grid controls. It does not model
LEO correlation; trace replay supplies burst structure. Its known base t and
independently generated late flags are the reference. Preserve source order
when permitted ordinary/late overtaking changes output order. The synthetic
small downward-step case remains INCONCLUSIVE rather than weakening minima.

Software replay is not wire precision, throughput evidence or G2. Physical
acceptance requires at least **five independent repeats per cell (estimate)**,
all passing, with registered clocks, uncertainty, load and capture accounting.
Those physical runs are not performed by this software-only workflow.
