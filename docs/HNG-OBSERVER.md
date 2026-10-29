<!-- SPDX-License-Identifier: Apache-2.0 -->

# HNG M2 observation, independent oracle and G2 report contract

Version: `hng-observer-v1`. Date: 2026-10-29. Status: implementation specification, not hardware acceptance evidence.

This document makes [HNG-DESIGN.md](HNG-DESIGN.md) section 9 implementable and uses the primary/backup wiring in [MACHINE-REQUEST.md](MACHINE-REQUEST.md). It does not authorize hardware access. Functional work and fault injection run with virtual devices first; physical accuracy and throughput are qualified later.

## 1. Scope, sources and boundaries

The controlling project task is the M2 specification request and roadmap OS1–OS6/M2/M5 decisions. Source revisions actually inspected:

| Source | Revision | Used for |
|---|---|---|
| Engine contract | DPDS-Core `d5df0b7` | Exact C/Q, counter selection, monotone trace, frontier and order semantics |
| Core implementation/API | DPDS-Core `9531a95e15b13801e117d2a0441f8d26d8d16ee4` | Current line/configuration/trace support and custody boundary |
| Host | DPDS `5cd3b6b23307d39a7a0a4dc48c54520faa5cc17b` | Physical ingress ordinal, software arrival time, TX and resource counters |
| M2 draft | DPDS `22368d97f1c6ad0ad238a2ecc03358eababa50a1`, `tools/g2/` | Independent Fraction reference, CLI, sampling domain, report and probe compatibility |
| Stage 1 method | Stage 1 measurement repository at commit `de035a63f5d0135ee70b0bceb27d5051897b85a1` (not public; its location is recorded in the private deployment manifest) | Target-check document/runner, `target_rate_metrics.py`, `target_rate_check.py`, probe emitter; **pending second review** |

Normative terms: **MUST** is required for this contract; **proposal** or **estimate** is an engineering choice to register before a run. Vendor capabilities below cite primary sources; no hardware performance is asserted.

The implementation belongs in the host repository's `tools/g2/`, preserving the draft Python reference/report interfaces. Independent observer tools may use DPDK or Linux timestamp APIs; the **observer, oracle and reporter must neither import/link the Rust engine nor call it to compute expectations**. The host's event emitter remains DUT instrumentation; independent tools consume its records as evidence, not modeled answers. A differential-test harness may call the engine as the system under test, separately from the reference. DPDS-Core gains no DPDK dependency.

Use an exact Python `Fraction` model for correctness and offline analysis. For 100G, use lossless buffered capture plus offline replay, or an independently implemented streaming reference proven equivalent on the same fixtures. No claim is made that Python can process 16.2 million events/s. Do not throttle the DUT to a reference consumer's speed.

### Chosen approach and alternatives

The chosen boundary is a versioned event protocol shared by independent capture, reference and report tools. A production-deadline-only checker cannot detect a shared scheduling bug. Timing-only packet samples cannot reconstruct finite Q or prove full-flow order. Full pcap payload retention is optional and expensive; full ingress/receiver **identities and model inputs** are mandatory **inputs to the independent oracle**. By default the oracle consumes them as streams and does not persist them: it keeps online statistics, histograms and full records only for the 1/1024 main sample (section 10.1). Full per-packet persistence is a capped debug mode. These choices preserve the existing draft rather than adding a second model API.

## 2. Timestamp architecture and calibration

### 2.1 Primary and backup equipment

The primary scheme uses DUT + dual-port TG + passive observer. Two duplex optical taps supply four unaggregated 100G monitor streams. The observer has two CX-6 Dx `MCX623106AN-CDAT` adapters, each on Gen4 x16. Pair the forward ingress and egress monitor ports on **one card**; pair reverse ingress and egress on the other. Each OWD uses a verified same-clock pair. Do not subtract raw clocks between cards; separate directions do not require a global clock merely to calculate their individual OWDs. The vendor specifies this adapter's dual-100G and Gen4 x16 interfaces, not our resulting accuracy. [NIC specification][cx6]

```text
                         DUT
TG A ---- tap L ---- port 0 [cable / LEO] port 1 ---- tap R ---- TG B
            |                                            |
            +-- forward-before --> Observer card F RX0   |
            |                   Observer card F RX1 <----+ forward-after
            +-- reverse-after ---> Observer card R RX1   |
                                Observer card R RX0 <----+ reverse-before
```

Verify actual clock/counter identities, timestamp modes and monitor link state. RX-only optical monitoring must be supported by the chosen NIC/tap/optics combination; if the monitor PHY requires an incompatible return link, this equipment combination is not qualified. Never merge two 100G copies into one 100G monitor port. Timestamp-enabled capture must sustain the registered frame mix without drops.

Backup: one dual-port CX-6 Dx TG with hardware **TX-port** and opposite-port RX timestamps for the registered timing sample. NVIDIA documents a TX-port timestamp path for eligible UDP/PTP traffic on CX-6 Dx and later; older completion timestamps and software send times are not equivalent. This is a Linux driver capability, not a claim that the DPDK generator already exports it. Linux timestamp requests can be correlated by explicit IDs; completion delivery order is not wire order. Validate the selected generator/collector path, sample coverage and load capacity before use. [NVIDIA MLNX_EN 24.07 timestamping][nvidia-ts], [Linux timestamp APIs][linux-ts]

A calibrated mirror can replace passive taps only after demonstrating bounded additional queueing, no oversubscription and complete identities at the target load. This is an alternative observation transport, not the default bill of materials.

### 2.2 Three distinct times

Never overload `arrival_ns` with a different meaning:

- `arrival_ns`: the timestamp supplied to the engine before Q, in **engine clock E**, with its original ingress order. This is the independent model input. It includes any software RX observation delay in the current host.
- `ingress_ts_ns`, `receive_ns`: observed before/after DUT, mapped into **one observation clock O per direction**. Their difference measures physical OWD; they are not production-computed deadlines.
- `gate_ns`, `submit_before_ns`, `submit_after_ns`: actual host arbitration and TX-call times in E. They check release/gate behavior and bound software residence; successful TX submission is not wire departure.

`OWD_raw_i = receive_ns_i - ingress_ts_ns_i`. Retain raw hardware counters and `clock_id`/mapping references for every retained timing observation. A same-host TSC or NTP state alone does not establish two NIC clocks' relation. DPDK RX timestamp support and scheduled TX are separate capabilities. [DPDK 26.03 mlx5][mlx5]

Physical comparisons MUST NOT use `receive_ns-arrival_ns` unless a calibrated mapping explicitly makes their domains and observation boundaries identical. For fixed no-queue calibration, `expected_i=t+I_i*delta`. For stress, `expected_i=release_i-arrival_ns_i` from the independent model; for trace this includes the modeled outage hold and monotone clamp. Extra delay from software RX backlog, stalled TX and late-release collisions remains realization error.

For diagnostics only, map engine `release_i` to O and report external ingress-to-engine observation lag and engine-release-to-observed-egress lag. Gate decisions need E-to-trace mapping; measured OWD does not require the two directions to use the same O. Never adjust each packet's expected time from its measured lateness.

### 2.3 Calibration record and acceptance

One `calibration.json` (`schema="hng-g2-calibration-v1"`) contains `calibration_id`, `run`, source/firmware/driver versions, timestamp points and the following arrays:

| Field | Exact content |
|---|---|
| `clocks` | Objects `{clock_id, kind, unit, width_bits, device_token, mode}`; `kind` is `engine_monotonic`, `nic_rx`, `nic_tx_port` or `fake`; tokens are manifest-local, not machine addresses |
| `mappings` | `{mapping_id, raw_clock_id, target_clock_id, raw_origin, target_origin_ns, scale_num, scale_den, valid_from_raw, valid_to_raw, residual_bound_ns}`; integer rational slope, positive denominator; validity interval is half-open |
| `pairs` | `{pair_id, ingress_clock_id, egress_clock_id, target_clock_id, offset_ns, offset_bound_ns, drift_bound_ns, quantization_bound_ns, load_bound_ns, mapping_bound_ns, uncertainty_ns, evidence_sha256}` |
| `qualification` | `{scope, offered_bps, frame_profile_sha256, duration_ns, before_after_checked, counter_resets, observer_drops, uncertainty_pass}`; actual results, no presumed true defaults |

Map a raw timestamp by `target_origin_ns + round_even((raw-raw_origin)*scale_num/scale_den)` using exact integer arithmetic. Rounding error is included in the bound. Reject unknown mappings, backward/stepped clocks, reset/wrap without a registered unwrap rule, extrapolation beyond validity, and nonpositive scale. Missing information is not a zero offset.

Calibration procedure, performed later by an authorized operator:

1. Before the DUT run, bypass it and observe known packets in both paths with identical timestamp settings. Measure differential tap/receiver/cable offset, swap paths to expose asymmetry, and test all required frame sizes.
2. Repeat at 10G and 100G offered load, both directions simultaneously, with timestamping and logging enabled. Retain sample pairs, link/capture loss, queue occupancy and load-dependent residual distributions; an unloaded average is insufficient.
3. Measure pre/post drift and clock rate over at least the longest registered packet residence and run duration. Bound error between calibration anchors; periodically anchor if needed. A fitted line with no residual/drift bound is insufficient.
4. Freeze offsets/mappings before evaluating G2. Repeat the bypass/path check after the run. A step or out-of-bound drift invalidates affected cells; do not fit emulator results to make them pass.
5. Store a conservative bound on differential OWD error: sum offset, drift, quantization, load and mapping terms, avoiding double-counting shared-clock terms. **`uncertainty_ns <= 10000`** is required. Record the bound's empirical coverage and limitations; no vendor model number alone proves it.

An illustrative allocation is 2 µs offset + 2 µs drift + 1 µs quantization + 3 µs load + 2 µs mapping = 10 µs. This is a **budget proposal**, not measured performance. Offset correction applies only to known observation-path effects. Keep raw OWD and corrected OWD (`OWD=OWD_raw-offset_ns`) together. Never remove DUT zero-delay residence or its load tail. For the TX/RX backup, actual TX timestamps are required; bypass cannot calibrate away unknown per-packet software-to-wire waiting.

## 3. Run bundle, types and integrity

The canonical public interchange is UTF-8 JSON/JSONL with LF termination; `results.csv` and `summary.txt` are derived views. Every integer time is nonnegative **relative nanoseconds** unless explicitly named an offset/residual. Integers are parsed exactly, never through floating-point or JavaScript Number. Reject duplicate JSON keys, NaN/Infinity, booleans in integer fields, integer overflow, unknown required enum/schema versions and unknown fields outside an optional `extensions` object. Nullable values use JSON null, never fabricated zero.

Logical identifiers (`run`, `line`, `stream_id`, `clock_id`, `external_identity`, app labels) use printable ASCII, 1–128 bytes, no control characters. Ports are logical uint16 indices; VLAN is 1–4094; lengths are uint32; sequences/counts/ns are uint64 without wrap. Signed offsets use int64. `run` identifies one immutable condition/repeat; `condition_id` groups registered repeats. A line is already direction-specific, as in host `lines[]`.

Required bundle:

| File | Schema / contents |
|---|---|
| `manifest.json` | `hng-g2-manifest-v1`; registration, immutable configuration and stream inventory below |
| `engine.json`, normalized trace files | Existing `hng-v1` / `hng-trace-v1`; exact bytes hashed, independent parser |
| `ingress.jsonl` | Complete pre-Q host input and frontier/end events; may be partitioned by port. Default retention: streamed to the oracle; persisted only for main-sample identities and debug windows |
| `receiver.jsonl` | Full egress identities/order and sampled timing; external before-point identities in a separate stream when using taps. Default retention as for `ingress.jsonl` |
| `host.jsonl` | Terminal non-TX outcomes, arbiter/TX attempts, optional production model diagnostics. Default retention as for `ingress.jsonl`; terminal records of anomalies are kept as mismatch records |
| `online.json` | Default retention mode only: streaming-oracle output per required cell (exact counts, histograms, mismatch records, stream digests), section 10.1 |
| `counters.jsonl` | Start/end and periodic typed layer snapshots, including observer health |
| `expected.jsonl` | Independent reference decisions plus required summary; optional persistence when streamed |
| `calibration.json` | Required for physical scope; explicit fake-clock record for software scope |
| `report.json`, `results.csv`, `summary.txt` | Verdicts, complete reasons, metrics and hashes |

A "stream" in this table is a logical record sequence. In the default `retention.mode="stream"` the high-volume streams are pipes into the oracle, and the persisted files contain the header, the retained records (main sample, debug window, mismatches) and the seal; the seal still covers every record that was produced (section 3.2). In `retention.mode="full"` every record is persisted, as for software fixtures.

All paths are supplied by arguments or relative to an operator-supplied manifest. Public examples contain synthetic identities and relative paths only. Preserve Apache-2.0/upstream attribution; add SPDX to new source and documentation. JSON fixtures use an adjacent SPDX-annotated README or license manifest, not invalid JSON comments.

### 3.1 Manifest fields

Required keys: `schema`, `run`, `condition_id`, `scope` (`software-only` or `physical`), `producer_versions` (host/core/oracle/reporter/capture SHAs and dependency versions), `stage1_source_commit`, `stage1_source_review`, `config` (`path`, `sha256`), `traces` (path/hash/provenance), `ports` (logical mapping, link bps, RX/TX queue layout), `lines` (name, port/VLAN mapping, app labels), `timing_mode` (`calibration` or `stress`), `sampling`, `clocks`, `windows`, `required_cells`, `streams`, `capacity`, `identity_profile`, `offloads`, `calibration_path` and `retention`.

`windows` lists `{phase,start_ns,end_ns,clock_id}` with phases `fill`, `steady`, `transition`, `drain`; intervals are half-open. Assign timing samples by **ingress** time, then retain their late outputs through drain. Report transition/drain separately without deleting their failures or loss. `required_cells` enumerates `(condition_id,repeat,direction,line,phase)` before data collection; a declared idle diagnostic cell is explicitly nonqualifying, not silently omitted. Each required cell also registers its sampling mask, planned delivered survivors and, where needed, a pooled repeat set (section 7.2); `validate` rejects a registration whose planned main sample cannot reach the minimum, before data collection. V1 qualification supports the current two-port direction mapping: each egress has exactly one opposite ingress source (or the declared one-port software loop). Reject a many-ingress-to-one-egress topology until it has a versioned total-order key; two separate port ordinals cannot be tied by sequence alone.

`retention` is `{mode, debug_windows, mismatch_cap}`: `mode` is `stream` (default for physical scope) or `full` (software fixtures, or physical runs short enough for the debug cap); `debug_windows` lists at most one `{phase,start_ns,end_ns}` per required cell, each at most 2 s of ingress time (section 10.2), and is empty by default; `mismatch_cap` is the registered maximum of retained anomaly records per run (proposal 10^6). `validate` rejects a debug window longer than the cap, a second window in one cell, or `mode=full` on a physical cell longer than the cap.

`capacity` records byte/packet/rate bounds, observer ring sizes, maximum join lag, time/space limits, raw-record widths and processing/storage qualification. `sampling` is specified in section 7. `clocks` identifies E/O domains and timestamp-point conventions. `offloads` records FCS/VLAN stripping, coalescing, TSO/GSO/GRO/LRO, CQE compression and timestamp modes; reject undeclared changes. Hardware names/PCI addresses/credentials live only in a private deployment manifest, referenced by digest if needed.

### 3.2 Stream envelope and seals

Every JSONL stream starts with one header:

```json
{"kind":"header","schema":"hng-g2-events-v1","run":"demo","stream_id":"ingress-0","role":"ingress","producer":"host-adapter","producer_commit":"synthetic","clock_id":"engine","seq":0}
```

Every subsequent record has `kind`, `run`, `stream_id`, `seq`; `seq` is contiguous from 1 across **all** record kinds. The seal is last:

```json
{"kind":"seal","run":"demo","stream_id":"ingress-0","seq":4,"record_count":3,"records_sha256":"<sha256-of-header-and-three-records>","complete":true,"dropped_records":0}
```

`record_count` excludes header/seal. The digest covers canonical bytes of header and every intervening record, each followed by LF. Canonical encoding uses sorted keys, compact separators, ASCII escaping, exact integers and no NaN (`json.dumps(...,sort_keys=True,separators=(",",":"),ensure_ascii=True,allow_nan=False)`). Example digest placeholders must be replaced by calculated hashes in executable fixtures. Also retain file SHA-256 over actual bytes; distinguish these two hashes.

A producer increments `seq` before attempting export, so overflow creates detectable gaps. It increments an independent loss counter even if it cannot emit a gap event. A successfully written `gap` record contains `{first_missing_seq,last_missing_seq,reason}` but **never repairs completeness**. The seal is emitted only after the producer stops and all export queues drain; `complete=false` on abort. Hashes cover markers as well as packets, unlike the draft packet-only digest. Require both stream seals and per-port input end markers. Truncation, missing seals, gap or hash mismatch prevents qualification.

In stream mode the producer computes `records_sha256` over all produced records and the oracle recomputes it over the records it consumed; both digests are retained and must match. The persisted file then holds a subset, so its file SHA-256 identifies the retained bytes only. A third party can verify the retained subset and compare the two stream digests, but cannot recompute the stream digest without a full-mode or debug-window copy.

## 4. Events and identity

### 4.1 Complete pre-Q input

