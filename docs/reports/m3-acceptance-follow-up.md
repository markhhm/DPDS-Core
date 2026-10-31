<!-- SPDX-License-Identifier: Apache-2.0 -->

# M3 acceptance follow-up

This report covers the seven-item review of the trace pipeline. It records
software behavior and leaves milestone acceptance to the research owner.

Historical verdicts below are preserved. The adopted
[fidelity specification](../proposals/m3-run-fidelity.md) and
[canonical replay assessment](m3-canonical.md) supersede the pending-policy discussion.

| Review item | Delivered behavior and evidence |
| --- | --- |
| 1 | Actual HngBuffer replay of converted synthetic and external Garcia traces; independent rational service/deadline oracle; downward-step clamp and zero unexpected reordering. Commit `962fdc2`. |
| 2 | Actual output run-length histograms and P(next late given late), compared in source ingress order; fixed p=0.01/0.10 and delta=30 ms comparisons. Commit `0433179`. |
| 3 | Engine startup checks actual line C/Q wire drain plus maximum delay and delta; zero, one-ns-short, exact, changed-Q and fractional-rate boundary cases. Commit `f9fd516`. |
| 4 | Recipe phase and verified pool item/hash/index; paired identity, missing phase, source margins, gaps and quantization checks. Direct conversion is diagnostic. Commit `4751c9f`. |
| 5 | Drop does not advance retained deadline floor. A deliberately incorrect mutation fails; the unchanged engine passes deadlines 300/310 ns after dropping 1200 ns. Commit `93a44bd`. |
| 6 | Independent Step 2 fixture snapshot, module hash, exact invocation and arithmetic documented in `tests/fixtures/README.md`; external reference regenerates all three rows. Commit `1ecba7c`. |
| 7 | NumPy absence fails with install help before Rust checks. Explicit expected sequence bounds include leading/trailing gaps; absent bounds stay unknown. Tests cover CLI, invalid bounds and partial knowledge. Included with this report. |

## Actual output measurements

The external Garcia figure-1 DL ZIP `2025-04-20-17-06-16__dl.csv.zip` has
SHA-256 `07647bd5b8d64525f5eaca40dcc995581611b593b70e78c0b0776984fe60c236`.
No measured samples or converted real trace are bundled. On the 38-49 s window,
all 41,159 actual scheduler records matched the independent oracle exactly.
19,207 packets incurred clamp; unexpected reordering was zero. Source/output
late counts were 271/272, both with 31 runs; CDF distance was 0.0322581 and
conditional-probability difference was 0.0319276 percentage point. The synthetic
step case changes from five to ten late packets, demonstrating why configured
row equality cannot establish output fidelity. Fixed two-point source/output
histograms matched exactly for both tested probabilities.

These measurements are nonqueueing virtual scheduling, not wire validation or
canonical-pool coverage. Tolerances remain **PROPOSAL_PENDING** in
[the fidelity proposal](../proposals/m3-run-fidelity.md). Proposed differences
are <=0.1 percentage point late share, <=0.05 run-length empirical CDF distance,
and <=1 percentage point conditional probability, with minimum evidence rules.
No source-fidelity PASS gate or approved equivalence claim is enabled.

## Sequence endpoint uncertainty

The Garcia ZIP contains received rows only. Its inspected README does not
supply expected first/last sent sequence numbers or a total sent-packet count.
The observed range is 1..187945 over 187389 rows, with 556 missing sequence numbers between received rows.
This is not a complete missing count and is not packet loss. Leading/trailing
counts and total remain unknown until independent endpoint registration exists.
The converter accepts `--expected-first-seq` / `--expected-last-seq`; a partial
registration makes only that side known. Both bounds are recorded in the recipe.
No arbitrary zero origin or observed final row is substituted for a missing bound.

## Reproduction and scope

Install the offline requirements in a venv and set PYTHON for `scripts/check.sh`
and normal pushes. The shared script performs policy/SPDX checks, Rust format,
Clippy and tests, builds `trace_replay`, and runs the Python suite. Supply
`REAL_GARCIA_ZIP` to exercise the external-data replay; synthetic checks always
run. The NumPy absence check is a failure, not a skip. Targeted RED/GREEN checks
cover the added behavior; item 5 uses mutation sensitivity and item 6 uses the
independent original reference. Each delivery runs the shared checks before
normal push and again through the hook.

Apache-2.0, NOTICE and upstream author metadata are retained. New owned files
carry SPDX headers; no reference implementation code or measured trace is copied
into the repository. Configuration examples and fixtures use portable synthetic
values. The engine has no DPDK/Python/NumPy dependency. Public CLI and startup
validation contracts are documented in TRACE-TOOLS.md and HNG-API.md.
OneWeb/Choi conversion, hardware timing and milestone acceptance remain outside
this follow-up. No hardware or testbed connection was used.
