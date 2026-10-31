#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Independent Garcia OWD conversion; configured intervals, never release order."""

import argparse
from collections import deque
import csv
from dataclasses import dataclass
import hashlib
import io
import json
from pathlib import Path
import platform
import sys
import zipfile

import numpy as np

U64_MAX = 2**64 - 1
VERSION = "garcia-interval-v1"
POP_DELAY_NS = {"A": 52_600_000, "B": 113_800_000}


def corridor_offset(corridor):
    if corridor not in POP_DELAY_NS:
        raise ValueError("corridor must be A or B")
    return POP_DELAY_NS[corridor] - 11_000_000 + 4_000_000


def detect_phase(source):
    delays = source.owd_ns / 1e6
    spikes = delays > np.median(delays) + 15.0
    if int(np.count_nonzero(spikes)) < 21:
        return None
    histogram, edges = np.histogram((source.send_ns[spikes] / 1e9) % 15.0,
                                   bins=300, range=(0, 15.0))
    scores = histogram + np.roll(histogram, -1) + np.roll(histogram, 1)
    return round(float(edges[int(np.argmax(scores))]) * 1e9)


def window_has_gap(source, start_ns, duration_ns):
    gaps = np.flatnonzero(np.diff(source.send_ns) > 500_000_000)
    return any(source.send_ns[i] < start_ns + duration_ns and source.send_ns[i + 1] > start_ns
               for i in gaps)


@dataclass
class Source:
    seq: np.ndarray
    send_ns: np.ndarray
    owd_ns: np.ndarray
    direction: str
    filename: str
    member: str
    sha256: str
    member_sha256: str
    expected_first_seq: int | None = None
    expected_last_seq: int | None = None

    @property
    def missing_internal(self):
        return int(np.sum(np.diff(self.seq) - 1))

    @property
    def missing_before(self):
        return None if self.expected_first_seq is None else int(self.seq[0]) - self.expected_first_seq

    @property
    def missing_after(self):
        return None if self.expected_last_seq is None else self.expected_last_seq - int(self.seq[-1])

    @property
    def missing(self):
        if self.missing_before is None or self.missing_after is None:
            return None
        return self.missing_before + self.missing_internal + self.missing_after

    @property
    def median_interval_ns(self):
        return float(np.median(np.diff(self.send_ns)))


def read_source(path, *, expected_first_seq=None, expected_last_seq=None):
    """Validate one bounded CSV member and hash the exact ZIP bytes parsed."""
    path = Path(path)
    if path.stat().st_size > 128 * 1024**2:
        raise ValueError("source ZIP exceeds the 128 MiB per-run limit")
    payload = path.read_bytes()
    try:
        with zipfile.ZipFile(io.BytesIO(payload)) as archive:
            members = archive.infolist()
            if len(members) != 1 or members[0].is_dir() or not members[0].filename.endswith(".csv"):
                raise ValueError("source must contain exactly one CSV member")
            member = members[0]
            if member.file_size > 128 * 1024**2 or member.flag_bits & 1:
                raise ValueError("source CSV is too large or encrypted")
            raw = archive.read(member)
    except (zipfile.BadZipFile, RuntimeError, NotImplementedError) as error:
        raise ValueError("invalid or unsupported ZIP") from error
    reader = csv.reader(io.StringIO(raw.decode("utf-8")))
    header = next(reader, None)
    if header not in (["seq_nr", "ul_t0", "owd_t0_t1"], ["seq_nr", "dl_t0", "owd_t0_t1"]):
        raise ValueError("expected seq_nr,ul_t0|dl_t0,owd_t0_t1 header")
    records = []
    for row in reader:
        if len(row) != 3 or any(not value.isascii() or not value.isdecimal() for value in row):
            raise ValueError("source rows must contain three nonnegative decimal integers")
        values = tuple(map(int, row))
        if max(values) > 2**53:
            raise ValueError("source integers exceed exact float64 conversion range")
        records.append(values)
    if len(records) < 2:
        raise ValueError("source needs at least two received samples")
    records.sort(key=lambda row: row[0])
    seq, sent, delay = np.asarray(records, dtype=np.int64).T
    if np.any(np.diff(seq) <= 0) or np.any(np.diff(sent) <= 0):
        raise ValueError("duplicate sequence or nonincreasing send timestamps")
    if any(bound is not None and (type(bound) is not int or not 0 <= bound <= U64_MAX)
           for bound in (expected_first_seq, expected_last_seq)):
        raise ValueError("expected sequence bounds must be nonnegative u64 integers")
    if ((expected_first_seq is not None and expected_first_seq > int(seq[0])) or
            (expected_last_seq is not None and expected_last_seq < int(seq[-1]))):
        raise ValueError("expected sequence bounds exclude received samples")
    return Source(seq, sent, delay, header[1][:2], path.name, Path(member.filename).name,
                  hashlib.sha256(payload).hexdigest(), hashlib.sha256(raw).hexdigest(),
                  expected_first_seq, expected_last_seq)