Each packet returned by RX receives an ingress ordinal before any host rejection and a `packet` record:

| Field beyond envelope | Type and meaning |
|---|---|
| `ingress_port`, `ingress_seq` | uint16, uint64; ordinal starts at zero per physical input port, includes invalid/handoff-dropped packets |
| `line`, `selection_seq` | Configured line name and **intended** uint64 line-local offer ordinal, assigned to every valid classified RX packet before handoff, including a subsequent handoff loss; null only for an unclassifiable/invalid packet |
| `external_identity`, `flow_id` | Packet-occurrence key and stable flow token; null only with explicit `identity_error`, which invalidates coverage |
| `arrival_ns` | Exact engine input time in header clock E; nondecreasing per input port |
| `length`, `vlan_id`, `segments`, `length_flags` | Retained bytes excluding FCS, tag ID, segment count; v1 flags are `{fcs_present:false,vlan_retained:true}`; ordinary valid profile is one segment |
| `app`, `selected_late` | Nullable registered app label; nullable **payload flag**, not a trusted model decision |
| `input_error` | null or explicit classification/length/identity failure reason |

`length` remains the draft field name. Independently derive `q_i=max(length+4,64)` L2 bytes and `w_i=q_i+20` line bytes. Check raw VLAN against manifest mapping, not only the host's `line`. Length/tag/ordinal corruption invalidates model input; an observed host rejection with intact raw input is a non-model data-path failure. Retain it and the terminal outcome instead of removing it from ingress history. Hardware packets lost before RX cannot be invented as model inputs; reconcile external before-point identities/NIC counters and report their loss separately.

The oracle advances intended selection ordinals on every valid classified RX packet, including Q/trace drops, independently of recorded production selection. `host.jsonl` also records `offer` events immediately before the real engine call: `{kind,run,stream_id,seq,ingress_port,ingress_seq,external_identity,line,at_ns,actual_selection_seq}`. `actual_selection_seq` counts actual calls per line from zero, including Q/trace rejections. A pre-offer host loss has a terminal record and no offer event; it consumes an intended ordinal but no actual one. Compare the histories explicitly rather than overwriting either. Such a run is a non-model FAIL with complete evidence; resulting selection/Q differences are reported as downstream divergence, not missing observer data. Nominal qualification requires every returned valid packet offered exactly once in original order with actual=intended ordinal. Optional production decisions may reference the offer stream, but never feed the intended-loss oracle.

Minimal semantic example (envelope omitted here only):

```json
{"kind":"packet","run":"demo","ingress_port":0,"line":"cable-forward","ingress_seq":0,"selection_seq":0,"external_identity":"f0:0","flow_id":"f0","arrival_ns":0,"length":1518,"vlan_id":101,"segments":1,"length_flags":{"fcs_present":false,"vlan_retained":true},"app":null,"selected_late":0,"input_error":null}
{"kind":"frontier","run":"demo","ingress_port":0,"through_ns":99,"next_ingress_seq":1}
{"kind":"end","run":"demo","ingress_port":0,"next_ingress_seq":1}
```

`frontier.through_ns` means **all input arrivals <= this time have already been emitted and consumed before the marker**; every later arrival must be strictly greater. `next_ingress_seq` equals the count consumed. This preserves the draft's closed-frontier convention. A certified core lower bound W means future arrivals >=W: export `through_ns=W-1` when W>0; no closed frontier is available at W=0. Conversely pass `through_ns+1` as a lower bound, with checked arithmetic. A clock read while timestamped NIC/RX backlog remains is not a certificate. Sequence and arrival checks remain active after a frontier. `end` certifies RX stopped and all pre-Q input/handoffs were accounted; it does not mean delay/TX/observer drain is complete.

### 4.2 Receiver and external-before observation

A `receive` record covers **every** output identity, not just sampled timestamps:

`{ingress_port, egress_port, line, external_identity, flow_id, receive_seq, receive_ns, timestamp, selected_late, length, vlan_id, app}` in addition to the envelope. `receive_seq` starts at zero per observed egress and records wire order. `receive_ns` and `timestamp` may both be null only for an unsampled packet whose order is independently preserved. `timestamp` when present is `{raw,clock_id,mapping_id,point,uncertainty_ns}`; `point` is `external_rx`, `tg_rx`, `software_rx`, `tx_submit` or `fake`. The collector computes mapped `receive_ns`; the reporter verifies it from retained raw data.

A `before` record uses the same identity/length fields with `ingress_ts_ns` instead of `receive_ns`, `before_seq` instead of `receive_seq`, and point `external_rx` or `tg_tx_port`. The after and before records join by `(run, ingress_port, external_identity)`, never by timestamp proximity. Full before-point identity coverage is required for the primary tap scheme; only timing can be sampled. Unmatched, duplicate or ambiguous **before-point** observations invalidate that coverage (they precede the DUT); after-point identity anomalies use the section 6 classes. A proven data-path loss with complete observer accounting is a FAIL, not an observer gap.

For software fixtures, before time equals injected `arrival_ns` under a declared fake-clock mapping. For physical runs, a separate before record is mandatory for every timing-selected survivor. The legacy draft `receive_ns` alone is insufficient physical evidence.

A single queue preserving port order is preferred. RSS delivery across queues does not establish wire order. A multi-queue observer must reconstruct order from validated hardware timestamps/order tags, with a complete queue frontier; ambiguous equal timestamps cannot be broken by ingress ordinal to manufacture correct order. If it cannot prove the relevant order, mark that order gate INCONCLUSIVE. [DPDK queue ownership/order context][ethdev]

### 4.3 Stage 1 probe and non-probe packets

Preserve Stage 1's 100-byte UDP probe payload:

| Byte range | Format |
|---|---|
| 0–7 | Big-endian uint64 sequence |
| 8–11 | Big-endian magic `0x4c30504b` |
| 12 | Selected-late flag, exactly 0 or 1 |
| 13–99 | 87 zero bytes |

The UDP port is a configurable manifest parameter, not hard-coded into the public decoder. Truncated headers, bad magic and invalid flags are rejected. For probe flow token `f`, identity is ASCII `f:<decimal-sequence>`, namespaced by run/input port. No host pointers, memory addresses or process-local mbuf IDs are external identity.

**Selection authority remains the independent counter selector.** Stage 1 generated probe flags separately and overrode its impairment selector; P1 does not silently gain such an override. Synthetic fixtures/generator trains must precompute flags from the registered P1 seed and intended per-line offer order and verify that the actual input order matches. Compare the carried flag at both observations with the independently selected flag. If production cross-flow interleaving prevents advance prediction, use a separate synthetic calibration run; do not rewrite forwarded payload or trust a payload flag as the P1 oracle. `selected_late=null` is permitted for ordinary non-probe traffic; oracle-derived late membership still drives strata.

For production traffic, the adapter must provide a collision-free **packet occurrence** mapping with an explicit retransmission/duplicate policy. A 5-tuple and TCP sequence alone are not unique. A digest of immutable bytes plus an ingress occurrence ordinal requires an independently unambiguous after-point match; identical retransmissions under loss/reorder can remain ambiguous. Retain diagnostic fingerprints but mark such coverage INCONCLUSIVE; do not claim full G2 from a unique-probe subset while silently excluding other traffic.

## 5. Independent reference and host evidence

### 5.1 Model algorithm and output

The existing `Reference.consume(event)` / `finish()` interface can remain after stream validation. The model reads engine JSON and normalized trace bytes independently, checks their hashes/units, and maintains one Fraction service ledger per line. It must:

1. Validate contiguous port/line ordinals and nondecreasing input times; derive lengths and late selection independently using `splitmix64-counter-v1`. Seed=0, ordinal=0 yields `0xe220a8397b1dcdaf`; p=0/1 are explicit endpoints. Selection is computed even for a Q-dropped offer.
2. Remove service entries with exact completion <= arrival **before** byte admission. Reject Q overflow without changing virtual service tail. A storage shortage is never inserted into the intended-loss model.
3. For admitted traffic, compute exact `S=max(a,S_previous)+8*w*10^9/C`, and `e=ceil(S)` only for deadline comparison. Do not accumulate rounded per-packet intervals.
4. Fixed mode: `deadline=release=e+t+I*delta`. Trace mode: use sample-and-hold values at e, independent loss selector, and `deadline=max(e+t(e),previous_retained_deadline)`; intentional trace loss consumes service but does not advance the retained floor. Outage drop/hold and finite terminal horizon follow HNG-DESIGN; `release=next_up(deadline)` for modeled hold.
5. Export a decision for each input packet; complete end markers and seals are required. Independent runtime/resource limits cause an incomplete report, not an altered model.

