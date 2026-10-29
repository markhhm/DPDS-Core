<!-- SPDX-License-Identifier: Apache-2.0 -->
# M2 independent oracle / reporter

2026-10-29. **Preparatory software implementation; M2 is not complete.**
Observer integration is awaiting the in-progress HNG-OBSERVER specification.
No testbed, physical NIC, hardware G2, precision or throughput result is claimed.

## Reviewable changes

- Core `hng/m2-obs`: `504d77b` adds a JSONL adapter around the actual scheduler.
  It contains no reference-model formulas and leaves scheduling behavior alone.
- Host `hng/m2-obs`: `9ca159d` adds an independent streaming Python Fraction
  reference and million-event differential comparison; `22368d9` adds the
  reporter, CLI, fault tests, Stage 1-compatible probe codec, docs and Meson
  integration. Review corrections are host `20433e1` (final implementation tip).
  Its submodule is exactly core `504d77b`.
- M0 dependencies remain core `e2d90a2`, host `450d6e4` on `hng/m0-hooks`.

The exact provisional formats and invocation commands are host
`tools/g2/README.md`. Scope: complete per-packet ingress decisions; exact finite
Q and fractional wire service; independent counter selector; normalized trace
loss/clamp/outage/terminal semantics; full receiver sequence checks; separately
hash-sampled timing; all four timing/order/storage gates; CI and clock bounds;
terminal drop conservation; Q bytes and remaining drain time; independently
predicted late-collision serialization and residual diagnostics.

The reference streams decisions and retains bounded finite-Q ledgers. The
reporter currently joins expected/receiver records in memory. It is explicitly
software-only and cannot certify hardware-length evidence or hardware clocks.

## Stage 1 compatibility

Source was fetched read-only after the source maintainer's push:
`the Stage 1 harness de035a63f5d0135ee70b0bceb27d5051897b85a1` (36d54a2 + de035a6),
**pending second review**. Actually read: `C2/docs/vm126-target-check.md`,
`C2/scripts/vm126_target_check.py`, `C2/harness/target_rate_check.py`,
`C2/harness/target_rate_metrics.py`, and `C2/scripts/target_rate_remote.py`.

The reporter preserves linear point quantiles, flat CSV metric names, per-row
evidence-invalid precedence and overall valid-FAIL precedence. Detailed JSON
uses INCONCLUSIVE for insufficient measurement evidence, mapped to INVALID in
CSV/exit 2. Optional `--out NEW_DIRECTORY` writes report.json, results.csv and
summary.txt; it refuses reuse. Probe codec preserves network-order uint64 seq,
magic 0x4c30504b, one 0/1 late flag and 87 zero bytes (100-byte UDP payload).
VM126-specific target-load, app-health and steal requirements are outside P1's
four gates; their absence is not manufactured evidence of passing VM126.

## Verification already run

| Check | Result |
| --- | --- |
| core fmt / clippy all-targets with -D warnings | exit 0 |
| core `cargo test --locked` | 42 passed, 0 failed |
| host `meson compile -C build` | exit 0, matching core adapter/library built |
| host `meson test -C build --print-errorlogs` | 10 passed, 0 failed |
| Python tests inside Meson | 36 passed, including real core comparison |
| Differential | 1,048,576 ingress events, 32 cases, 0 mismatches |

Differential outcomes: accepted 409,140; Q drop 385,836; trace loss 252,424;
trace outage 1,176. Every admission/Q occupancy and every survivor's
selection/service/deadline/release/clamp/length fields and global release order
were compared. Output SHA-256:
`02da0dbf4175df39c833d118c021cc18f13d73e818bc021b645d680dde175929`.

Fault-injection CLI results with saved report.json/results.csv/summary.txt:

| Injection | Result |
| --- | --- |
| none | software-only PASS, g2_qualified=false |
| ordinary pair swapped | order FAIL |
| one delay-storage loss | storage FAIL |
| 5% of packets +0.2 ms from zero baseline | 0.2 ms detected, p99 gate PASS |
| same subset +0.6 ms (or +0.2 over registered +0.4) | p99 FAIL |
| observer misses 1% | INCONCLUSIVE, CSV INVALID/exit 2 |

Fixtures deliberately choose identities in the uniform timing sample so gate
checks are deterministic. They do not demonstrate unbiased hardware sampling.
Additional tests cover unsampled reordering, missing samples/end markers,
duplicate/unmatched identities, changed flags, missing/negative counters,
clock uncertainty, strict JSON, fractional Q equality and normalized trace hash.
RED runs are saved before implementations. A harness regression verifies that
Python -O cannot disable comparison failures.

Detailed raw logs and private evidence locations are retained in the research
records. They are intentionally not part of this public repository.

## Pending work / differences from the task

The task owner corrected the 0.2 ms injection: detection is separate from a
threshold-crossing fixture; G2 thresholds stay 0.1 ms median / 0.5 ms p99.
Current tests reflect that correction rather than changing the acceptance gate.

HNG-OBSERVER is being written on hng/design; latest fetched `f1d2050` contains
MACHINE-REQUEST and HNG-DESIGN only. Final schema adapters, classifier-reject
physical ordinal treatment, bounded nonblocking observer capture, observer
overflow counters, common-clock pairing, port TX attempt/gate checking,
pool-occupancy time series and pcap/net_ring full observation acceptance remain.
These are not claimed done by the replay adapter/null-PMD/Meson passes.