class DelayCurve:
    """Trend steps and interpolation reconstructed from the registered equations."""

    def __init__(self, source, seed):
        self.times = source.send_ns / 1e9
        self.delay = source.owd_ns / 1e6
        relative = self.times - self.times[0]
        sums = [np.r_[0.0, np.cumsum(values)] for values in
                (relative, self.delay, relative * relative, relative * self.delay)]
        left_edges = np.searchsorted(relative, relative - 0.005)
        self.slopes = np.zeros(len(relative) - 1)
        self.pair_switch = np.full(len(relative) - 1, np.nan)
        draws = np.random.Generator(np.random.PCG64(seed)).random(len(relative) - 1)
        since_step = 0
        for index in range(len(relative) - 1):
            left, right = max(int(left_edges[index]), since_step), index + 1
            count = right - left
            gradient = 0.0
            if count >= 3:
                x, y, xx, xy = (float(prefix[right] - prefix[left]) for prefix in sums)
                denominator = count * xx - x * x
                if denominator > 0:
                    gradient = (count * xy - x * y) / denominator
            gradient = min(1000.0, max(-1000.0, gradient))
            self.slopes[index] = gradient
            predicted = self.delay[index] + gradient * (relative[index + 1] - relative[index])
            if abs(self.delay[index + 1] - predicted) >= 1.0 - 1e-9:
                self.pair_switch[index] = self.times[index] + draws[index] * (
                    self.times[index + 1] - self.times[index])
                since_step = index + 1
        self.switches = self.pair_switch[np.isfinite(self.pair_switch)]
        # Sliding range minima include both ends of each pair and one second around it.
        candidates = deque()
        inserted = 0
        self.floors = np.empty(len(relative) - 1)
        for index in range(len(self.floors)):
            while inserted < len(self.times) and self.times[inserted] <= self.times[index + 1] + 1.0:
                while candidates and self.delay[candidates[-1]] >= self.delay[inserted]:
                    candidates.pop()
                candidates.append(inserted)
                inserted += 1
            while self.times[candidates[0]] < self.times[index] - 1.0:
                candidates.popleft()
            self.floors[index] = self.delay[candidates[0]]

    def evaluate(self, times):
        indices = np.clip(np.searchsorted(self.times, times, side="right") - 1, 0, len(self.slopes) - 1)
        values = np.interp(times, self.times, self.delay)
        switching = self.pair_switch[indices]
        in_pair = np.isfinite(switching) & (times >= self.times[indices]) & (times <= self.times[indices + 1])
        # Match the registered ms/ms slope storage and its conversion back to ms/s.
        gradient = (self.slopes[indices] / 1000.0) * 1000.0
        projected = self.delay[indices] + gradient * (times - self.times[indices])
        jump = self.delay[indices + 1] - (
            self.delay[indices] + gradient * (self.times[indices + 1] - self.times[indices]))
        values = np.where(in_pair, projected + np.where(times < switching, 0.0, jump), values)
        raised = values < self.floors[indices]
        values = np.maximum(values, self.floors[indices])
        routes = np.searchsorted(self.switches, times, side="right")
        return values, routes, raised

    def intervals(self, start_ns, duration_ns, offset_ns):
        start = start_ns / 1e9
        end = start + duration_ns / 1e9
        boundaries = [start, end, *self.switches]
        for index, (left, right) in enumerate(zip(self.times[:-1], self.times[1:])):
            if right < start or left > end:
                continue
            switch = self.pair_switch[index]
            pieces = [(left, right, abs(self.delay[index + 1] - self.delay[index]))]
            if np.isfinite(switch):
                gradient = abs((self.slopes[index] / 1000.0) * 1000.0)
                pieces = [(left, switch, gradient * (switch - left)),
                          (switch, right, gradient * (right - switch))]
            for begin, finish, change in pieces:
                divisions = max(1, int(np.ceil(change / 0.01 - 1e-9)))
                boundaries.extend(begin + (finish - begin) * np.arange(divisions) / divisions)
        microseconds = np.unique(np.round(np.asarray(boundaries) * 1e6)).astype(np.int64)
        microseconds = microseconds[(microseconds >= round(start * 1e6)) &
                                    (microseconds <= round(end * 1e6))]
        if len(microseconds) < 2:
            raise ValueError("window vanishes after microsecond boundary rounding")
        centers = (microseconds[:-1] + microseconds[1:]) / 2e6
        delays, routes, raised = self.evaluate(centers)
        if np.any(delays < 0):
            raise ValueError("negative native delay")
        rows, previous_route = [], None
        for index, (delay, route) in enumerate(zip(delays, routes)):
            latency = round(float(delay) * 1e6 / 1000) * 1000 + offset_ns
            if not 0 <= latency <= U64_MAX:
                raise ValueError("offset produces negative or overflowing latency")
            if rows and rows[-1]["t_ns"] == latency and route == previous_route:
                continue
            rows.append({"at_ns": int(microseconds[index] - microseconds[0]) * 1000,
                         "t_ns": latency, "loss": "0", "outage": False})
            previous_route = route
        return rows, int(microseconds[0]) * 1000, int(microseconds[-1]) * 1000, {
            "floor_raised_intervals": int(np.count_nonzero(raised)),
            "floor_raised_ns": int(np.diff(microseconds)[raised].sum()) * 1000,
        }


