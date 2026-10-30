<!-- SPDX-License-Identifier: Apache-2.0 -->

# M3 trace pipeline inventory

Date: 2026-10-29. Scope: task step 1 only. Status: inventory submitted for lead
review; source conversion, cross-engine comparison and engine replay are pending.
This report does not register new replay semantics or claim hardware validation.

Historical inventory: the questions below were subsequently resolved by the
[registered M3 semantics](../TRACE-SEMANTICS.md). Their implementation and
updated verification are recorded in [the follow-up report](m3-registered-traces.md).

## Findings that need a decision first

The existing Step 2 converter and the P1 engine do not currently implement the
same packet release rule. Step 2 changes route IDs at delay steps and preserves
FIFO within each route. P1 preserves FIFO across all steps. A shared configured
delay sequence therefore does not establish identical packet deadlines.

The corridor formulas are registered in the research plan, but the inspected
Step 2 generator exports native Garcia delays without applying those formulas.
There is no identified corridor-adjusted golden artifact in the inspected code.
Garcia missing sequence numbers are explicitly **not** primary-profile loss.
Missing samples also do not identify outages. See the decision questions below.

## Evidence versions

Paths and line numbers below refer to these exact source versions. Research
records are project evidence, not runtime dependencies of the public engine.

| ID | Source inspected |
| --- | --- |
| C | DPDS-Core `e9dc33514f3526a9f588185ca1bb6fe50bcc0206`, the base of this branch. [Trace API](../HNG-API.md#trace-files), [design section 5a](../HNG-DESIGN.md#5a-vlan-lines-and-trace-replay-1955-addition), `src/hng_engine.rs`, `src/hng_trace.rs`. |
| H | Reference converter snapshot `69daa0c5cc48d80223ea42563c2dd8603c3699c2`. The replay model, single-segment generator and loader are also unchanged in main snapshot `12e76f357532f89035e3a470e7b0eabfd3406636`; the experimental splice additions are not in that main version. |
| T | [TheaterQ hng `bc57c33ef2890cdaffa23f7a5c069ae0699c30b2`](https://github.com/markhhm/TheaterQ/tree/bc57c33ef2890cdaffa23f7a5c069ae0699c30b2), inspected source only. H's `C2/leo-emu/validation/README.md:94-103` separately records the earlier validation build at `55f0b5e`; this inventory did not inspect a running module. |
| K | Project research records (not public), snapshot `7402fe63af1a9e9b49b0e85ef4b93acda2da9f1d`: the Step 2 routing-demo design plan (abbreviated **Step 2** below) and three review reports from 2026-10-29 (DPOP, splice clamp, and the P0 100G emulator review). |
| O | Project research records (not public), snapshot `4b07aeace4bc0f70da354a1640194943626d4572`: the P1 100G roadmap, sections OS1-OS6 and M3; rechecked against main snapshot `7a1d828` after the session resumed. |
| G | Garcia dataset [Zenodo record 16275284](https://zenodo.org/records/16275284), its [metadata API](https://zenodo.org/api/records/16275284), and the locally supplied author `README.txt`, lines 1-12. README SHA-256: `a2f0ebc15ae8b81f299e86b576a0536c46f92e88f6c97c0c9080091bfcae571e`. |
| L | LENS `75d9befd4da77c25b3e7181f200ddda232873bbb`: [OneWeb README](https://github.com/clarkzjw/LENS/blob/75d9befd4da77c25b3e7181f200ddda232873bbb/oneweb/README.md), [main README](https://github.com/clarkzjw/LENS/blob/75d9befd4da77c25b3e7181f200ddda232873bbb/README.md#license), and [LICENSE](https://github.com/clarkzjw/LENS/blob/75d9befd4da77c25b3e7181f200ddda232873bbb/LICENSE); OneWeb [Zenodo record 15786375](https://zenodo.org/records/15786375) and its [metadata API](https://zenodo.org/api/records/15786375). |
| D | Choi and Paek dataset DOI `10.21227/bc71-sg44`: [DataCite metadata](https://api.datacite.org/dois/10.21227/bc71-sg44), [IEEE DataPort record](https://ieee-dataport.org/documents/leo-access-link-traces-measurements-commercial-starlink-terminal), and the associated paper DOI `10.1109/JIOT.2026.3743200`, whose data-availability statement points to that dataset. |

## Existing pipeline and reuse boundary

| Component | Existing responsibility and evidence |
| --- | --- |
| Source loader | H `C2/harness/owd_replay.py:162-182`: read one zipped CSV, stable-sort by sequence number, interpret send timestamps as ns from run start and OWD as ns. This is already OWD, not RTT/2. |
| Continuous model | H `C2/harness/owd_replay.py:55-159`: interpolation, seeded steps, local trend, extrapolation floor, loss cells and route assignment. |
| TheaterQ exporter | H `C2/harness/owd_replay.py:242-324`: adaptive sample-and-hold rows in extended CSV format. |
| Paired single-segment generator | H `C2/leo-emu/g2/g2_traces.py:26-63`: select a paired run and common offset, export both directions and record row hashes and parameters. |
| Existing validation input builder | H `C2/leo-emu/bench/garcia_l0.py:19-34`: build one source window and a probe schedule. This is a validation tool, not a source-independent P1 adapter. |
| Deployment consumer | H `C2/leo-emu/g2/leo-up.sh:36-51`: load extended rows with HOLD and start both directions in one synchronization group. `C3/hng_select/G2-RUNBOOK.md:43-44` delegates replay to the C2 tools; conversion is in C2. These deployment tools are not suitable public-core dependencies. |
| Experimental FIFO analysis | H `C2/harness/owd_splice.py:94-109` and `C2/leo-emu/g2/validate_splice_clamp.py:62-92`: clamp sampled probe deadlines for offline splice analysis. The function explicitly disclaims a service-rate model and TheaterQ export. It is not an already integrated FIFO replay path. |

No converter or deployment code was copied. No top-level license was found in H
at the inspected commit. Confirm code reuse licensing before transplanting its
implementation; the dataset license is a separate matter.

## Registered source and replay rules

### Segment selection, epoch and direction

Step 2 lines 145, 154 and DECISION 05:15 at line 200 require one natural segment
with HOLD for experiments of at most 45 s. M3 does not splice or loop. The 45 s
limit is an experiment rule; it does not guarantee that every selected starting
slot has 45 s of source coverage.

H `g2_traces.py:26-55` enumerates sorted stamps with both directions, derives
reconfiguration phase from DL, leaves a 0.5 s margin at the start and end, and
uses the smaller of the two directions' end times. Candidate starts are
`S0 + 15*k`; only windows that fit are eligible. A repetition seed selects a
candidate uniformly with Python's `random.Random`. A fixed stamp instead selects
its first candidate. The model's switch seed defaults to 1 and is distinct from
the candidate-selection seed. Both routing policies must reuse the same choice.

H `owd_replay.py:185-202` derives phase from spikes above median + 15 ms, using
300 phase bins and neighboring-bin counts. It aligns to wall-clock seconds
12, 27, 42 and 57. If phase cannot be detected, `g2_traces.py:35` falls back to
the margin; it does not fail. Filename timestamps are approximate (G README
line 5), so filename time alone is not the registered phase estimator.

Step 2 line 109 and H `g2_traces.py:4-8` map **tenant to remote = UL**, and
**remote to tenant = DL**. Both use the same source offset S and replay epoch.
P1 port numbers are configuration values, not intrinsic UL/DL identities.
The adapter must preserve this role mapping and the original source offset.

### Corridor OWD conversion

The shared formula is `OWD_LEO = OWD_Garcia - 11 ms + 4 ms + d_PoP`.
Evidence: K Step 2 lines 207, 210-212 (DECISION 11:00, R&E correction, 12:05
candidate and DECISION 12:30); K `report-rne.md:48-58` supplies the arithmetic.

| Scenario | Registered parameters |
| --- | --- |
| Corridor A | Cable OWD 65 ms. Tokyo-to-remote component 52.6 ms gives a +45.6 ms offset, summarized in the registration as approximately +46 ms. |
| Corridor B | Cable OWD assumed 100 ms, with 80/100/120 ms sensitivity values. Tokyo-to-remote component 113.8 ms gives +106.8 ms, summarized as approximately +107 ms. This is an assumed scenario; the alternate contemporary route is not the selected scenario. |

H `g2_traces.py:58-61` passes the source model directly to `write_theaterq`.
H `owd_replay.py:168,280,303-305` converts units and quantizes native OWD; it
does not subtract the baseline or add a corridor offset. Searching C2/C3
Python, shell, JSON and YAML files did not identify the corridor implementation.
That search does not rule out an uncommitted or separately stored artifact.

### Interpolation, steps and row rounding

The final registered step detector is deviation of at least 1 ms from the local
trend, not merely adjacent-sample difference. Estimate slope by least squares
over the previous 5 ms since the last step and clip it to +/-1 ms/ms. Draw a
switch uniformly between the two samples with the recorded seed. Both sides
follow the same trend; otherwise interpolate linearly. Across missing samples,
raise extrapolation to the minimum received delay in the surrounding 1 s window;
reject negative exported latency. Evidence: K Step 2 lines 117, 120 and 129;
H `owd_replay.py:58-153,288-290`.

The exporter splits sloping intervals adaptively with a 0.05 ms target, includes
switch and loss-cell boundaries, rounds absolute boundaries to integer
microseconds, and evaluates each rounded interval at its midpoint. LATENCY is
rounded to 1,000 ns using Python `round`; KEEP is in microseconds. Adjacent
identical rows merge. Each step gets another route ID, cycling through 1..255
with a reuse-distance guard. Evidence: H `owd_replay.py:242-309` and K Step 2
lines 126-129. Copying raw sample OWD values into P1 intervals would bypass
these rules and would not reproduce the existing TheaterQ row sequence.

### Loss, measurement gaps and outages

Garcia missing sequence numbers may reflect failed sender timestamp reads
(G README line 8; Zenodo description). The later Step 2 decision at line 114
supersedes the earlier missing-as-loss proposal at line 107. H
`owd_replay.py:162-182` leaves all primary-profile `lost` flags false and counts
missing samples separately. `missing_as_loss=True` inserts missing send times
by sequence interpolation and marks them lost, solely for an upper-bound
sensitivity profile. This does not establish that physical loss was zero.

For explicitly lost samples, H `owd_replay.py:70-74,125-153,272-280` uses
midpoint-to-midpoint time cells and exports LOSS 0 or the all-loss u32 value.
Received-sample separation greater than 0.5 s is classified as a measurement
gap, with loss suppressed inside it and `valid_s=False`. However, row generation
still evaluates interpolated delay there and does not reject a window using
`valid_s`. That is an observed implementation behavior, not a registered outage.

No source-to-outage mapping, outage threshold, or Step 2 drop-versus-hold
decision was identified in the inspected registration and conversion path.
The extended exporter has no explicit outage field. P1's `outage_action` must
therefore not be guessed from a missing sample or a long reply interval.

## P1 input contract and compatibility gaps

C `docs/HNG-API.md:43-90`, `src/hng_engine.rs:40-105` and
`src/hng_trace.rs:19-65` require a hashed `hng-trace-v1` document: source ID,
direction, source SHA-256, conversion recipe, `units=ns_owd`, epoch zero,
time scale one, splice policy, finite end, and ordered rows starting at zero.
Rows carry integer `at_ns`, integer `t_ns`, decimal-string loss and an explicit
outage flag. Unknown fields fail parsing. The engine validates direction as
nonempty metadata; deployment configuration still has to map the proper source
direction onto the proper line.

The intended single-segment policy is `natural_hold`. P1 trace-end
`hold_last_sample` requires a finite horizon beyond the original end. This is
different from outage action `hold`; the latter postpones release until recovery.
HOLD must cover service, propagation and drain, not only the offered-input
duration. Evidence: C `docs/HNG-API.md:66-85`, `docs/HNG-DESIGN.md:483-497`.

| Difference | Evidence and consequence |
| --- | --- |
| FIFO scope | H `owd_replay.py:205-215,291-305` and T `README.md:59` allow cross-route overtaking. C `HNG-DESIGN.md:382-408` clamps across every step and empty queue. H's existing test at `C2/tests/test_owd_replay.py:209-222` explicitly expects cross-route overtakes. Identical configured delays cannot imply identical realized deadlines. |
| Delay lookup time | T `theaterq_lkm/sch_theaterq.c:1203-1205,1246-1254` reads the active row at dequeue wall time and adds service duration. C `HNG-API.md:69-71` reads at rounded virtual service completion. These can select different rows around a transition, particularly under backlog. The no-backlog helper is not proof of equivalence under stress. |
| Loss position | T `sch_theaterq.c:1028-1043` applies trace loss at enqueue before service. C `HNG-DESIGN.md:431-441` applies loss at service completion after finite-Q admission and retains its service/byte accounting. A lossy stress run can differ even with the same loss intervals. |
| Loss granularity | Existing H exports deterministic all/none loss cells. C `HNG-API.md:87-90` supports interval probability, not packet-identity masks. A probe loss mask must not silently become a loss mask for unrelated packets. |
| Source provenance | H `g2_traces.py:56-63` hashes generated rows, but does not include original ZIP hashes or a corridor recipe. P1 requires both source provenance and normalized-file integrity. |

## Available data and licensing

The inspected reorder collection contains 757 ZIPs: 285 UL and 472 DL, with
240 paired stamps, 45 UL-only stamps and 232 DL-only stamps. The separately
supplied sample collection contains 10 ZIPs plus the author README. The
published figure-1 DL file is in the sample collection, while its UL counterpart
is in the reorder collection. A paired pool must be selected from actual
available pairs, not inferred from the total file count.

Five ZIPs passed CRC, single-member/header checks and the existing Garcia
loader. Their primary loss arrays were all false. Full-corpus CRC/hash validation
was not performed. These SHA-256 values identify original ZIP bytes:

| Source filename | SHA-256 |
| --- | --- |
| `2025-04-20-17-06-16__dl.csv.zip` | `07647bd5b8d64525f5eaca40dcc995581611b593b70e78c0b0776984fe60c236` |
| `2025-04-20-17-06-16__ul.csv.zip` | `84b64ce53e2fd414e19af60e3eee74c9fdde2b3588cc2c1965acd7908467affb` |
| `2025-04-13-12-06-16__dl.csv.zip` | `1ae939baff5cf3f9a7014e3056ac816fe4922a6beb7671ec805e22c8ad272a5c` |
| `2025-04-12-16-36-16__dl.csv.zip` | `c5a8e95c2611bd1c58d4c2548a45c126737427e084077ff5464be70b2d3bbae1` |
| `2025-04-12-16-36-16__ul.csv.zip` | `2cd33eb23e5d373bbbc3fbd4565e4dc79521965d85221cd329c6087442d0eb9e` |

The existing loader reproduced 5,792 missing sequence numbers in 925 stretches
for `2025-04-13-12-06-16__dl.csv.zip`. The sensitivity option marked exactly
5,792 inserted samples lost. Private receipts retain source locations and
counts; no raw data is committed here.

### Original dataset licenses, checked on 2026-10-29

These are original-data licenses verified from publisher/depositor records,
not licenses inferred from the papers or the ability to download a file.

| Dataset | Primary evidence and access | License and distribution conditions |
| --- | --- | --- |
| Garcia Starlink OWD | G's official Zenodo API identifies `cc-by-4.0` for record 16275284. The checked ZIPs above identify the locally available subset. | **CC BY 4.0.** Retain attribution to the dataset, its source link and license; identify any conversion changes. Do not describe the data as Apache-2.0. |
| LENS OneWeb, Iowa | L's OneWeb README lines 5, 19 and 24 identifies the Iowa raw dataset and its March 2025 snapshot, which includes IRTT. Lines 63-65 explicitly license the OneWeb dataset as CC BY-SA 4.0. Zenodo API record 15786375 independently reports `cc-by-sa-4.0` and lists `202503.tar.zst` (26,609,747,785 bytes). Only metadata and documentation were fetched, not that archive. | **CC BY-SA 4.0.** Preserve attribution, source/license links and modification notices. Distributing adapted data must follow the ShareAlike terms; do not relabel a converted OneWeb trace as Apache-2.0. |
| Choi / Paek Starlink traces | D's DataCite `rightsList` explicitly identifies `cc-by-4.0` and links the license legal text. The DataPort record lists `dataset_release.zip` (91.69 GB) and requires an authenticated subscription to access it. The paper identifies the same dataset DOI. No login or raw-archive download was attempted. | **CC BY 4.0 according to the depositor's DOI metadata.** Preserve dataset attribution, source/license links and modification notices. The subscription download gate is distinct from the deposited license. Archive-internal notices and the exact local subset still need verification before using or redistributing archive members. |

The redistribution/adaptation conditions above follow the official Creative
Commons [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) and
[CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/) summaries.
The LENS main README separately licenses repository code as GPL-3.0 and the
Zenodo dataset files as CC BY-SA 4.0 (lines 600-604). Its code license is not a
license for the DPDS converter. No LENS code was copied or imported.

OneWeb replay still waits for the assigned feasibility assessment (K Step 2
line 208; O M3) and an identified source member/hash. The inspected local IRTT
download manifest points to the LENS Starlink collection; that does not establish
the availability of the selected Iowa OneWeb segment. Choi raw data exists in
the official release; absence of a Choi-named local file is not evidence that
only a paper exists. A LEO dataset inventory review in K (2026-10-29)
also records a seven-run Choi analysis. Its selected-source manifest and the
archive's `DOCUMENTATION.md` remain necessary before an adapter can be specified.
Packet-specific reordering remains outside this task.

This inventory adds no original or converted third-party data to the fork.
Future CI fixtures must be small synthetic traces. Public reproduction should
use user-supplied source paths, download/conversion instructions and verified
hashes, with the appropriate data-license and attribution notices.

## Questions for the research owner

1. **Equality target and FIFO policy:** Does M3 equality mean the configured
   interval-delay sequence or the realized per-packet delay sequence? If it is
   the latter, which registered change reconciles Step 2's per-route FIFO with
   P1's global clamp? The sampled offline FIFO analysis is not a deployed export.
2. **Corridor golden artifact and rounding:** Which Step 2 converter commit or
   hashed output applies the registered corridor offsets? Should the common
   recipe retain +45.6/+106.8 ms or use +46/+107 ms? The 0.4/0.2 ms difference
   is larger than the separate 1 microsecond row quantization.
3. **Clock and loss basis under load:** How should cross-engine comparisons
   handle dequeue-time versus service-completion delay lookup, and pre-service
   versus post-service loss? No adapter can make these engine choices identical
   merely by relabeling intervals.
4. **Measurement gaps and outages:** Should windows containing invalid
   measurement gaps be excluded, rejected, or explicitly reconstructed? What
   source evidence defines an actual outage, and does it drop or hold? Keep the
   already registered Garcia primary missing-not-loss rule intact.
5. **Phase fallback and terminal coverage:** Is the generator's no-phase fallback
   acceptable for the canonical pool, or must such runs be excluded? Register
   the finite HOLD horizon and admission/drain coverage for the chosen fixture.
6. **Other sources and code reuse:** Supply the selected OneWeb/Choi member
   manifests and hashes, Choi archive-internal notices, and the OneWeb feasibility
   result before those adapters. The original data licenses are now identified
   above; this does not identify the selected replay subsets. Confirm a license
   for any H code copied into the Apache-2.0 fork.

These are questions, not proposed defaults. Steps 2-4 remain pending lead review
and resolution of the applicable semantic questions.

## Verification and open-source acceptance

The existing model suite ran locally with NumPy 2.5.4 and pytest 9.2.0:

```sh
python -m pytest -q -p no:cacheprovider C2/tests/test_owd_replay.py
```

Result: **24 passed**. The attempted combined run adding
`C2/tests/test_owd_replay_garcia.py` failed during collection because SciPy was
absent (`owd_replay_check.py:13` imports it). That suite did not execute.
No dependency was installed and no existing code was changed to hide the error.
The separate source audit above used the existing loader without the SciPy
checker. Initial audit assumptions that all ZIPs were paired and that figure-1
DL was in the reorder collection were corrected after inspecting the files.

The exact commands, errors, metadata receipt and source audit are in the private
handoff logs. This documentation-only submission changes this report and its
README link. It adds no converter, interface, fixture or engine behavior.

| Gate | This submission and remaining milestone work |
| --- | --- |
| OS1 | The new report has an Apache-2.0 SPDX header. LICENSE is unchanged. No NOTICE exists in inspected base C; no inherited notice was removed. Garcia, OneWeb and Choi original data licenses are identified above. Exact-subset provenance, Choi archive notices and source-code reuse licensing remain prerequisites before those inputs are used or distributed. |
| OS2 | New repository text contains no deployment addresses, machine names, credentials or absolute local paths. Private evidence stays outside the repository. Future converter input/output locations must be parameters with portable examples. |
| OS3 | README links this inventory and states that conversion is pending. A converter quick start waits for an approved, implemented converter. |
| OS4 | No dependency or public interface changed; the Rust core remains independent of DPDK. The normalized trace contract already exists in HNG-API. Future CLI and provenance fields need public documentation. |
| OS5 | No tests or CI were added for this documentation-only inventory. Synthetic converter CI fixtures and equivalence/replay tests remain pending. Base C has no tracked CI, CHANGELOG or hook installer; M0 owns the shared CI/hook acceptance work. No hook was bypassed. |
| OS6 | Added repository text and commit message are English. No earlier M3 commit needs rewriting. |

M3 acceptance is **not complete**. Only task step 1 is delivered. No testbed was
contacted, and no software-only result is offered as precision or throughput
evidence.
