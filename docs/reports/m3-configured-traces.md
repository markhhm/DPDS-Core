<!-- SPDX-License-Identifier: Apache-2.0 -->

# M3 configured trace conversion and comparison

Date: 2026-10-30. Scope: independent Garcia conversion, provenance, configured
row comparison and late-run diagnostics. Source outage reconstruction, terminal
HOLD coverage and engine FIFO/release equivalence remain deferred. This is not
hardware precision or throughput evidence.

This is the initial configured-conversion validation snapshot. The subsequent
[registered-semantics implementation](m3-registered-traces.md) adds canonical
eligibility, exact corridors, finite terminal HOLD and synthetic outages.

## Integration and implementation

Integrated core commit `4655e81768f189970b0227a6782c7d767ab28c55` with a merge,
preserving the work branch's history. The existing hook is installed, and the
shared check script now runs synthetic Python tests as well as policy, Rust
formatting, Clippy and Rust tests. CI uses that same script with NumPy installed.
No Rust or DPDK dependency changed.

The [tool guide](../TRACE-TOOLS.md) documents CLI parameters, the unchanged
`hng-trace-v1` schema, versioned recipe, source/normalized hashes and output
reports. Offset is a required caller parameter, not a corridor default. Input
locations and output locations are caller-supplied. No reference source code
was copied or imported into the implementation.

## Independent configured-row checks

The reference was loaded externally from converter snapshot
`69daa0c5cc48d80223ea42563c2dd8603c3699c2`; it is not bundled or required by CI.
Both tools used the same raw ZIP bytes, source window, seed and 0.05 ms adaptive
rows. Native reference LATENCY and accumulated KEEP durations were compared
against every normalized row and final end. There was no row tolerance.

| Source | Start / duration s | Rows, seed 1 | Rows, seed 11 | Exact boundaries and latency |
| --- | --- | --- | --- | --- |
| `2025-04-20-17-06-16__dl.csv.zip` | 38 / 11 | 247854 | 248005 | PASS, both seeds |
| `2025-04-20-17-06-16__ul.csv.zip` | 38 / 11 | 245702 | 245690 | PASS, both seeds |
| `2025-04-13-12-06-16__dl.csv.zip` | 8 / 11 | 246976 | 246465 | PASS, both seeds |
| `2025-04-12-16-36-16__dl.csv.zip` | 35 / 11 | 248063 | 248031 | PASS, both seeds |
| `2025-04-12-16-36-16__ul.csv.zip` | 35 / 11 | 243453 | 243467 | PASS, both seeds |

Source ZIP SHA-256 values are listed in the [inventory](m3-trace-inventory.md).
No measured ZIP, sample sequence, reference CSV or normalized trace is committed.
The synthetic reference CSV has three rows and checks the separate comparison
CLI without downloading data.

## Late-run diagnostics

The source diagnostic is the fixed 15 s median baseline anchored to the first
source send; late means excess strictly greater than 20 ms. Runs break at
sequence gaps or send gaps greater than three times the full-source median
cadence. Converted values are evaluated at identical received source samples
against the same baseline. Full histograms, spans and connected-pair counts
are retained in runtime JSON reports; no missing sample becomes a late packet.

| Source | Original / configured late samples | Original / configured runs | Changed flags | Original / configured P(next late given late), % |
| --- | --- | --- | --- | --- |
| `2025-04-20-17-06-16__dl.csv.zip` | 271 / 271 | 31 / 31 | 2 | 91.6031 / 91.6031 |
| `2025-04-20-17-06-16__ul.csv.zip` | 801 / 800 | 22 / 22 | 1 | 97.7415 / 97.7387 |
| `2025-04-13-12-06-16__dl.csv.zip` | 1057 / 1059 | 54 / 54 | 4 | 95.4329 / 95.4416 |
| `2025-04-12-16-36-16__dl.csv.zip` | 22 / 22 | 1 / 1 | 0 | 95.4545 / 95.4545 |
| `2025-04-12-16-36-16__ul.csv.zip` | 636 / 636 | 17 / 17 | 0 | 98.5669 / 98.5669 |

These five selected windows are not the full-corpus estimate. Seeds 1 and 11
gave the same diagnostic counts shown here. Interpolation, midpoint sampling
and quantization changed 0-4 flags per window despite exact HNG/reference rows.
Equal run counts do not guarantee identical histograms. No run-length tolerance
has been registered, so these comparisons are descriptive, not a new PASS gate.
They show the observed correlated runs rather than replacing them with IID loss
or late selection.

## Verification boundary

Synthetic tests cover source integrity and validation, missing samples, trend
steps, full-source RNG state, rounded boundaries and latency, explicit offsets,
late threshold ties, crop baselines, empty diagnostic windows, run breaks,
row/coverage mismatch, unsupported fields, duplicate JSON keys and overwrite
protection. See `tests/test_trace_tools.py`; all data fixtures are synthetic.

The fail-closed measurement-gap restriction and error terminal policy are
implementation limits while source-gap/HOLD decisions remain unresolved. No
canonical pool, phase fallback, outage reconstruction or source-to-deadline
equivalence is registered by this delivery. See the inventory's open questions.

Existing Apache-2.0 LICENSE and attribution are unchanged; no inherited NOTICE
was removed. New code and documents have SPDX headers. Original data licenses
remain separate from code licenses. Public documentation uses portable examples
and contains no deployment identifiers or absolute personal/shared paths.