`kind="expected"` fields: identity/input fields; `egress_port`, `l2_bytes`, `wire_bytes`, `late` (independent boolean, including Q rejects), `outcome` (`accepted`, `q_drop`, `trace_loss`, `trace_outage`, `invalid_input`), `service_ns`, `deadline_ns`, `release_ns`, `clamp_extra_ns`, `q_bytes`, `q_drain_ns`. Nonexistent times are null. `q_bytes` is occupancy immediately after this admission/rejection at its arrival watermark; `q_drain_ns=ceil(max(0,S_tail-arrival))` includes remaining wire service, including the partially served head. Periodic Q records must include their watermark and whether they are an admission snapshot or a non-mutating current-time estimate.

The terminal summary (`kind="summary", schema="hng-g2-reference-v1"`) includes run/config/trace hashes, full input stream hashes and counts, per-port end counts, per-line offer/expected-loss/survivor counts, `input_complete`, oracle version and resource peaks. It is the last data record before the reference stream's seal. The reference assigns its own output `stream_id` and contiguous envelope `seq`; input ordinals remain separate fields. Never accept production deadlines/outcomes as the oracle's only input. Differential harness output is evidence to compare, not imported logic.

### 5.2 Terminal custody and counters

`host.jsonl` records every terminal non-TX packet outcome as `{kind:"terminal", ingress_port, ingress_seq, external_identity, line, at_ns, reason}`. Reasons are `q_drop`, `trace_loss_drop`, `trace_outage_drop`, `delay_storage_drop`, `invalid_input_drop`, `handoff_drop`, `terminal_tx_drop`, `missed_deadline_outage_drop`, `shutdown_drop`. A packet has one terminal outcome, either one of these reasons or successful TX acceptance; storage categories may have subreason/byte fields but must not be double counted. Q/trace losses are checked against the reference per identity, not just in total. A missed-deadline outage loss remains a non-model failure.

Each counter snapshot (`kind="counter"`) includes `scope` (`port`, `line`, `pool`, `observer`), `scope_id`, `at_ns`, `clock_id`, `reset_generation`, `values` and `descriptors`. Each descriptor names the unit (`packets`, `bytes`, `events`, `mbufs`), `semantics` (`terminal_disjoint`, `overlapping_diagnostic`, `gauge`) and source. Missing/unsupported values are null with a reason; software-only hardware counters are `not_applicable`, not zero. Require start/end snapshots and register periodic cadence (proposal: 1 s).

Required software packet counts per port: `rx_returned`, `tx_accepted`, `still_owned`, all terminal reasons above. Require observer `records_attempted`, `records_emitted`, `records_dropped`, `capture_drops`, `queue_high_water`, `queue_capacity`, `malformed_records`, `identity_errors`, `timestamp_missing`, `clock_invalid`, plus per-stream terminal counts. NIC RX/TX packets/bytes, missed/no-buffer/discard/error counters and mempool failures retain driver semantics; an overlapping NIC sum is only a zero/nonzero indicator, not a unique loss count.

At terminal drain: `rx_returned = tx_accepted + sum(disjoint_terminal_drops) + still_owned`, with `still_owned=0`. Reconcile mbuf get/free/driver ownership separately. TX accepted is not received: compare full receiver identities and port counters; missing output with complete capture is a non-model delivery failure even if software custody balances. If observation completeness itself is unknown, measurement is INCONCLUSIVE. Counters cannot excuse an unexplained identity mismatch or replace missing events.

### 5.3 Actual-time port arbitration

Per-line release sorting alone cannot validate two lines sharing a port during outages. Capture an `attempt` record for **every TX burst and retry**, including zero accepted:

`{kind, run, stream_id, seq, egress_port, queue_id, attempt_id, gate_ns, submit_before_ns, submit_after_ns, seen_next_ingress_seq, frontiers, consumed_dispatch, offered, accepted_count}`.

- `attempt_id` is contiguous per egress, starting at zero. Times are in E and obey `gate_ns <= submit_before_ns <= submit_after_ns`.
- `seen_next_ingress_seq` maps each input port to the prefix actually consumed by the scheduler at selection time. `frontiers` maps ports to their consumed closed frontier or an explicit input-end marker. Do not infer either from later arrivals.
- `consumed_dispatch` maps every registered handoff queue ID to the next handoff item ordinal not yet consumed by this TX owner. The prefix is contiguous from zero and monotone. It defines what is actually available, including previously consumed pending suffixes; a dispatch timestamp alone does not. Freeze these prefixes while assembling the offered burst. An implementation that consumes more handoffs while assembling must record the updated map on each affected offered entry instead.
- `offered` is an ordered array of `{ingress_port,ingress_seq,external_identity,line,gate_ns}`. Per-entry gate time records the actual selection check; if the implementation checks a whole burst once, all use the same time and it must report that batching policy. `accepted_count` is in `[0,len(offered)]` and means exactly that prefix transferred to the NIC.
- Also record `dispatch` events `{at_ns,egress_port,handoff_queue_id,first_handoff_seq,identities,seen_next_ingress_seq,frontiers}` for successful scheduler-to-TX enqueue, including the common envelope. Each queue's handoff item ordinals are contiguous from zero; `identities` is the successfully enqueued ordered batch, giving item ordinals `first_handoff_seq + index`. Unsuccessful enqueue items remain scheduler-owned during retry, without handoff IDs; emit a terminal event only when an item is actually discarded. Record any host pending-queue drop with a terminal event. An adapter may combine dispatch and attempt only for a genuinely fused path, using a declared local queue with the same consumed-prefix semantics.

The independent checker replays ingress prefixes, dispatch/pending custody, frontiers and trace state in event order. At dispatch, no earlier safe releasable head may be skipped. At each TX selection/retry, compare eligible pending line heads using `(release_ns,ingress_seq)` and the registered egress mapping. Preserve each line's unaccepted suffix, allow a held-down LEO line to be bypassed by cable, and remove accepted prefix entries exactly once. Missing evidence, impossible prefix or ambiguous same-clock ordering is INCONCLUSIVE; a fully observed wrong choice is FAIL. Producer claims of gate state are diagnostic only; derive the gate from the trace independently.

A fresh scheduler head, an enqueued but unconsumed handoff and a TX-pending packet are different custody states; do not require the TX owner to send a packet it has not received. The dispatch checker enforces the full merger rule, while the TX checker enforces pending order and actual-time outage eligibility. Every offered entry includes `handoff_queue_id` and `handoff_seq`. On each consumed-prefix advance, transfer exactly those dispatched items into the checker’s pending set; reject holes, regression or consumption beyond dispatched items. Check all eligible consumed heads across queues, retaining previously unaccepted items. This establishes availability without inventing cross-thread ordering from timestamps alone. The manifest fixes each handoff queue's single producer/consumer and capacity; a shared multi-producer queue without a recorded total enqueue order is unsupported in v1.

Boundary example: LEO modeled release 190, cable 220, outage `[200,300)`, owner resumes at 250. Cable may transmit first while LEO holds; LEO retains expected release 190 and its extra wait counts as error. A modeled survivor dropped at actual time 250 is not intentional trace loss. Packets already accepted by the NIC cannot be recalled: observed wire transmission across an outage boundary is measured separately. Retain first/last-bit convention and uncertainty; do not promise precise wire silence from a software gate.

## 6. Complete order checks and bounded processing

Use full receiver identities, including unsampled traffic, to check: no duplicate delivery; all model survivors delivered or explicitly failed; only intended Q/trace identities absent; per-line stable release order; ordinary and late subsequence order; allowed late overtakes; and actual port arbitration where required. Exact whole-port sorting is valid only when no actual-time gate bypass changes eligibility. Preserve Stage 1 per-flow high-water out-of-order counts as diagnostics; the P1 independent permutation check is stronger. A flow crossing lines has no invented cross-line FIFO guarantee: evaluate its registered routing/epoch contract, not a new scheduler policy.

Receiver identity anomalies have exactly one class each. Classify them only after stream integrity (section 3.2) is checked; a stream defect such as a repeated envelope `seq` is a stream error, not a receiver anomaly. "Complete observer coverage" means: all ingress and receiver streams sealed `complete=true` with no `seq` gap, observer `records_dropped`, `capture_drops` and observer NIC drop counters known and zero, and drain finished within the registered bound.

| Anomaly | Class and reason code | Rationale |
|---|---|---|
| Receiver identity never present in the complete ingress streams | INVALID, `receiver_unknown_identity` | The DUT forwards identities; it does not create them. An unknown identity is capture contamination, a parse/mapping error or an identity collision, so the identity evidence itself is unreliable. A DUT that corrupts identity bytes cannot be told apart from these, so the run cannot PASS either way |
| Identity delivered although the reference outcome is `q_drop`, `trace_loss`, `trace_outage` or `invalid_input` | FAIL, `delivered_intended_drop` | Ingress identity and the independent decision are both proven; the wire shows a packet the model forbids. Applies even if a host terminal record agrees with the reference |
| Same identity at two different `receive_seq` values (one egress or across egresses) | FAIL, `duplicate_delivery` | A sealed contiguous stream shows two wire occurrences of one ingress occurrence: a data-path duplication |
| Reference survivor not received | FAIL, `delay_storage_drop` when its terminal record is `delay_storage_drop` (storage gate), otherwise `missing_delivery` (non-model delivery gate) | With complete observer coverage the absence is a proven loss, located or not; custody balance cannot excuse it (section 5.2) |

