<!-- SPDX-License-Identifier: Apache-2.0 -->

# Offline Garcia trace tools

These tools convert **configured delays**, not packet release order. They run
without DPDK, a kernel module, hugepages or a NIC. Python 3.11+ and NumPy 2.x are
required. The Rust engine has no Python, NumPy or DPDK dependency.

The [registered trace semantics](TRACE-SEMANTICS.md) define configured equality,
engine lookup/loss differences, canonical eligibility and terminal coverage.

## Synthetic quick start

Run from the repository root in a fresh output directory:

```sh
python3 -m venv .venv
. .venv/bin/activate
python -m pip install -r scripts/trace-requirements.txt
mkdir -p target/trace-example
python - <<'PY'
from pathlib import Path
from zipfile import ZipFile
with ZipFile('target/trace-example/synthetic.zip', 'w') as archive:
    archive.writestr('synthetic.csv', Path('tests/fixtures/garcia-synthetic.csv').read_bytes())
PY
python scripts/garcia_trace.py target/trace-example/synthetic.zip \
  target/trace-example/normalized.json \
  --start-ns 0 --duration-ns 15000000 --offset-ns 0 --queue-drain-ns 0
python scripts/compare_trace.py target/trace-example/normalized.json \
  tests/fixtures/theaterq-synthetic.csv \
  --source target/trace-example/synthetic.zip \
  --output target/trace-example/comparison.json
```

The converter creates `normalized.json` and `normalized.manifest.json`. The
comparison exits 0 with `intervals_equal=true`: three intervals, covering 15 ms.
Both original and configured samples contain five late samples in runs of
lengths 4 and 1. The missing sequence number splits those runs. All checked-in
fixtures are synthetic. Existing output files are never overwritten; use a
new output name to rerun. The example ZIP's hash can vary with archive metadata;
the converter records the actual bytes, not an assumed fixture hash.

For development, use this environment's interpreter for the complete suite:

```sh
PYTHON=python scripts/check.sh
```

CI installs the same offline requirements and runs the same script as the hook.
SciPy and pytest are not converter or CI dependencies.

## Conversion CLI and input validation

```text
python scripts/garcia_trace.py SOURCE.zip OUTPUT.json
    --start-ns INTEGER --duration-ns INTEGER
    (--offset-ns INTEGER | --corridor A|B)
    [--seed INTEGER] [--late-threshold-ns INTEGER]
    [--queue-drain-ns INTEGER] [--hold-delta-ns INTEGER]
```

- `SOURCE.zip`: one unencrypted CSV member. Expected header:
  `seq_nr,ul_t0,owd_t0_t1` or `seq_nr,dl_t0,owd_t0_t1`. Direction comes from the
  header; both timestamps and OWD are already nanoseconds, not RTT/2.
- `--start-ns`: source-relative start, not UTC. `--duration-ns`: positive, at
  most 45 s, wholly inside source coverage. Neither phase detection nor automatic
  segment/pair selection is provided. Choose paired directions and their common
  offset through the experiment's registered selection procedure.
- `--offset-ns`: required signed propagation offset. Zero preserves native OWD.
  It is added **after** native 1,000 ns latency quantization; it is not rounded
  again. Alternatively, `--corridor A` computes `-11 + 4 + 52.6 = +45.6 ms`;
  `--corridor B` computes `-11 + 4 + 113.8 = +106.8 ms`. These registered exact
  values replace the earlier whole-millisecond approximations. The recipe records
  all three formula components. The two options are mutually exclusive. Negative resulting
  delay or timestamp overflow is rejected.
- `--seed`: default 1. The NumPy PCG64 stream draws once for **every source
  sample pair**, before cropping; changing the crop does not restart the RNG.
- `--late-threshold-ns`: default 20,000,000 ns. This is the diagnostic threshold
  used below, not a parameter that inserts late packets into the trace.
- `--queue-drain-ns`: explicitly register terminal HOLD for replay. The extension
  is `max(1, t_max + hold_delta + queue_drain)` ns. `t_max` is computed from the
  actual converted rows including propagation offset. `--hold-delta-ns` defaults
  to zero and only enlarges the coverage budget. It never inserts late packets.
  The recipe records each component and extension; the profile records the
  finite horizon. Without a queue budget, conversion is diagnostic-only with
  terminal error. A real replay must declare its actual maximum queue drain.

The ZIP and uncompressed member are each limited to 128 MiB per run. At least
two samples are required. Noninteger/negative values, duplicate sequence numbers,
nonincreasing send timestamps after sequence sorting, unsupported headers, bad
CRC, encryption and multiple members fail. Source integers must not exceed
2^53 so the reference float64 conversion is meaningful. Missing sequence numbers
are counted, never converted to primary-profile packet loss.

