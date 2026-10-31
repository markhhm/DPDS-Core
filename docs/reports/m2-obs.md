<!-- SPDX-License-Identifier: Apache-2.0 -->
# M2 independent oracle / reporter

2026-10-29. **Preparatory software implementation; M2 is not complete.**
Observer integration is awaiting the in-progress HNG-OBSERVER specification.
No testbed, physical NIC, hardware G2, precision or throughput result is claimed.

## Reviewable changes

- Core `hng/m2-obs`: `44bceab` adds a JSONL adapter around the actual scheduler.
  It contains no reference-model formulas and leaves scheduling behavior alone.
- Host `hng/m2-obs`: `e238371` adds an independent streaming Python Fraction
  reference and million-event differential comparison; `488388c` adds the
  reporter, CLI, fault tests, Stage 1-compatible probe codec, docs and Meson
  integration. Review corrections are host `0554cad` (final implementation tip).
  Its submodule is exactly core `44bceab`.
- M0 dependencies remain core `ba628ef`, host `8a4a49f` on `hng/m0-hooks`.

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

HNG-OBSERVER is being written on hng/design; latest fetched `a549e65` contains
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

The host adds a strict event-stream validator (`04d4bf9`), covering header,
all-record sequence continuity, marker counts, final seals, canonical content
hashes and separate byte-level file hashes. A sequence gap alone is sufficient
to report incomplete observation; a drop counter is not required. Receiver
wire ordinal is separate from JSONL export ordering.

Online cell statistics and sampled retention are implemented in host `5cd64da`.
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

Host `e8bfa7b` adds exact rational clock-map and raw/corrected OWD validation.
Its software debug custody replay checks offer ordinals, terminal identity,
dispatch item ordinals, consumed prefixes, zero/partial/full TX acceptance and
cable/LEO gate bypass on a shared egress. These components pass 25 focused
tests. Full-history replay is explicitly limited to software full-mode fixtures;
it refuses normal streaming and physical scope. Database handles close after
replay and lookups. The complete host check and pre-push check both passed
16/16 tests with no skips.

Host `d806d68` connects the reference CLI to sealed v1 input/output. Its output
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

Host `d72f8ec` adds static v1 registration preflight. It checks logical
port/VLAN/queue mapping, immutable config/trace/calibration sources, required
cells, sampling targets and pool declarations, and bounded debug retention.
The planning vector is `N_plan(10)=440433`; the m=1 boundary is also checked
against exact binomial sums. Invalid types cannot bypass required sample
planning. Config parsing uses the same bytes whose hash was checked. The
preflight explicitly does not claim captured-stream integrity or data-path
gate evaluation. Focused checks passed 66 tests; the full host check passed
16/16 with no skips.

Host `85baaa7` normalizes typed counter snapshots, retaining explicit endpoints,
clock/generation metadata, descriptors and complete source hashes. Reset or
regression cannot become a zero drop; unknown required values remain null.
Observer accounting, queue high-water, software conservation and terminal
semantics are checked independently. The draft reporter accepts the normalized
view and preserves detailed INCONCLUSIVE with compatibility INVALID. Focused
checks passed 54 tests.

The first complete counter check passed 15/16 tests: the existing null smoke
test sent SIGINT before startup completed. A delayed real-DUT fixture reproduced
the fixed-sleep race. Host `d6b0e8d` waits for actual worker readiness and retains
the delay as a regression; the corrected check passed 16/16. Host `1db3168`
then merges M4 `b99aca3`, keeping its early signal-handler installation,
2/3/4/5-lcore and startup-SIGINT coverage, plus readiness before traffic.
The pinned core is merge `36b7aae`, containing M4 `a2a339c` and the independent
replay adapter. The merged full host check passed 25/25 with no skips; core
policy/trace tools/fmt/clippy/tests also passed. No hardware qualification ran.

The observation contract now contains a clearly marked proposal for a bounded,
volatile identity/outcome index. It must not persist full packet history or use
a disk fallback. An unrepresentable/retired identity increments `join_overflow`
and makes its affected cells INCONCLUSIVE; memory limits and their formula are
reported explicitly. Implementation and design-branch integration remain open.