The design owner received current schema/CLI commitments. Two schema questions
were sent: how rejects retain physical ingress ordinals, and exact TX attempt
time/offered IDs/accepted prefix/gate/attempt-order fields. Neither blocks
independent oracle work. Full M2 milestone/lab-notebook completion will follow
the final specification and observer acceptance, not this preparatory report.

## Independent review and fixes

An independent reviewer found three Important false PASS paths: expected
rows were not sealed to the reference summary, invalid/unsupported storage
and envelope settings were accepted, and impossible negative OWD could pass
as small timing error. It also found that Q-drop rows omitted the sent
late-selection flag. Each got a RED regression, then a minimal shared fix.
The reviewer rechecked those reproductions and confirmed the corrected outcomes.
Its final remaining trace-metadata type issue (truthy arrays/bools accepted as
provenance/epoch/scale) also received RED cases and strict boundary validation.
No Critical findings were reported. The review explicitly did not sign off
hardware or the unfinished observer scope.

Expected output now has a canonical expected-row SHA-256 seal; the reporter
verifies kind/schema/run/count/hash fields before evaluation. Complete observer
source seals remain a separate final-spec step. Both reference and reporter
validate registered config fields/types/envelopes/storage bounds independently
from Rust. Every available matched negative-OWD pair is invalidated beyond
clock uncertainty before sample selection. Q drops retain the selector's result.

The first review-fix Meson run failed `g2-software` only in
`test_optimized_python_cannot_disable_mismatch_detection`: its deliberately
short input had previously requested less storage than its declared profile.
The fixture now declares adequate storage even for short corrupt-output runs;
the final corrected run passed. The failure log is preserved as
the first review-fix test log, not hidden or overwritten.

The M2 spec owner clarified modeled trace-hold timing: expected OWD uses
`release_ns-arrival_ns`, including modeled hold. Fixed mode retains the original
deadline. Collision serialization remains a separate diagnostic; it is not
subtracted from gate error. A new trace-hold fixture covers this distinction.
Engine arrival versus external ingress, raw clock calibration, per-line gates
and complete stream seals will follow the committed specification.

The project maintainer owns integration; no main/hng/core pushes.


## Integration and streaming follow-up

The measurement branch now merges the integrated core quality gates without
rebasing. License headers and public documentation have been checked against
the repository's policy scanner; private machine/session names and evidence
paths are excluded from this report. Existing upstream history is unchanged.

The host adds a strict event-stream validator (`20d712b`), covering header,
all-record sequence continuity, marker counts, final seals, canonical content
hashes and separate byte-level file hashes. A sequence gap alone is sufficient
to report incomplete observation; a drop counter is not required. Receiver
wire ordinal is separate from JSONL export ordering.

Online cell statistics and sampled retention are implemented in host `4ed39d8`.
Each line/direction/phase has separate main, ordinary/late main, late extra and
per-app extra groups. Main errors are retained within a registered bound for
exact quantiles; all groups have bounded signed/absolute histograms with exact
nonzero/min/max/overflow counts. The default retention policy is streaming plus
hash-selected main witnesses. Full retention requires an explicit software
fixture or bounded debug window; it is not the normal data path.

Mixed-population tests exercise 500,000 inputs on each of two lines and a
440,433-input timing-fault population with the default 1/1024 hash. A failing
LEO tail cannot be masked by cable traffic. The timing cases include 60% at
+0.2 ms (median FAIL), 5% at +0.2 ms (detected below the thresholds), and 5% at
+0.6 ms (p99 FAIL). These are online-statistics tests, not yet full virtual-PMD
M2 fault-injection acceptance. The existing integrated virtual-PMD suite passes
independently; it does not substitute for that remaining observation test.

The bounded-fixture reporter now uses explicit receive ordinals when supplied.
Complete duplicate delivery and delivery of model-intended drops fail the data
path; an identity absent from ingress is a structural invalidity. Missing
observation evidence remains inconclusive.

Host `f823b9e` adds exact rational clock-map and raw/corrected OWD validation.
Its software debug custody replay checks offer ordinals, terminal identity,
dispatch item ordinals, consumed prefixes, zero/partial/full TX acceptance and
cable/LEO gate bypass on a shared egress. These components pass 25 focused
tests. Full-history replay is explicitly limited to software full-mode fixtures;
it refuses normal streaming and physical scope. Database handles close after
replay and lookups. The complete host check and pre-push check both passed
16/16 tests with no skips.

Host `6019877` connects the reference CLI to sealed v1 input/output. Its output
has independent envelope ordinals and a final v1 summary containing source
hashes/counts, per-port end counts, per-line outcomes and Q-ledger resource
peaks. Invalid raw input preserves its ingress ordinal without consuming valid
line service; raw port/VLAN mapping is independently checked. An intact input
rejected by the host is not reclassified as an intended model loss. A missing
1% of records is detected through sequence continuity alone, without a supplied
drop counter; partial output cannot acquire a complete seal. The bounded draft
reporter still accepts the reference output during migration. Focused reference,
report and protocol checks passed 54 tests; the complete host check passed
16/16 with no skips.

Final manifest/report migration, complete online before/after joins, online
offer/terminal and shared-port custody reconciliation, plus actual virtual-PMD
observation faults remain open. The uncompleted host exporter work is preserved
outside the public repository until it can be tested against the streaming
contract. There is still no full M2 or physical G2 signoff.