def late_runs(seq, send_ns, late, median_interval_ns):
    """Observable contiguous late packets; missing samples always break a run."""
    connected = (np.diff(seq) == 1) & (np.diff(send_ns) <= 3 * median_interval_ns)
    starts = np.flatnonzero(late & np.r_[True, ~(late[:-1] & connected)])
    ends = np.flatnonzero(late & np.r_[~(late[1:] & connected), True])
    lengths = ends - starts + 1
    values, counts = np.unique(lengths, return_counts=True)
    spans = send_ns[ends] - send_ns[starts]
    numerator = int(np.count_nonzero(late[:-1] & late[1:] & connected))
    denominator = int(np.count_nonzero(late[:-1] & connected))
    return {"n_samples": len(seq), "n_late": int(late.sum()), "n_runs": len(starts),
            "histogram": {str(int(value)): int(count) for value, count in zip(values, counts)},
            "span_quantiles_ns": np.quantile(spans, [0.5, 0.95, 0.99]).tolist() if len(spans) else None,
            "max_span_ns": int(spans.max()) if len(spans) else 0,
            "late_pair_numerator": numerator, "late_pair_denominator": denominator,
            "p_next_late_given_late": numerator / denominator if denominator else None}


def lateness(source, document, threshold_ns):
    """Evaluate configured delays at received source timestamps, without FIFO."""
    recipe = json.loads(document["conversion"])
    start, end = recipe["effective_start_ns"], recipe["effective_end_ns"]
    bins = (source.send_ns - source.send_ns[0]) // 15_000_000_000
    baseline = np.empty(len(bins))
    for window in np.unique(bins):
        baseline[bins == window] = np.median(source.owd_ns[bins == window])
    selected = (source.send_ns >= start) & (source.send_ns < end)
    seq, sent = source.seq[selected], source.send_ns[selected]
    rows = document["profile"]["rows"]
    indices = np.searchsorted([row["at_ns"] for row in rows], sent - start, side="right") - 1
    configured = np.asarray([row["t_ns"] - recipe["offset_ns"] for row in rows], dtype=np.float64)[indices]
    native_late = source.owd_ns[selected] - baseline[selected] > threshold_ns
    configured_late = configured - baseline[selected] > threshold_ns
    source_stats = late_runs(seq, sent, native_late, source.median_interval_ns)
    configured_stats = late_runs(seq, sent, configured_late, source.median_interval_ns)
    return {"baseline": "full-source-median15s-first-send-anchor", "threshold_ns": threshold_ns,
            "comparison": "configured delays at identical received source samples; no release model",
            "source": source_stats, "configured": configured_stats,
            "late_flag_mismatches": int(np.count_nonzero(native_late != configured_late)),
            "histograms_equal": source_stats["histogram"] == configured_stats["histogram"]}