Final report migration, complete online before/after joins, online
offer/terminal and shared-port custody reconciliation, plus actual virtual-PMD
observation faults remain open. The uncompleted host exporter work is preserved
outside the public repository until it can be tested against the streaming
contract. There is still no full M2 or physical G2 signoff.

## Gap ledger against the independent M2 review (2026-10-30)

Software-only throughout: nothing here is G2, accuracy or throughput evidence.
"Done" cites the host commit that carries it; "Open" says what is still missing.
Item numbers follow the review's "gaps to close" list. The Done column maps
items to commits by commit message and test names; only item 12 was re-run
end to end for this ledger, so other rows are claims to audit, not new evidence.

| # | Gap | Done | Open |
| --- | --- | --- | --- |
| 1 | Stream envelope and seal | `04d4bf9`, `d806d68` | none for software |
| 2 | Observer loss from seq gap, not counters | `04d4bf9`, `d806d68` | none for software |
| 3 | v1 manifest | `d72f8ec` | physical fields (offloads, clocks) are fixtures only |
| 4 | v1 ingress records, invalid inputs | `d806d68` | none for software |
| 5 | Per-identity offer and terminal checks | `e8bfa7b` (debug replay), `9952a4b` (live) | none for software |
| 6 | Receive order from `receive_seq` | `5cd64da` | none |
| 7 | Timestamp objects, before stream, raw/corrected OWD | `e8bfa7b`, `b482cbe` | physical timestamp adapters (M5) |
| 8 | Calibration | `e8bfa7b` | real apparatus calibration (M5) |
| 9 | Typed counter snapshots | `85baaa7` | NIC-specific descriptors |
| 10 | Per-cell verdicts and inventory | `5cd64da`, `b482cbe` | none |
| 11 | Signed errors, strata, windows | `5cd64da`, `b482cbe` | none |
| 12 | Shared-egress attempt/dispatch audit | `e8bfa7b`, `53f5061`, live in `9952a4b`; `e916c9e` adds the end-to-end shared-egress fixture | a held trace line next to a cable line is covered by the replay tests, not by a sealed-bundle fixture |
| 13 | Busy-period collision diagnostic | `b482cbe` | none |
| 14 | `hng-g2-report-v1` and CSV | `a058f49`, `b482cbe` | CSV column audit against the spec |
| 15 | Receiver anomaly classes | `5cd64da`, `a058f49` | none |
| 16 | Bounded streaming join | `a058f49`, `e44eafd` | none for software |
| 17 | Wider differential coverage | `4cd8a84`, `962e12f`, core `843c14a` | none |
| 18 | Fault injection through the real PMD path | online-statistics fixtures only | pcap/net_ring DUT injection with mixed population |
| 19 | Native exporter | `eb75f37`, `9952a4b`, `29f7d23`; `96c5fb3` rejects 7-lcore capture before startup | 7-lcore observation remains unsupported |
| 20 | Open-source items | SPDX and path scrub in earlier commits; host `0a024dd` installs pinned-core trace requirements with Python 3.13; `66fe088` requires the trace suite to run with zero skips | event/report format document; CI wiring of the missing PMD fault-injection tests |

Added by the 2026-10-30 integration: observation in the 7-lcore layout is not
supported (the host refuses it at startup), listed after item 12.

## Review fixes and four-CPU timing (2026-10-30)

Software-only. These changes address the review of host `ae284f2`; they do not
provide accuracy, throughput, physical G2 or complete M2 qualification. The gap
ledger above was published as core `54ea201` after its full check suite passed.

| Item | Status | Host commit and evidence |
| --- | --- | --- |
| Real host-core merge | Done | `0330d83` has two parents and includes `5e5bca1` |
| Recovered line at the final clock read | Done | `f5db598`; `recovery_at_final_clock_read` first failed on the observed release order 200 then 100; now sends 100 then 200 |
| Expired retry rejoining a merged burst | Done in the base branch | Reproduced on `hng/core` at `96d2075`; `ce12ef2` validates replacement heads; present in current host core `0a024dd` and merged into M2 |
| Reject 7-lcore capture before producers | Done | `96c5fb3`; `observer-export` first showed pools and a launched layout, then passed with an empty capture and rejection before pool creation; disabled 7-lcore forwarding also passes |