Without complete observer coverage, an absent survivor is not this anomaly: the cell is INCONCLUSIVE with the coverage reason (for example `observer_incomplete`), and it never counts as DUT loss.

For long runs, the default implementation path is bounded streaming joins partitioned by input direction/line, plus retained mismatch records and digests. Retain unresolved identities until the registered residence/drain/observer-lag bound expires; eviction must create an explicit unresolved record and invalidate the run, never count a pass. The bound includes trace hold and host queues. The current draft O(N) in-memory reporter is acceptable only for bounded software fixtures. Offline external sorting/joining is an alternative if disk capacity and exact stable ordering are preserved. Hash digests supplement counts and joins; they do not replace per-identity comparison.

**Classification without full logs.** The streaming oracle receives every ingress, receiver and host record and performs all checks of this section online, so every anomaly class above is still decided on complete evidence. For each anomaly it retains one mismatch record (the identity, its ingress/receive records, terminal record, reference outcome and class) up to `retention.mismatch_cap`; counts per class stay exact beyond the cap, and reaching the cap gives INCONCLUSIVE (`mismatch_cap_exhausted`) because the retained evidence no longer covers every anomaly. A cell with zero anomalies retains no mismatch records. What is lost outside debug windows: per-packet forensics of non-anomalous unsampled packets (for example, the exact interleaving around a later-found bug), re-running a corrected reference version on the same data (a rerun of the cell is needed), and independent recomputation of stream digests by a third party.

After input end, wait for model ledger, propagation storage, TX pending, NIC completion accounting and observer queues to drain within registered coverage. Seal every stream, then produce results. Forced timeout has an explicit incomplete-drain reason. Snapshots taken before drain cannot establish final conservation.

## 7. Sampling, uncertainty and confidence intervals

### 7.1 Main sample and additional strata

Keep the draft's deterministic sampling exactly. Let `key=[run,ingress_port,external_identity]` encoded with the canonical JSON function above. Compute:

```text
bytes = ASCII("hng-g2-timing-v1") || 0x00 || uint64_be(sampling_seed) || canonical(key)
h = uint64_be(SHA256(bytes)[0:8])
main = (h & (2**m - 1)) == 0      # m = registered mask bits of the cell; default m = 10
```

The default m=10 is the draft's 1/1024 rule unchanged. A cell may register a smaller m (0 <= m <= 10) before data collection; m=0 times every survivor. Because the rule is a mask of the same hash, the m=10 set is a subset of every smaller-m set, so oversampling never changes which default-rate packets are selected. One m per cell; never change it after data collection.

The seed is registered before packet generation and independent of late/loss selection. Apply the rule at both observation points and independently in the reporter. Do not replace a missing selected pair with a later packet. Use all model survivors in the window as the timing population; intended drops remain in the full-input model but have no OWD. Every main-selected **delivered** survivor needs a valid pair. A model survivor proved lost by the DUT is recorded as `datapath_lost`, not as an observer-missing pair; the row fails the data-path/storage gate. Timing statistics of remaining deliveries are labeled conditional on delivery and cannot rescue that run. Without complete observer coverage (section 6), the absence remains measurement incompleteness.

Interoperability vector: seed=17 and key `["demo",0,"f0:0"]` produce SHA-256 `0a99a7341a2be1cd5a4f8b764a2cc90ef0ebf538d9d2fe265c6a2e518300e7b6`; the low ten bits are 461, so the packet is not in main.

Report `main` separately from `late_extra` (all selected-late survivors outside main) and `app_extra:<label>` (all declared app survivors outside main). Extra strata can overlap each other; identities must not duplicate within a stratum. The main sample contains late/app packets naturally. Never concatenate extra strata into the uniform main sample. Keep per-line main, ordinary-main and late-main diagnostics, plus per-direction aggregates. Required gates are per registered line/direction/cell, so a dominant cable population cannot mask a failing LEO line. Strata with no population are `not_applicable`; missing samples from a nonempty population are insufficient evidence.

Both observation points must preserve timestamps for the union of main and registered extra strata. If late membership is not independently known at capture time, timestamp all candidate packets and retain their timestamps in a bounded join until the reference supplies membership, or capture a registered superset. A production late flag may trigger extra capture but cannot be the only membership/completeness authority. Failure to retain required extra timestamps is reported, never silently removed from the stratum.

Stage 1-compatible dedicated probes can provide a separate diagnostic stream. They cannot replace a 1/1024 sample of all qualified traffic or the complete order checker. Fixed hardware filtering that captures only a favorable traffic subset must not be described as identity-hash sampling.

### 7.2 Point estimates and intervals

For sampled signed error `r_i=OWD_i-expected_i`, retain both signed and absolute errors. For sorted absolute errors `x[0..n-1]`, the point q-quantile linearly interpolates at `(n-1)*q`, exactly as Stage 1 `de035a6`; q=0.5 and 0.99. The interpolation convention matters and is versioned. [NIST discussion of quantile conventions][nist]

Default interval: 95% equal-tail binomial order-statistic bounds, matching the draft's intended method. For `K~Binomial(n,q)`, choose `L=max{k in [0,n]: P(K<k)<=0.025}` and `U=min{k in [1,n+1]: P(K>=k)<=0.025}`. The interval is `[X_(L),X_(U)]` using 1-based ranks; `X_(0)=0` for nonnegative absolute error and `X_(n+1)=+infinity` (JSON upper null). Compute binomial tails stably; do not use numerical underflow to turn an unbounded interval into a finite one. Test small n and q=0.99 against exact binomial sums. Apply these ranks unchanged when errors have ties or atoms: order-statistic bounds over a discrete distribution already cover at least the nominal level, so no tie adjustment is made.

This is a registered **statistical design choice** under independent uniform packet sampling (a hash treated as pseudorandom, not chosen to favor observed errors). It describes the packet distribution in that run, not independence of time-correlated process behavior or reproducibility across runs. Report independent repeats and per-time-window distributions separately; never treat millions of packets as millions of independent experimental repetitions. No pooled CI across changing trace phases. If making a process-level claim beyond the observed run, add a preregistered time-block/repeat analysis; absent that, restrict the claim. A finite-population exact alternative is allowed only as a separately versioned method with matching tests.

**Minimum main sample.** U is finite only when `U<=n`, i.e. `P(K>=n)=0.99^n<=0.025`, so `n>=ln(0.025)/ln(0.99)=367.04`, giving **n_min=368** paired main samples per cell (check: `0.99^367=0.025010>0.025`, `0.99^368=0.024760<=0.025`). At n=368, U=368, so the p99 upper bound is the sample maximum; U<=n-1 first holds at n=555. The median bound is finite from n=6, so p99 sets the minimum. This is a necessary condition for a finite bound, not a PASS guarantee.

At mask m, the cell needs `368*2^m` delivered survivors in expectation; at m=10 that is 376,832. Because the hash count is binomial, that expectation falls short of 368 with probability 0.493. Register instead the **planning target** N_plan(m): the smallest survivor count with `P(Binomial(N,2^-m)<368)<=0.001`; N_plan(10)=440,433 (430.1 expected main samples). At 1,542 line bytes this is 54.3 ms of one 100G direction (8.106 Mpps), 0.543 s at 10G, and 5.43 s on a 1 Gbit/s line (81,064 pps). The binding rule is on the actual paired count, not on the plan.

Registration per required cell, from registered offered rate, window length and intended loss, in this order:

1. If planned survivors >= N_plan(10), use m=10.
2. Otherwise use the largest m with planned survivors >= N_plan(m) (oversampling, typical for low-rate LEO lines and short `transition` windows); m=0 needs at least 368 survivors.
3. If even m=0 cannot reach 368 survivors in one repeat, register a **pooled row** (`repeat="pooled"`) over the declared repeats of the same condition, direction, line, phase and trace segment, never across phases. The pooled row is the required timing row and uses the same thresholds. Per-repeat rows stay as diagnostics; any conclusive per-repeat FAIL (order, storage, or a timing lower bound above threshold) still makes the cell FAIL.
4. If pooling the declared repeats also stays below 368, `validate` rejects the registration (INVALID, `insufficient_planned_samples`); add repeats or lengthen the window. Never register a timing gate as not applicable to avoid this.