A selected window intersecting a received-sample gap greater than 0.5 s fails.
This is the registered canonical-pool exclusion rule; it never infers an outage.

## Canonical paired source pool

```sh
python scripts/garcia_pool.py source-directory pool.json --duration-ns 11000000000
```

The directory must contain paired `STAMP__ul.csv.zip` / `STAMP__dl.csv.zip` files.
The selector derives the common phase from DL with the registered detector,
excludes runs without a phase, and lists natural starts `phase + 15*k` with
0.5 s source margins. Windows intersecting a >0.5 s received-sample gap in either
direction are excluded; exactly 0.5 s is not excluded by that test. Invalid,
unpaired and insufficient-coverage inputs have explicit exclusion reasons.

`pool.json` uses `schema=garcia-canonical-pool-v1`: `candidates` lists shared
start/duration/phase and both ZIP/member hashes; `excluded` retains reasons and
available hashes. No source samples or absolute directory paths are embedded.
Choose a candidate using the experiment's registered repetition seed, preserve
the choice in its run manifest, and convert both directions at that same start.
An empty candidate list is an empty pool, not validation success. Direct
conversion remains available for synthetic/diagnostic windows and does not by
itself certify canonical membership or wall-clock phase alignment.

## Registered numerical recipe

Version `garcia-interval-v1` independently implements the equations documented in
the [inventory](reports/m3-trace-inventory.md). No reference implementation code
is imported or copied.

1. Estimate local least-squares slope from the preceding 5 ms since the latest
   step, using at least three samples. Clip to +/-1 ms/ms.
2. Detect a step when the next delay deviates from that trend by at least 1 ms
   (with the registered 1e-9 ms numerical tolerance). Choose its switch uniformly
   between the samples. Both sides follow the same trend; other pairs interpolate.
3. Raise extrapolated delay to the lowest received delay within 1 s around the
   pair. The floor changes neither the step detector nor the source samples.
4. Subdivide sloping pieces using `ceil(change / 0.05 ms - 1e-9)`, without a cap.
   Include source and switch boundaries. Round absolute boundaries to integer
   microseconds using ties-to-even, then evaluate delay at each rounded midpoint.
5. Round native latency to 1,000 ns using ties-to-even, then add the explicit
   offset. Merge equal-delay neighbors only when the logical step segment is the
   same, preserving row boundaries across reference route changes. This is row
   bookkeeping; it adds no route FIFO policy to the engine.

The recipe records requested and effective rounded start/end, seed, quantums,
offset application, implementation SHA-256 and Python/NumPy versions. Original
sample ns remain integers for diagnostics. The output's original epoch is zero;
`at_ns` is relative to the rounded window origin. A sub-microsecond window that
vanishes after rounding fails. As in the reference, the floating end is computed
by adding separately converted start and duration; addition order matters at
half-microsecond ties.

## Output and provenance