The retry correction is now included in host `hng/core` at `0a024dd`.
The earlier PR number belongs to the previous repository history and is not a
pending integration requirement. M2 also retains the `96d2075` affinity-call fix.

Each of the three regressions was run failing before its production fix. The
retry branch's normal pre-push check passed all 30 Meson tests with zero skips.
The final M2 host `96c5fb3` passed the normal pre-push check: all 35 Meson tests,
zero skips. Full checks and pushes were serialized with the shared test lock.

### Four-CPU software test timing

On host commit `96c5fb3`, with affinity restricted to logical CPUs `0,1,2,3`:

```sh
flock /tmp/hng-tests.lock taskset -c 0,1,2,3 /usr/bin/time \
  -f 'wall_seconds=%e user_seconds=%U sys_seconds=%S exit=%x' \
  meson test -C build --no-rebuild g2-software --print-errorlogs --verbose
```

One run passed all 179 Python tests in 106.961 s. Meson recorded 107.04 s;
the outer timer recorded 107.25 s wall, 104.99 s user and 1.35 s system time,
exit 0. Lock waiting and compilation are excluded. The unchanged Meson timeout
is 180 s. Differential coverage included 1,048,576 events across 32 cases,
with zero mismatches. This is a single software-only run on a shared machine,
not a CI timing guarantee or physical performance evidence.

### Historical remote CI limitation, resolved after realignment

Before realignment, host CI failed before Meson with
`NumPy is missing or cannot be imported by python3.` Host core `0a024dd` now
installs the pinned core's requirements with Python 3.13. The M2 integration
at `fed7ea0` passed remote CI, so the missing dependency is no longer open.
No CI check or hook was bypassed.

The held-trace-plus-cable sealed-bundle fixture (item 12), mixed-population
faults through actual PMDs (item 18), CSV/spec audit and format documentation
remain open. Other ledger rows are prior implementation claims, not a new
independent audit. Further M2 integration requires another independent review.

## Realigned integration and remaining work (2026-10-31)

All commit references in this update use the rewritten history. The earlier
four-CPU timing is historical software-only evidence, not a new measurement on
these integration commits.

| Item | Done (commit) | Open |
| --- | --- | --- |
| Merge current core into M2 | Core `57af73e` merges `5805604`; full local check and [remote CI](https://github.com/markhhm/DPDS-Core/actions/runs/41108386899) pass | none for this integration |
| Merge current host and align the pin | Host `fed7ea0` merges `0a024dd` and pins core `57af73e`; local Meson 35/35 with zero skips and [remote CI](https://github.com/markhhm/DPDS/actions/runs/41109396603) pass | none for code integration; see the host submodule pin for the final documentation revision |
| Required trace execution | Host `66fe088`; actual `test_trace_tools` suite: 29 passed, zero skips; the guard rejects empty, skipped and failed suites | none in the required-suite guard; optional external-source replay remains separate |
| Shared-egress trace coverage, review item 12 | Existing checker and fixed-line sealed-bundle fixture `e916c9e` | held-trace/cable sealed-bundle tests; precise FAIL versus INCONCLUSIVE assertions |
| Seven-lcore observation | Startup rejection `96c5fb3` remains active | approved per-writer SPSC/native v2 design; specification §5.3 must be updated before support implementation; retain v1 producer-5 rejection tests |
| Remaining report and apparatus work | Existing runtime v1 format and Done/Open ledger retained | CSV/spec audit, format documentation, real-PMD mixed-population faults, physical timestamps/calibration and independent review |

The zero-skip requirement applies to the required synthetic trace-tool suite.
The separate optional external-source replay test may still report a skip when
its input is not supplied; that skip is not counted as trace-suite execution.
Seven-lcore support and changes to the runtime v1 report format are not part of
this integration.