A cell whose window contains no survivor ingress (normally `drain`, because samples are assigned by ingress time) has timing gates `not_applicable`; its order, storage and loss gates still apply. A nonempty timing population with fewer than 368 paired main samples is INCONCLUSIVE (`insufficient_main_samples`), even if the point estimates pass.

Expand quantile bounds by the calibrated worst-case OWD uncertainty u: `[max(0,L-u), U+u]`. Require u<=10,000 ns for physical scope. PASS a timing gate only if the expanded upper bound is <= its threshold; FAIL if the lower bound is > threshold; otherwise INCONCLUSIVE. Record the Stage 1 point-only threshold result separately. The interval check intentionally prevents a near-threshold point estimate with inadequate evidence from qualifying. No finite sample minimum by itself overrides an unbounded/straddling interval. Software fault fixtures also need at least 368 paired main samples in every cell whose timing gate they test.

## 8. Serialization and Q reporting

Do not redefine modeled deadlines to remove congestion when late and ordinary packets become due together. All extra physical serialization remains in G2 error. Diagnostics use an independent work-conserving serializer at actual registered link rate R on **modeled survivors**, with the declared first-bit reference:

`start_i=max(release_i,finish_previous)` and `finish_i=start_i+8*wire_bytes_i*10^9/R`, using exact rational arithmetic. `serialization_wait_i=start_i-release_i`. First run this in stable eligible release order for fixed mode; for actual trace gate bypass, separately label a serializer driven by the verified attempt order/gate times. It is explanatory, not a replacement for original expected release.

Define a `late_involved_busy_period` as a modeled egress busy period containing a late packet; report its waiting packets/bytes, max wait, signed residual `OWD-expected-serialization_wait` and wait distribution. This is a reproducible association, **not** causal attribution of every ns to late selection. An optional counterfactual ordinary-only serializer must be labeled a different diagnostic. Never subtract either diagnostic for G2. Trace clamp and modeled outage hold are separate expected-delay components; unplanned stall-induced hold remains error.

Q reports per line: configured/peak/current L2 bytes, snapshot watermark, exact remaining service drain time `ceil(max(0,S_tail-watermark))` ns, and separately `8*Q_bytes*10^9/C` normalized ns. The latter omits preamble/IFG and partially served-head detail; it is not the exact queue drain. Report configured Q both as bytes and its explicitly named byte normalization, and observed drain from the ledger. Do not age the production ledger to wall time while earlier input is unconsumed.

## 9. Verdicts and public report format

### 9.1 Decision order

Evaluate each required line/direction/cell; keep direction aggregate diagnostics and port-wide validity. Never average a failing cell into a passing aggregate.

1. Structural/configuration errors (schema, hash, `receiver_unknown_identity`, duplicate input identity, impossible sequence/time, missing required files) give `status=INVALID`, `measurement_status=INVALID`, with reason codes. Emit an error report if possible; do not crash into an empty successful report.
2. Measurement incompleteness (observer drops, missing seals/end, missing timing pairs for delivered selected packets, unresolved identities, incomplete drain, unknown counters/calibration, excessive uncertainty, unsupported order reconstruction, fewer than 368 paired main samples in a nonempty timing population) gives `measurement_status=INCONCLUSIVE`, compatibility `status=INVALID`. Within a row this takes precedence over recorded numeric failures, which remain visible in `gates`/`observed_failures`. A fully proved data-path loss is not observer incompleteness; its absent egress/timing record proceeds to step 3 as a failure. Receiver anomalies use only the section 6 classes.
3. With valid evidence, fail non-model data-path loss (`missing_delivery`, `delivered_intended_drop`, `duplicate_delivery`), unintended order or delay-storage loss. Evaluate timing gates with section 7's intervals. Any conclusive gate failure gives FAIL; otherwise any indeterminate required gate gives INCONCLUSIVE/INVALID; all required gates passing gives PASS.
4. Across required rows, a valid FAIL takes precedence; otherwise any missing/duplicate/invalid/inconclusive row gives INVALID; only exactly the registered set of passing rows gives PASS. This preserves Stage 1 aggregation. Additional diagnostic rows never fill missing required rows.
5. `g2_qualified=true` only for **physical** scope, overall PASS, qualified observer/bypass/load and all required evidence. Software fixtures may report PASS for algorithm checks but always `g2_qualified=false`, `scope="software-only"`, and banner **"software-only, not G2"**.

Hard limits: median absolute error 100,000 ns; p99 absolute error 500,000 ns; unexpected order=0; delay-storage loss=0. Intended Q/trace loss is separate. A genuinely measured storage/NIC/terminal loss is FAIL when evidence is complete; do not automatically call every missing survivor an observer gap. Missing or reset counters cannot prove a zero loss result. Applicability of every counter is explicit.

### 9.2 Report fields and outputs

`report.json` schema `hng-g2-report-v1` has required keys: `run`, `condition_id`, `scope`, `status`, `measurement_status`, `g2_qualified`, `reason_codes`, `sources` (all input/config/calibration/producer hashes), `stage1_source_commit`, `stage1_source_review`, `rows`, `directions`, `stream_integrity`, `observer_cost`, `registration` and `unavailable`.

Each `rows[]` element has:

| Fields | Meaning |
|---|---|
| `condition_id`, `repeat`, `direction`, `line`, `phase`, `required` | Unique cell key and registered applicability; `repeat` is `pooled` for a section 7.2 pooled row |
| `status`, `measurement_status`, `g2_pass`, `g2_qualified`, `reason_codes`, `observed_failures` | Compatibility and detailed verdict; g2_pass means four model gates passed, not physical certification |
| `gates` | median/p99/order/storage objects with status, threshold, value, CI and expanded bound; separate non-model-loss/coverage validity |
| `error_median_ms`, `error_p99_ms` | Main-sample point estimates; exact ns inputs retained; null when unavailable |
| `timing` | main/extra groups: population, selected/paired/missing counts, point/CI, raw/corrected OWD, signed/absolute errors, algorithm/seed/mask bits m, pooled repeat set if any; CI infinity is null plus `upper_unbounded=true` |
| `counts` | Full ingress, model outcomes, received identities, duplicates, unknowns, probe subset counts, selection/actual-OOO/unexpected-OOO |
| `counters`, `q`, `serialization`, `clamp`, `outage`, `clock` | Layer evidence and diagnostics defined above |
| `offered_bps_l1`, `delivered_bps_l1`, `delivered_bps_l2`, `pps`, `l7_bps` | Per direction rates with measurement window and accounting basis; L7 null unless measured |

`results.csv` uses one row per required cell and these stable columns: `condition_id,repeat,direction,line,phase,status,measurement_status,g2_pass,g2_qualified,error_median_ms,error_p99_ms,delay_storage_drops,capture_drops,nic_drops,shared_q_status,shared_q_drops,trace_loss_drops,trace_outage_drops,probe_sent,probe_received,probe_lost,probe_selected_late,probe_actual_ooo,probe_selected_late_ooo,probe_unexpected_ooo,timing_samples,reasons`. Missing numeric values are empty; full explanation remains in JSON. `probe_*` counts only the Stage 1-format probe subset, not all packets renamed as probes; full counts are in JSON. `reasons` is semicolon-joined stable codes. Additive columns require documented schema revision.

`summary.txt` shows scope, overall verdict, passed/required cells, each failed/invalid cell and first reasons, thresholds and report path. CLI exit 0=PASS, 1=valid FAIL exists, 2=INVALID/INCONCLUSIVE or handled setup/schema error; controlled interruption may use 130 with incomplete outputs explicitly marked. A requested output directory must be new; never overwrite previous evidence.

## 10. Observer cost and 100G budget

The host exporter must copy fixed metadata into a preallocated bounded per-producer queue without synchronous JSON, hashing, allocation, locks shared with the oracle, disk writes or waits for consumer progress. Hash/serialize on observer cores. On queue overflow increment a reliable loss counter, latch invalidity and continue or perform an explicitly invalidating controlled stop. Never silently slow the DUT until the oracle catches up. Measure timestamp-on/export-on overhead, CPU, per-node bandwidth, queue occupancy and ring losses; capture-off results are diagnostic only.

The canonical JSONL format is required for fixtures/interchange. A lossless compact adapter is permitted for hardware, but its exact binary layout, endian rules, lengths, checksum/seal and round-trip equivalence tests must be documented and pinned before qualification. **The following widths are capacity estimates, not a hidden binary ABI or proof JSON records are this small.** Do not implement a new binary protocol just to satisfy software M2; qualify measured encoded widths before M5.

At 100G, 1500-byte IP plus one VLAN is 1,542 line bytes. Bidirectional rate is `2*100e9/(8*1542)=16.212711 Mpps`.