def convert(path, start_ns, duration_ns, offset_ns=0, seed=1, late_threshold_ns=20_000_000, *,
            corridor=None, queue_drain_ns=None, delta_ns=0, pool_path=None, pool_index=None,
            expected_first_seq=None, expected_last_seq=None):
    source = read_source(path, expected_first_seq=expected_first_seq, expected_last_seq=expected_last_seq)
    if any(type(value) is not int for value in (start_ns, duration_ns, offset_ns, seed, late_threshold_ns)):
        raise ValueError("conversion parameters must be integers")
    if type(delta_ns) is not int or not 0 <= delta_ns <= U64_MAX:
        raise ValueError("invalid delta drain allowance")
    if queue_drain_ns is None and delta_ns != 0:
        raise ValueError("delta allowance needs an explicit queue drain budget")
    if queue_drain_ns is not None and (type(queue_drain_ns) is not int or not 0 <= queue_drain_ns <= U64_MAX):
        raise ValueError("invalid queue drain budget")
    propagation = None
    if corridor is not None:
        if offset_ns != 0:
            raise ValueError("choose a corridor or an explicit offset, not both")
        offset_ns = corridor_offset(corridor)
        propagation = {"corridor": corridor, "baseline_ns": 11_000_000,
                       "geometry_ns": 4_000_000, "pop_delay_ns": POP_DELAY_NS[corridor]}
    if start_ns < source.send_ns[0] or not 0 < duration_ns <= 45_000_000_000:
        raise ValueError("window must start inside the source and last at most 45 seconds")
    if start_ns + duration_ns > source.send_ns[-1] or seed < 0 or late_threshold_ns < 0:
        raise ValueError("invalid coverage, seed or late threshold")
    if window_has_gap(source, start_ns, duration_ns):
        raise ValueError("selected window spans a measurement gap excluded from the canonical pool")
    phase = detect_phase(source) if source.direction == "dl" else None
    pool_item = None
    if pool_path is not None or pool_index is not None:
        if pool_path is None or pool_index is None:
            raise ValueError("canonical conversion needs both pool manifest and index")
        from garcia_pool import validate_selection
        phase, pool_item = validate_selection(source, path, start_ns, duration_ns, pool_path, pool_index)
    curve = DelayCurve(source, seed)
    rows, begin, finish, stats = curve.intervals(start_ns, duration_ns, offset_ns)
    if pool_item and (begin != start_ns or finish != start_ns + duration_ns):
        raise ValueError("canonical window changes after boundary quantization; use a registered microsecond-aligned pool")
    if any(row["t_ns"] + finish - begin > U64_MAX for row in rows):
        raise ValueError("delay plus end overflows engine timestamps")
    drain_budget, horizon = None, None
    if queue_drain_ns is not None:
        maximum = max(row["t_ns"] for row in rows)
        extension = max(1, maximum + delta_ns + queue_drain_ns)
        horizon = finish - begin + extension
        if horizon > U64_MAX:
            raise ValueError("terminal HOLD horizon overflows engine timestamps")
        drain_budget = {"t_max_ns": maximum, "delta_ns": delta_ns,
                        "queue_drain_ns": queue_drain_ns, "extension_ns": extension}
    recipe = {"version": VERSION, "implementation_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
              "numpy_version": np.__version__, "python_version": platform.python_version(),
              "start_ns": start_ns, "duration_ns": duration_ns, "effective_start_ns": begin,
              "effective_end_ns": finish, "offset_ns": offset_ns,
              "propagation": propagation,
              "phase_ns": phase, "pool_item": pool_item,
              "eligibility": "canonical" if pool_item else "diagnostic",
              "drain_budget": drain_budget,
              "offset_application": "after native 1000 ns latency quantization", "seed": seed,
              "rng": "numpy-PCG64-full-source-pairs", "step_ms": 1, "trend_window_ns": 5_000_000,
              "slope_clip_ms_per_ms": 1, "floor_window_ns": 1_000_000_000,
              "adaptive_ns": 10000, "boundary_quantum_ns": 1000, "latency_quantum_ns": 1000,
              "missingness": "unknown unregistered endpoints; no loss inference; sequence gaps break diagnostic runs",
              "expected_sequence_bounds": {"first": expected_first_seq, "last": expected_last_seq},
              "late_threshold_ns": late_threshold_ns}
    document = {"schema": "hng-trace-v1", "source_id": source.filename, "direction": source.direction,
                "units": "ns_owd", "conversion": json.dumps(recipe, sort_keys=True),
                "source_sha256": source.sha256, "splice_policy": "natural_hold",
                "trace_epoch_ns": 0, "time_scale": 1,
                "profile": {"end_ns": finish - begin, "terminal": "hold_last_sample" if horizon else "error",
                            "terminal_horizon_ns": horizon,
                            "loss_seed": 0, "outage_action": "drop", "rows": rows}}
    manifest = {"schema": "garcia-conversion-manifest-v1", "recipe": recipe,
                "source": {"filename": source.filename, "member": source.member, "sha256": source.sha256,
                           "member_sha256": source.member_sha256, "direction": source.direction,
                           "samples": len(source.seq), "missing": source.missing,
                           "missing_internal": source.missing_internal, "missing_before": source.missing_before,
                           "missing_after": source.missing_after,
                           "missing_runs": int(np.count_nonzero(np.diff(source.seq) > 1)),
                           "license": "CC-BY-4.0", "dataset": "https://doi.org/10.5281/zenodo.16275284"},
                "interval_count": len(rows), **stats, "lateness": lateness(source, document, late_threshold_ns)}
    return document, manifest


