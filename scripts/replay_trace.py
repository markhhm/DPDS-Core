#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Verify actual HngBuffer records against an independent rational deadline oracle."""

import argparse
from bisect import bisect_right
from fractions import Fraction
import json
from pathlib import Path
import subprocess

import numpy as np

from compare_trace import strict_json, validate_document
from garcia_trace import late_runs, read_source, write_json


def selected(seed, ordinal, probability):
    mask = 2**64 - 1
    word = (seed + (ordinal + 1) * 0x9E3779B97F4A7C15) & mask
    for shift, multiplier in ((30, 0xBF58476D1CE4E5B9), (27, 0x94D049BB133111EB)):
        word = ((word ^ (word >> shift)) * multiplier) & mask
    word ^= word >> 31
    return word < (Fraction(probability) * 2**64).__floor__()


def oracle(model, trace, packets):
    """Independent no-drop reference; reject offered traffic that would queue/drop."""
    finish, floor = Fraction(0), 0
    result = []
    if trace and any(row["outage"] or row["loss"] != "0" for row in trace["rows"]):
        raise ValueError("delay-only replay does not reconstruct loss/outages")
    starts = [row["at_ns"] for row in trace["rows"]] if trace else []
    for ordinal, packet in enumerate(packets):
        arrival = packet["arrival_ns"]
        l2 = max(packet["length"] + 4, 64)
        if l2 > model["q_bytes"] or arrival < finish:
            raise ValueError("nonqueueing reference requires sufficient C/Q")
        finish = Fraction(arrival) + Fraction((l2 + 20) * 8_000_000_000, model["c_bps"])
        service = -(-finish.numerator // finish.denominator)
        delay = model["t_ns"]
        if trace:
            index = bisect_right(starts, service) - 1
            delay = trace["rows"][index]["t_ns"]
            horizon = trace["terminal_horizon_ns"] if trace["terminal"] == "hold_last_sample" else trace["end_ns"]
            if service >= horizon:
                raise ValueError("service outside registered trace coverage")
        late = selected(model["seed"], ordinal, model["p"])
        raw = service + delay + (model["delta_ns"] if late else 0)
        deadline = max(raw, floor) if trace else raw
        if trace:
            if deadline >= horizon:
                raise ValueError("deadline outside registered trace coverage")
            floor = deadline
        result.append({"id": packet["id"], "ingress_seq": ordinal, "service_ns": service,
                       "deadline_ns": deadline, "release_ns": deadline,
                       "clamp_extra_ns": deadline - raw, "late": late})
    return sorted(result, key=lambda event: (event["release_ns"], event["ingress_seq"]))


def run_engine(model, trace, packets, runner):
    request = {"model": model, "trace": trace, "packets": packets}
    run = subprocess.run([str(runner)], input=json.dumps(request), text=True, capture_output=True, check=True)
    return strict_json(run.stdout)


def check_events(expected, actual):
    if expected != actual:
        raise ValueError("engine records differ from independent service/deadline/order oracle")


def measure_lateness(seq, arrivals, original, baseline, actual, offset_ns, threshold_ns, cadence):
    ordered = sorted(actual, key=lambda event: event["ingress_seq"])
    realized = np.array([event["release_ns"] - int(arrival) - offset_ns
                         for event, arrival in zip(ordered, arrivals)], dtype=np.float64)
    source_stats = late_runs(seq, arrivals, original - baseline > threshold_ns, cadence)
    engine_stats = late_runs(seq, arrivals, realized - baseline > threshold_ns, cadence)
    first, second = source_stats["histogram"], engine_stats["histogram"]
    cdf_first = cdf_second = distance = 0.0
    for length in sorted(set(first) | set(second), key=int):
        cdf_first += first.get(length, 0) / max(source_stats["n_runs"], 1)
        cdf_second += second.get(length, 0) / max(engine_stats["n_runs"], 1)
        distance = max(distance, abs(cdf_first - cdf_second))
    p_source = source_stats["p_next_late_given_late"]
    p_engine = engine_stats["p_next_late_given_late"]
    return {"status": "PROPOSAL_PENDING", "ordering_basis": "original ingress/send order",
            "engine_delay": "release_ns-arrival_ns; includes C service and clamp; offset removed exactly",
            "threshold_ns": threshold_ns, "source": source_stats, "engine": engine_stats,
            "run_length_cdf_distance": distance,
            "late_share_difference_pp": abs(engine_stats["n_late"] - source_stats["n_late"]) * 100 / max(len(seq), 1),
            "p_next_difference_pp": None if p_source is None or p_engine is None else abs(p_source - p_engine) * 100}


def verify_trace(source, document, runner):
    recipe = validate_document(document, source)
    selected = (source.send_ns >= recipe["effective_start_ns"]) & (source.send_ns < recipe["effective_end_ns"])
    arrivals = source.send_ns[selected] - recipe["effective_start_ns"]
    packets = [{"id": i + 1, "arrival_ns": int(arrival), "length": 76} for i, arrival in enumerate(arrivals)]
    capacity = max(512, len(packets) + 1)
    model = {"c_bps": 8_000_000_000, "q_bytes": 80, "t_ns": 0, "delta_ns": 0, "p": "0", "seed": 1,
             "storage_capacity_packets": capacity, "storage_capacity_bytes": capacity * 80}
    expected = oracle(model, document["profile"], packets)
    actual = run_engine(model, document["profile"], packets, runner)
    check_events(expected, actual)
    bins = (source.send_ns - source.send_ns[0]) // 15_000_000_000
    baseline = np.empty(len(bins))
    for window in np.unique(bins):
        baseline[bins == window] = np.median(source.owd_ns[bins == window])
    realized = measure_lateness(source.seq[selected], arrivals, source.owd_ns[selected], baseline[selected],
                                actual, recipe["offset_ns"], recipe["late_threshold_ns"], source.median_interval_ns)
    return {"schema": "hng-replay-check-v1", "scope": "nonqueueing virtual scheduler, not wire/G2",
            "source_sha256": source.sha256, "packets": len(actual), "deadline_mismatches": 0,
            "unexpected_reordering": int(np.count_nonzero(np.diff([event["ingress_seq"] for event in actual]) < 0)),
            "clamp_packets": sum(event["clamp_extra_ns"] > 0 for event in actual),
            "clamp_max_ns": max((event["clamp_extra_ns"] for event in actual), default=0),
            "realized_lateness": realized}


def verify_fixed(runner, probability, count=10_000, delta_ns=30_000_000, seed=1):
    if not 0 <= Fraction(probability) <= 1 or not 0 < count <= 1_000_000 or delta_ns < 0:
        raise ValueError("invalid fixed-mode probability, count or delta")
    arrivals = np.arange(count, dtype=np.int64) * 100_000
    packets = [{"id": i + 1, "arrival_ns": int(arrival), "length": 76} for i, arrival in enumerate(arrivals)]
    model = {"c_bps": 8_000_000_000, "q_bytes": 80, "t_ns": 20_000_000, "delta_ns": delta_ns,
             "p": probability, "seed": seed, "storage_capacity_packets": count + 1,
             "storage_capacity_bytes": (count + 1) * 80}
    expected = oracle(model, None, packets)
    actual = run_engine(model, None, packets, runner)
    check_events(expected, actual)
    original = np.array([model["t_ns"] + (delta_ns if selected(seed, i, probability) else 0) for i in range(count)])
    return {"schema": "hng-replay-check-v1", "mode": "fixed-two-point", "p": probability,
            "delta_ns": delta_ns, "packets": count, "deadline_mismatches": 0,
            "permitted_reordering": int(np.count_nonzero(np.diff([event["ingress_seq"] for event in actual]) < 0)),
            "realized_lateness": measure_lateness(np.arange(count), arrivals, original, model["t_ns"],
                                                   actual, 0, 20_000_000, 100_000)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("normalized", nargs="?", type=Path)
    parser.add_argument("--source", type=Path)
    parser.add_argument("--fixed-p")
    parser.add_argument("--count", type=int, default=10_000)
    parser.add_argument("--delta-ns", type=int, default=30_000_000)
    parser.add_argument("--seed", type=int, default=1)
    parser.add_argument("--runner", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    if args.fixed_p is not None:
        if args.source is not None or args.normalized is not None:
            parser.error("fixed mode does not take source/trace inputs")
        result = verify_fixed(args.runner, args.fixed_p, args.count, args.delta_ns, args.seed)
    else:
        if args.source is None or args.normalized is None:
            parser.error("trace mode requires normalized input and --source")
        result = verify_trace(read_source(args.source), strict_json(args.normalized.read_bytes()), args.runner)
    write_json(args.output, result)
    print(json.dumps(result))


if __name__ == "__main__":
    main()