Illustrative packed layouts, one per record class, derived from the field lists in sections 4–5. Common assumptions: `run`/`stream_id`/schema live in the stream header; each record carries `kind` (1 B) and envelope `seq` (8 B); line, app, clock and reason are manifest indices (2/2/1/1 B); a synthetic or probe `external_identity` is flow index u32 + occurrence u64 = 12 B and also gives `flow_id`; nullable small fields use a sentinel; each record pads to a multiple of 8 B. A wider production identity adds its extra width to every identity-bearing record and per-entry array below.

| Record | Field bytes | Packed | Padded |
|---|---|---:|---:|
| ingress `packet` (§4.1) | kind 1, seq 8, ingress_port 2, ingress_seq 8, line 2, selection_seq 8, identity 12, arrival_ns 8, length 4, vlan_id 2, segments 1, length_flags 1, app 2, selected_late 1, input_error 1 | 61 | 64 |
| host `offer` (§4.1) | kind 1, seq 8, ingress_port 2, ingress_seq 8, identity 12, line 2, at_ns 8, actual_selection_seq 8 | 49 | 56 |
| fused ingress+offer (option) | ingress fields 61, at_ns 8, actual_selection_seq 8, offer-present flag 1 | 78 | 80 |
| after `receive` identity/order (§4.2) | kind 1, seq 8, ingress_port 2, egress_port 2, line 2, identity 12, receive_seq 8, selected_late 1, length 4, vlan_id 2, app 2, timestamp-present flag 1 | 45 | 48 |
| external `before` identity (§4.2) | as `receive` without egress_port, with before_seq in place of receive_seq | 43 | 48 |
| timing row, one per observation point (§4.2) | kind 1, seq 8, ingress_port 2, line 2, identity 12, mapped ns 8, timestamp {raw 8, clock_id 1, mapping_id 2, point 1, uncertainty_ns 4}, stratum flags 1 | 50 | 56 |
| `dispatch` batch header (§5.3) | kind 1, seq 8, at_ns 8, egress_port 2, handoff_queue_id 2, first_handoff_seq 8, entry count 2, seen_next_ingress_seq {port 2, u64 8}, frontiers {port 2, through_ns 8, end flag 1} | 52 | 56 |
| `dispatch` per identity | identity 12 | 12 | 12 |
| `attempt` header (§5.3) | kind 1, seq 8, egress_port 2, queue_id 2, attempt_id 8, gate_ns 8, submit_before_ns 8, submit_after_ns 8, seen_next_ingress_seq 10, frontiers 11, consumed_dispatch {queue 2, u64 8}, accepted_count 2, offered count 2 | 80 | 80 |
| `attempt.offered` entry | ingress_port 2, ingress_seq 8, identity 12, line 2, gate_ns 8, handoff_queue_id 2, handoff_seq 8 | 42 | 48 |
| `terminal` (§5.2) | kind 1, seq 8, ingress_port 2, ingress_seq 8, identity 12, line 2, at_ns 8, reason 1 | 42 | 48 |

**Full per-packet volume (debug mode only, section 10.2).** These are the bytes that every record class produces; in the default stream mode they flow through the oracle and only the section 10.1 subset is persisted. Per-input volume for 600 s at zero intended loss, with v1's one opposite ingress port and one handoff queue per egress, mean burst B=32 for both dispatch and attempt, and no TX retries:

| Retained record class | Bytes / input packet | Rate | Ten-minute volume |
|---|---:|---:|---:|
| Complete pre-Q ingress | 64 | 1.038 GB/s | 622.6 GB |
| Host offer | 56 | 0.908 GB/s | 544.7 GB |
| Complete after identity/order | 48 | 0.778 GB/s | 466.9 GB |
| Extra external-before identity | 48 | 0.778 GB/s | 466.9 GB |
| Dispatch | 12 + 56/32 = 13.75 | 0.223 GB/s | 133.8 GB |
| TX attempt | 48 + 80/32 = 50.5 | 0.819 GB/s | 491.2 GB |
| Paired main timing | 2*56/1024 = 0.109 | 1.773 MB/s | 1.064 GB |
| **Total** | **280.36** | **4.545 GB/s** | **2.727 TB** |

Check: `280.359375 B * 16.212711e6 /s = 4.545 GB/s`; `* 600 s = 2.727 TB`. Fusing offer into the ingress record (one host producer, offer fields null on a pre-offer loss) gives `280.36-64-56+80 = 240.36 B`, 3.897 GB/s and 2.338 TB. Intended loss at fraction p replaces about 48p B of after records with 48p B of terminal records, so the total is unchanged to this precision. Each TX retry adds an 80 B header plus 48 B per re-offered entry; at B=1 the per-packet total rises to 412.11 B. The 10%-late extra stratum at 112 B/pair adds `0.1*112*16.212711e6*600 = 108.9 GB`, for **2.836 TB** per cell with it.

The table excludes expected output (streamed), counters, stream headers/seals, indices, repeats and other extra strata. If all of it were persisted, a ten-minute cell would need at least **3.6 TB** (2.836 TB plus 25% for these and for width error, rounded up). That is no longer the default. These widths are not measured; register the measured encoded widths, burst distribution and retry rate before M5, and recompute. TG TX/RX backup may avoid the extra passive-before identity stream but still needs complete generation/input/delivery evidence. Streaming reduces disk, not event processing: the oracle still consumes about 4.545 GB/s of records at this reference load.

### 10.1 Default: streaming oracle with sampled full records

The oracle computes every gate online and persists only:

1. **Main-sample full records.** For each 1/1024 main-sample identity (section 7.1), every record that carries it: ingress 64 B, offer 56 B, after 48 B, external before 48 B, its dispatch header and entry 56 + 12 B, its attempt header and offered entry 80 + 48 B, and two timing rows 2 × 56 B. Worst case, each sampled packet is in its own dispatch and attempt batch: `64+56+48+48+68+128+112 = 524 B` per sampled packet, or `524/1024 = 0.512 B` per input packet.
2. **Histograms.** Per required cell, line and stratum (main, ordinary-main, late-main, `late_extra`, `app_extra:<label>`): signed and absolute OWD error at 1 µs bins from -10 ms to +10 ms with exact underflow/overflow counts and exact minimum/maximum; reordering high-water and drop counts per layer. 2 × 20,001 bins × 8 B is about 0.32 MB per histogram set (*estimate*); negligible below.
3. **Mismatch records**, capped by `retention.mismatch_cap` (proposal 10^6 per run). At the full per-packet width this is at most `10^6 × 280.36 B = 0.280 GB`.
4. Counters, stream headers, seals and both stream digests (section 3.2).

The timing gates are computed from the retained main-sample pairs exactly as in section 7.2: n, the quantile point estimates and the L/U ranks need only the main sample, so `n_min=368`, the planning target N_plan(10)=440,433 and the 376,832 expected count are unchanged. Histograms serve the extra strata and diagnostics; an extra-stratum quantile from a histogram uses the conservative bin edge (bin upper edge for an upper bound, lower edge for a lower bound). Order, duplicate, storage and missing-delivery checks use the complete streams online (section 6).

**Per-cell storage (estimate)**, bidirectional 100G, 1,542 line bytes, 600 s: main samples `16.212711e6/1024 × 600 = 9.4996e6`; full records `9.4996e6 × 524 B = 4.978 GB`; plus the 0.280 GB mismatch cap gives 5.258 GB; with 25% for headers, histograms and width error, `5.258 × 1.25 = 6.573 GB`. Provision **7 GB per ten-minute cell**, against 3.6 TB with full recording. One 100G direction halves the sample term: `(2.489 + 0.280) × 1.25 = 3.46 GB`, provision 4 GB. A smaller registered m (oversampling) multiplies the sample term by `2^(10-m)`; recompute it per cell.

### 10.2 Debug mode: full per-packet recording

Full recording (`retention.mode=full`, or a `debug_windows` entry) is off by default and must be switched on explicitly in the manifest before data collection. Each window covers at most **2 s of ingress time** in one cell; outputs of packets that entered in the window are retained through drain. 2 s is about four times the 512 ms residence used below (142+320+50 ms), so most packets of a window complete inside it. Cap arithmetic at the reference load: `4.545 GB/s × 2 s = 9.09 GB`; with the 10%-late stratum `2.836 TB / 600 s × 2 s = 9.45 GB`; with 25% margin `9.45 × 1.25 = 11.8 GB`. Provision **12 GB per debug window**, at most one window per cell. The writer must sustain about 4.7 GB/s for the window, or buffer it in RAM; an observer drop during the window gives INCONCLUSIVE for the cell, exactly as outside it. A debug window does not change any gate and is reported in `report.json`. Software fixtures may use `mode=full` without the cap.

Retain unresolved identities in the bounded joins until the registered residence/drain/observer-lag bound expires (section 6). Register the retention caps; cap exhaustion is explicit, not silent sample loss. Extra stratum streaming summaries use the histograms above or an exact sorted spill; the first software reporter can keep them in memory for limited fixtures.

