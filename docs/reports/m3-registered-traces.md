<!-- SPDX-License-Identifier: Apache-2.0 -->

# M3 registered trace semantics implementation

Date: 2026-10-30. Implements research decision 2026-10-29 20:55, items 1-6.
The [registered semantics](../TRACE-SEMANTICS.md) supersede the earlier open
questions. The [tool guide](../TRACE-TOOLS.md) documents CLI and output formats.

## Delivered

- Configured interval-delay equality is the M3 contract. P1 retains clamp; no
  route-FIFO mode or scheduling-core behavior changed. Cross-engine packet
  comparisons are restricted to nonqueueing conditions and still disclose
  release-rule differences. Stress comparisons use statistics with explicit
  lookup/loss differences.
- `--corridor A|B` applies `native - 11 + 4 + d_PoP` with exact offsets
  +45.6/+106.8 ms. Both formula components and the applied integer offset are
  recorded. Explicit-offset diagnostic conversion remains available.
- `garcia_pool.py` lists paired natural windows, ZIP/member hashes and exclusion
  reasons. No-phase runs and windows crossing a gap strictly greater than
  0.5 s are excluded; no fallback phase or inferred outage is introduced.
- `--queue-drain-ns` registers finite terminal HOLD from the actual maximum
  converted delay plus the explicit delta/Q drain allowance. All numeric
  components and the horizon are recorded and validated by the comparator.
  Without that registration the output is diagnostic-only, with terminal error.
- Separate synthetic drop/hold outage fixtures register [200,300) ms down,
  10 ms propagation, zero delta, 50 ms Q allowance, 60 ms terminal extension
  and a 1,060 ms horizon. Engine integration tests check load/provenance,
  exact deadlines/releases, gate boundaries and distinct outage-drop counters.

OneWeb and Choi adapters remain deferred pending their source-subset/feasibility
decisions. No raw measured or normalized trace is included in this repository.
Late-run histograms and source/configured comparisons remain in both tool reports.

## Verification

Synthetic Python tests cover exact corridor arithmetic, recipe components,
strict gap cutoff, no-phase rejection, paired phase alignment, exclusion
provenance, finite HOLD arithmetic and insufficient-budget rejection, alongside
the earlier conversion/comparison regressions. The registered outage test uses
the existing Rust engine; its reference service time is 1 ms per packet and
its expected release times are independent integer constants.

An additional real-source check used `2025-04-20-17-06-16__dl.csv.zip`
(SHA-256 `07647bd5b8d64525f5eaca40dcc995581611b593b70e78c0b0776984fe60c236`)
and its paired UL source. The DL-derived phase was 8,950,000,000 ns, matching
the external reference detector, and the paired pool had two eligible 11 s
windows. Converting the DL 38-49 s window produced 247,854 configured rows:

| Corridor | t_max ns | delta ns | Q allowance ns | Extension ns | Horizon ns |
| --- | --- | --- | --- | --- | --- |
| A | 114904000 | 0 | 50000000 | 164904000 | 11164904000 |
| B | 176104000 | 0 | 50000000 | 226104000 | 11226104000 |

Every latency equaled its native interval latency plus the exact registered
offset. These are declared software-fixture drain budgets, not a universal Q
setting or evidence that arbitrary actual TX stalls fit within the horizon.

Run `scripts/check.sh` for policy, synthetic Python tests, formatting, Clippy
and all Rust tests. The pre-push hook runs that same script. No hook bypass,
history rewrite, hardware test or precision/throughput claim is part of this
delivery. Final integration review remains separate from these software checks.