def write_json(path, value):
    """Create new evidence without replacing an existing artifact."""
    with Path(path).open("x", encoding="utf-8") as output:
        json.dump(value, output, indent=2, allow_nan=False)
        output.write("\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--start-ns", type=int, required=True)
    parser.add_argument("--duration-ns", type=int, required=True)
    propagation = parser.add_mutually_exclusive_group(required=True)
    propagation.add_argument("--offset-ns", type=int, default=0)
    propagation.add_argument("--corridor", choices=sorted(POP_DELAY_NS))
    parser.add_argument("--seed", type=int, default=1)
    parser.add_argument("--late-threshold-ns", type=int, default=20_000_000)
    parser.add_argument("--queue-drain-ns", type=int, help="register finite terminal HOLD with this Q drain budget")
    parser.add_argument("--hold-delta-ns", type=int, default=0, help="additional drain allowance; does not add late packets")
    parser.add_argument("--pool", type=Path, help="registered canonical pool manifest")
    parser.add_argument("--pool-index", type=int, help="zero-based candidate index")
    parser.add_argument("--expected-first-seq", type=int, help="independently registered first sent sequence, inclusive")
    parser.add_argument("--expected-last-seq", type=int, help="independently registered last sent sequence, inclusive")
    args = parser.parse_args()
    sidecar = args.output.with_suffix(".manifest.json")
    try:
        if args.output == sidecar or args.output.exists() or sidecar.exists():
            raise ValueError("output or manifest already exists")
        document, manifest = convert(args.source, args.start_ns, args.duration_ns, args.offset_ns,
                                     args.seed, args.late_threshold_ns, corridor=args.corridor,
                                     queue_drain_ns=args.queue_drain_ns, delta_ns=args.hold_delta_ns,
                                     pool_path=args.pool, pool_index=args.pool_index,
                                     expected_first_seq=args.expected_first_seq, expected_last_seq=args.expected_last_seq)
        write_json(args.output, document)
        manifest["normalized_sha256"] = hashlib.sha256(args.output.read_bytes()).hexdigest()
        write_json(sidecar, manifest)
        print(json.dumps({"intervals": manifest["interval_count"], "drain_budget": manifest["recipe"]["drain_budget"],
                          "lateness": manifest["lateness"]}))
    except (ValueError, OSError, UnicodeError) as error:
        parser.exit(2, f"conversion failed: {error}\n")


if __name__ == "__main__":
    main()