A 64-byte ingress ring holding 100 ms of aggregate traffic already needs about 103.8 MB. At 512 ms residence, ~8.301 million unresolved identities require at least 531.3 MB at an illustrative 64 B each, before hash-table/Python overhead. Calculate per-line/window burst bounds, not only averages. The observer receives four physical monitor streams (up to 400G aggregate RX); both its PCIe and timestamp-on receive path must be qualified. Failure to sustain this evidence pipeline means lower `R_valid` or INCONCLUSIVE at 100G, not permission to sample away full-input evidence.

## 11. CLI compatibility, implementation steps and acceptance tests

Keep draft commands, adding strict stream/manifest validation and optional `--out NEW_DIRECTORY`:

```sh
python3 -m tools.g2.reference engine.json demo ingress.jsonl > expected.jsonl
python3 -m tools.g2.report engine.json expected.jsonl receiver.jsonl counters.json manifest.json --out results
```

For migration, `counters.json` is the normalized terminal view derived from `counters.jsonl` with retained snapshot hashes, status descriptors and start/end deltas; the positional name remains compatible. Manifest paths provide before/host/calibration streams. A new capture adapter takes only operator-supplied manifest/interface/port parameters; no default live network interface. `python3 -m tools.g2.validate manifest.json` is the proposed software-only schema/integrity preflight; it makes no network connections. These final-schema commands are an implementation contract, not a claim they already work on this documentation branch.

Implement in this order: strict format/seal adapter; independent reference compatibility; bounded identity/pair join; counter/attempt checker; report/statistics/CSV; virtual-device fixtures; then hardware timestamp adapter and calibration. Keep public helpers small and tool-local. CI/pre-push run the same independent unit/fault tests plus software PMD integration without physical NICs/hugepages; af_packet privilege limits are reported explicitly and resolved before the full software gate.

Required acceptance fixtures:

| Test | Required outcome |
|---|---|
| 10^6 or more full events, fixed/trace, >=32 seeds | Independent per-packet Q/service/selection/release agreement against a separate engine harness; both directions/four states; reference does not import engine |
| Delay UP / DOWN | `(e,t)=(10,100),(20,200),(30,200)` -> deadlines 110,220,230; DOWN t=200,100,100 -> 210,210,210 and original order |
| One ordinary pair swapped | Two packets on the same line, different release times, neither late; complete evidence -> order FAIL; include an unsampled pair so timing-only checking cannot pass |
| One delay-storage drop | Complete terminal evidence and missing survivor -> storage FAIL; not reclassified as trace/Q loss |
| Survivor missing, no terminal record | Complete observer coverage -> FAIL `missing_delivery`; same with one receiver `seq` gap -> INCONCLUSIVE (section 6) |
| (c) main: 60% receive an extra 0.2 ms | Zero-error baseline; survivors with `ingress_seq % 5 < 3` get +200,000 ns. Median point = 200,000 ns, median CI lower bound = 200,000 ns > 100,000 -> median gate FAIL; p99 gate PASS (all errors <= 200,000 ns); row FAIL with median as the only failing gate |
| (c1) 5% receive an extra 0.2 ms | Same baseline; `ingress_seq % 20 == 0` gets +200,000 ns. Assert: main count of samples with absolute error > 0 equals the fixture's independently computed k; main max signed error = 200,000 ns; p99 point = 200,000 ns; median point = 0 ns; both timing gates PASS; row PASS |
| (c3) 5% receive an extra 0.6 ms | Same selector as c1 with +600,000 ns. p99 point and CI lower bound = 600,000 ns > 500,000 -> p99 gate FAIL; median PASS; row FAIL. Variant: +0.2 ms on a registered 0.4 ms baseline tail gives the same p99 FAIL |
| 1% observer record loss | Sequence/counter/seal gap -> INCONCLUSIVE, CSV INVALID; never count as DUT loss |
| Trace modeled hold and actual-time delayed hold | Expected uses modeled release for first case; a later CPU-induced outage wait remains error; cable bypass allowed only with complete attempt evidence |
| Zero/partial/full TX, retries across outage | Accepted prefix/custody correct; illegal drop/gate order fails; missing attempt evidence inconclusive |
| Clock/calibration faults | Different raw domains without mapping, >10 µs bound, drift/step, missing pair, extrapolation -> INCONCLUSIVE; do not subtract engine baseline |
| Missing seal/end, duplicate key/identity, bad hash, backward frontier | INVALID with stable reasons; marker corruption is detected even if packet digest matches |
| Statistics | Stage 1 point quantiles match; exact small-n binomial bounds, ties, unbounded p99, uncertainty-straddling threshold; separate extra strata do not bias main |
| Aggregation | Invalidity wins inside row; a different valid failing row gives overall FAIL; missing/duplicate required row cannot PASS; software PASS never qualifies G2 |
| Stream versus full retention | The same fixtures run in `stream` and `full` mode give identical `report.json` verdicts, gates, counts and main-sample statistics; stream-mode persisted records are exactly the main-sample, mismatch and debug-window subsets; producer and oracle stream digests match |
| Retention limits | A debug window > 2 s, two windows in one cell, or physical `mode=full` beyond the cap -> `validate` rejects; mismatch count above `mismatch_cap` -> INCONCLUSIVE `mismatch_cap_exhausted` with exact per-class counts |
| Capacity/observer fault | Slow consumer cannot stall DUT invisibly; export gap latches invalidity; all buffers bounded and accounted |

The roadmap fault (c) is "part of the packets receive an extra 0.2 ms; the reporter detects it". From a zero-error baseline, +0.2 ms never exceeds the 0.5 ms p99 threshold, whatever the affected fraction. It exceeds the 0.1 ms median threshold once more than half of the main samples are affected, so the main (c) fixture uses 60% and must FAIL. c1 keeps the few-packet case and proves detection through the asserted fields, not through a verdict. Do not lower either threshold to manufacture a failure.

All three (c) fixtures use the default 1/1024 sampling on one line/direction/phase cell with at least 440,433 delivered survivors, so unsampled packets exist (section 7.2 planning target). The test computes the main sample size n and the affected main count k independently from the sampling hash and fault selector, and asserts these preconditions before it checks the report: n >= 368; for c1, `k > ceil(0.01*(n-1))` (p99 point is a faulted sample) and `U(n,0.5) <= n-k` (median upper bound is a zero sample); for the main (c), `n-k < L(n,0.5)` (median lower bound is a faulted sample); for c3, `n-k < L(n,0.99)` and `U(n,0.5) <= n-k`. L and U are the section 7.2 1-based ranks. Software scope uses u=0.

## 12. Differences from the inspected draft and handoff

The draft already supplies useful independent arithmetic, selector, probe encoding, deterministic sampling and software reporting. Preserve those where consistent. Required changes:

- Replace provisional schema IDs with this version; validate all streams/markers and full seals, not only packet counts/digest.
- Retain hardware-before time separately from engine arrival and require calibrated mapping; fake-clock equality is software-only.
- Trace timing uses modeled `release_ns`, not `deadline_ns` when the profile itself holds an outage. Actual-time stall hold does not change expected release.
- Complete terminal identities, dispatch/TX evidence and full observer coverage; do not infer wire delivery from TX accepted.
- Evaluate required cells per line/direction; keep uniform main and extra strata separate. Missing CI/clock/counter evidence cannot qualify a row.
- Align all `probe_*` fields to the true probe subset, preserve `INCONCLUSIVE -> INVALID`, and avoid unbounded O(N) joins at hardware run length.
- Add SPDX/English/environment-value checks, README accuracy workflow and these fault tests to CI/pre-push. Do not assert that documentation of these requirements means all OS1–OS6 checks already pass in the fork.

Equipment calibration, M4 capacity measurements, production identity disambiguation and final matrix/Q/trace registration remain external prerequisites. Stage 1 second-review changes must be compared against the pinned source before reporter freeze. This task changes documentation only; no runtime implementation or hardware acceptance is delivered here.

## Sources

Primary web sources checked 2026-10-29. Mathematical algorithms and resource calculations above are explicit design choices/estimates. Source code is pinned in section 1; no source implementation is copied into the engine.

[cx6]: https://networking-docs.nvidia.com/connectx6dxhw/introduction
[nvidia-ts]: https://networking-docs.nvidia.com/mlnxenswum/24070610/time-stamping
[linux-ts]: https://www.kernel.org/doc/html/latest/networking/timestamping.html
[mlx5]: https://doc.dpdk.org/guides-26.03/nics/mlx5.html
[ethdev]: https://doc.dpdk.org/guides-26.03/prog_guide/ethdev/ethdev.html
[nist]: https://www.itl.nist.gov/div898/handbook/prc/section2/prc262.htm