`OUTPUT.json` follows [hng-trace-v1](HNG-API.md#trace-files) without extra fields.
`conversion` is a JSON-encoded recipe string, `source_sha256` hashes the exact
ZIP bytes parsed, and `source_id` is its basename. Preserve that basename when
supplying the source for comparison. The sidecar includes:

- `schema=garcia-conversion-manifest-v1`, the parsed `recipe`;
- `source`: ZIP/member basenames and SHA-256 values, direction, full-source sample
  and missing-sequence counts, source DOI and CC-BY-4.0 attribution reference;
- `normalized_sha256`: hash of the emitted normalized file;
- `interval_count`, floor-raised interval count and duration;
- `lateness`: original/configured run histograms and differences, as defined below.

No absolute input or output path is written into either artifact. Runtime files
remain the caller's responsibility and should be kept outside version control.
The synthetic CSVs have the repository's Apache-2.0 license. Measured Garcia data
has its own [CC BY 4.0 license](https://zenodo.org/records/16275284); retain source
credit, license link and conversion notices if distributing derived data. No
measured raw or normalized trace is bundled. Other-source licenses and separate
code-license concerns are recorded in the inventory.

The delay-only profile sets loss to zero and outage flags to false. Its required
`outage_action=drop` is inactive because no down interval exists. Registered replay
uses finite terminal HOLD; it is distinct from an outage hold. The comparator
checks the recorded horizon against the actual maximum delay and t+delta+Q
registration. Coverage does not replace rate/storage validation or permit
admission/drain beyond the declared half-open horizon.

The separately registered `tests/fixtures/outage-drop.json` and
`tests/fixtures/outage-hold.json` are synthetic input profiles, not Garcia
conversion outputs. Their source, numeric bounds and expected releases are
registered in [TRACE-SEMANTICS.md](TRACE-SEMANTICS.md#synthetic-outage-registration).
`tests/registered_outages.rs` validates engine loading, release/drop counters,
boundary gates and finite terminal coverage without DPDK or a NIC.

## Configured-row comparison

```text
python scripts/compare_trace.py NORMALIZED.json THEATERQ.csv
    --source SOURCE.zip --output REPORT.json
```

Supply the **unshifted native** extended TheaterQ rows generated independently
from the same source window and seed. The tool adds the normalized recipe's
declared offset to their native LATENCY, without requantizing. It compares every
row start, latency, loss and outage value and the final coverage. Equal delays
with distinct reference route IDs retain distinct rows. Rate, queue limit and
route IDs have no corresponding fields in an interval-delay profile and are
not claimed equal. Nonzero JITTER, LOSS or duplication are unsupported and fail;
the tool does not silently interpret them as delay-only input.

Exit codes: **0** exact configured equality; **1** a reported row/coverage
mismatch; **2** invalid input or an output-file conflict. Duplicate/nonfinite
JSON values, source hash/direction/identity mismatch and unsupported profile
fields fail. `REPORT.json` uses `schema=hng-trace-comparison-v1` and contains row
counts, coverage equality, first mismatch, offset and input hashes.

`intervals_equal` does not assert identical deadlines, loss placement under load,
serialization, FIFO clamp, release order or hardware precision.

## Actual scheduler replay and independent oracle

```sh
cargo build --locked --bin trace_replay
python scripts/replay_trace.py normalized.json --source source.zip \
  --runner target/debug/trace_replay --output replay-check.json
```

`trace_replay` is an offline HngBuffer executable. It reads one JSON request
from stdin (`model`, optional `trace` profile, `packets` with unique nonzero
`id`, `arrival_ns`, `length`) and emits actual scheduler records as a JSON array:
`id`, ingress ordinal, service completion, deadline, release, clamp extra and
late flag. It advances virtual time; it does not perform packet I/O or wire
timestamping. Nonaccepted packets fail this no-drop diagnostic explicitly.

The Python checker samples the converted profile at original received-packet
times, runs the executable, and compares every emitted record with an independent
`Fraction` service/deadline oracle. The oracle does not import Rust or call its
selector/scheduling functions. It checks the lookup at rounded service completion
and the global retained-deadline clamp. The default test traffic is nonqueueing,
with 100 ns service and sufficient storage. Mismatches fail instead of becoming
timing tolerances. Output includes clamp count/maximum and unexpected reordering.

CI always runs converted synthetic downward-step replay. The measured-data test
requires an external, unchanged figure-1 DL ZIP (not redistributed in CI):

```sh
REAL_GARCIA_ZIP=source.zip python -B -m unittest discover -s tests -p test_replay.py
```

That test executes the 38-49 s source window, checks more than 40,000 packets and
requires an exercised downward-step clamp with zero unexpected reordering.
Without that external input, only that measured-data test is explicitly skipped;
the synthetic engine replay remains mandatory. Software replay is not G2.

## Late-run distribution comparison

Both tools report `lateness` using the diagnostic from the source research
report, rather than replacing the correlated trace with independent markings:

- Bin the **full source** by 15 s windows anchored to its first send timestamp.
  Each received sample's baseline is the median native OWD in that bin.
- A source sample is late when `OWD - baseline > threshold_ns`, strictly greater.
  No late value is imputed for a missing sequence number.
- Evaluate the configured intervals at the same received source send times in
  the selected rounded window. Compare against the **same** source baseline
  plus the declared offset. This isolates conversion changes and avoids fitting
  a new baseline to an incomplete crop. It models configured delays only.
  Remove the integer offset before float conversion so large offsets cannot
  change a threshold classification through floating-point cancellation.
- Consecutive late samples join a run only if sequence numbers differ by one
  and the send gap is no greater than three times the full-source median interval.
  Crops and files never join. Missing replies cannot lengthen a late run.

Each `source`/`configured` result includes `n_samples`, `n_late`, `n_runs`, a
histogram mapping packet-run length to count, span p50/p95/p99 and maximum in ns,
connected late-pair numerator and denominator, and `p_next_late_given_late`.
The conditional probability is null when its denominator is zero; an empty
window has zero observed samples, not evidence of zero population lateness.
`late_flag_mismatches` and `histograms_equal` compare the two configurations.
The comparison tool reports both `hng_lateness` and `theaterq_lateness`, each
against the matching source window. Different coverage uses different explicitly
reported sample counts and cannot establish histogram equivalence.

Midpoint sampling and latency rounding can change a few threshold classifications
even when the HNG and TheaterQ rows are identical. The reports retain those
differences. No numerical tolerance for run-length fidelity has been registered;
the tools do not turn descriptive statistics into an acceptance claim. They
also do not equate packet-run lengths across different probe cadences.
