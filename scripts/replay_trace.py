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
from garcia_trace import read_source, write_json


def oracle(model, trace, packets):
    """Independent no-drop reference; reject offered traffic that would queue/drop."""
    finish, floor = Fraction(0), 0
    result = []
    if model["p"] != "0":
        raise ValueError("this oracle currently requires p=0")
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
        raw = service + delay
        deadline = max(raw, floor) if trace else raw
        if trace:
            if deadline >= horizon:
                raise ValueError("deadline outside registered trace coverage")
            floor = deadline
        result.append({"id": packet["id"], "ingress_seq": ordinal, "service_ns": service,
                       "deadline_ns": deadline, "release_ns": deadline,
                       "clamp_extra_ns": deadline - raw, "late": False})
    return sorted(result, key=lambda event: (event["release_ns"], event["ingress_seq"]))


def run_engine(model, trace, packets, runner):
    request = {"model": model, "trace": trace, "packets": packets}
    run = subprocess.run([str(runner)], input=json.dumps(request), text=True, capture_output=True, check=True)
    return strict_json(run.stdout)


def check_events(expected, actual):
    if expected != actual:
        raise ValueError("engine records differ from independent service/deadline/order oracle")


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
    return {"schema": "hng-replay-check-v1", "scope": "nonqueueing virtual scheduler, not wire/G2",
            "source_sha256": source.sha256, "packets": len(actual), "deadline_mismatches": 0,
            "unexpected_reordering": int(np.count_nonzero(np.diff([event["ingress_seq"] for event in actual]) < 0)),
            "clamp_packets": sum(event["clamp_extra_ns"] > 0 for event in actual),
            "clamp_max_ns": max((event["clamp_extra_ns"] for event in actual), default=0)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("normalized", type=Path)
    parser.add_argument("--source", required=True, type=Path)
    parser.add_argument("--runner", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    result = verify_trace(read_source(args.source), strict_json(args.normalized.read_bytes()), args.runner)
    write_json(args.output, result)
    print(json.dumps(result))


if __name__ == "__main__":
    main()
