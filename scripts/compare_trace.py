#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Compare configured HNG intervals with delay-only TheaterQ extended rows."""

import argparse
import copy
import csv
import hashlib
import json
from pathlib import Path

from garcia_trace import U64_MAX, VERSION, lateness, read_source, write_json


def strict_json(text):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError(f"duplicate JSON key: {key}")
            result[key] = value
        return result

    def reject(value):
        raise ValueError(f"nonfinite JSON value: {value}")

    return json.loads(text, object_pairs_hook=unique, parse_constant=reject)


def read_theaterq(path):
    if Path(path).stat().st_size > 128 * 1024**2:
        raise ValueError("reference CSV exceeds 128 MiB")
    with Path(path).open(encoding="utf-8") as source:
        return [[int(value) for value in row] for row in csv.reader(
            line for line in source if line.strip() and not line.lstrip().startswith("#"))]


def validate_document(document, source):
    required = {"schema", "source_id", "direction", "units", "conversion", "source_sha256",
                "splice_policy", "trace_epoch_ns", "time_scale", "profile"}
    if not isinstance(document, dict) or set(document) != required:
        raise ValueError("unsupported normalized document fields")
    if (document["schema"] != "hng-trace-v1" or document["units"] != "ns_owd" or
            type(document["trace_epoch_ns"]) is not int or document["trace_epoch_ns"] != 0 or
            type(document["time_scale"]) is not int or document["time_scale"] != 1 or
            document["splice_policy"] != "natural_hold" or
            document["source_sha256"] != source.sha256 or document["direction"] != source.direction or
            document["source_id"] != source.filename):
        raise ValueError("source hash, direction, identity or normalized schema mismatch")
    recipe = strict_json(document["conversion"])
    if not isinstance(recipe, dict) or recipe.get("version") != VERSION:
        raise ValueError("unsupported conversion recipe")
    for key in ("effective_start_ns", "effective_end_ns", "offset_ns", "late_threshold_ns"):
        if type(recipe.get(key)) is not int:
            raise ValueError(f"invalid recipe {key}")
    profile = document["profile"]
    if not isinstance(profile, dict) or set(profile) != {
            "end_ns", "terminal", "terminal_horizon_ns", "loss_seed", "outage_action", "rows"}:
        raise ValueError("unsupported profile fields")
    terminal_ok = ((profile["terminal"] == "error" and profile["terminal_horizon_ns"] is None) or
                   (profile["terminal"] == "hold_last_sample" and type(profile["terminal_horizon_ns"]) is int and
                    type(profile["end_ns"]) is int and
                    profile["end_ns"] < profile["terminal_horizon_ns"] <= U64_MAX))
    if (not terminal_ok or profile["outage_action"] != "drop" or type(profile["loss_seed"]) is not int or
            not 0 <= profile["loss_seed"] <= U64_MAX or type(profile["end_ns"]) is not int or
            not 0 < profile["end_ns"] <= U64_MAX or recipe["late_threshold_ns"] < 0 or
            recipe["effective_end_ns"] - recipe["effective_start_ns"] != profile["end_ns"]):
        raise ValueError("invalid finite delay-only profile")
    if recipe["effective_start_ns"] < 0:
        raise ValueError("negative source origin")
    rows = profile["rows"]
    if not isinstance(rows, list) or not rows:
        raise ValueError("profile needs intervals")
    previous = -1
    for row in rows:
        if not isinstance(row, dict) or set(row) != {"at_ns", "t_ns", "loss", "outage"}:
            raise ValueError("invalid interval fields")
        if (type(row["at_ns"]) is not int or type(row["t_ns"]) is not int or
                not previous < row["at_ns"] < profile["end_ns"] or
                not 0 <= row["t_ns"] <= U64_MAX - profile["end_ns"] or
                row["loss"] != "0" or row["outage"] is not False):
            raise ValueError("invalid or unsupported delay-only interval")
        previous = row["at_ns"]
    if rows[0]["at_ns"] != 0:
        raise ValueError("intervals must start at zero")
    if profile["terminal"] == "hold_last_sample":
        budget = recipe.get("drain_budget")
        fields = {"t_max_ns", "delta_ns", "queue_drain_ns", "extension_ns"}
        if (not isinstance(budget, dict) or set(budget) != fields or
                any(type(value) is not int or value < 0 for value in budget.values())):
            raise ValueError("invalid terminal drain registration")
        extension = max(1, max(row["t_ns"] for row in rows) + budget["delta_ns"] + budget["queue_drain_ns"])
        if (budget["t_max_ns"] != max(row["t_ns"] for row in rows) or budget["extension_ns"] != extension or
                profile["terminal_horizon_ns"] != profile["end_ns"] + extension):
            raise ValueError("terminal HOLD does not match the registered t+delta+Q budget")
    return recipe


def compare(document, theaterq_rows, source):
    recipe = validate_document(document, source)
    expected, end = [], 0
    if not theaterq_rows:
        raise ValueError("reference has no rows")
    for row in theaterq_rows:
        if len(row) != 10 or any(type(value) is not int for value in row):
            raise ValueError("expected ten integer TheaterQ columns")
        if row[0] <= 0 or any(row[column] != 0 for column in (2, 4, 6, 7)):
            raise ValueError("reference must be delay-only, without jitter, loss or duplication")
        delay = row[1] + recipe["offset_ns"]
        if not 0 <= delay <= U64_MAX or row[1] < 0 or end + row[0] * 1000 > U64_MAX:
            raise ValueError("reference latency or duration overflow")
        expected.append({"at_ns": end, "t_ns": delay, "loss": "0", "outage": False})
        end += row[0] * 1000
    actual = document["profile"]["rows"]
    first = None
    for index in range(max(len(actual), len(expected))):
        a = actual[index] if index < len(actual) else None
        b = expected[index] if index < len(expected) else None
        if a != b:
            first = {"index": index, "hng": a, "theaterq": b}
            break
    coverage_equal = document["profile"]["end_ns"] == end
    if first is None and not coverage_equal:
        first = {"index": len(actual), "hng_end_ns": document["profile"]["end_ns"], "theaterq_end_ns": end}
    reference_document = copy.deepcopy(document)
    reference_document["profile"]["rows"] = expected
    reference_document["profile"]["end_ns"] = end
    reference_recipe = dict(recipe, effective_end_ns=recipe["effective_start_ns"] + end)
    reference_document["conversion"] = json.dumps(reference_recipe)
    return {"schema": "hng-trace-comparison-v1", "scope": "configured intervals only",
            "intervals_equal": first is None, "coverage_equal": coverage_equal,
            "hng_rows": len(actual), "theaterq_rows": len(expected), "first_mismatch": first,
            "offset_ns": recipe["offset_ns"], "source_sha256": source.sha256,
            "hng_lateness": lateness(source, document, recipe["late_threshold_ns"]),
            "theaterq_lateness": lateness(source, reference_document, recipe["late_threshold_ns"]),
            "run_length_acceptance": "descriptive comparison; no tolerance threshold registered"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("normalized", type=Path)
    parser.add_argument("theaterq", type=Path)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.output.exists():
            raise ValueError("report already exists")
        if args.normalized.stat().st_size > 128 * 1024**2:
            raise ValueError("normalized document exceeds 128 MiB")
        normalized_bytes = args.normalized.read_bytes()
        document = strict_json(normalized_bytes)
        result = compare(document, read_theaterq(args.theaterq), read_source(args.source))
        result["normalized_sha256"] = hashlib.sha256(normalized_bytes).hexdigest()
        result["theaterq_sha256"] = hashlib.sha256(args.theaterq.read_bytes()).hexdigest()
        write_json(args.output, result)
        print(json.dumps({"intervals_equal": result["intervals_equal"],
                          "first_mismatch": result["first_mismatch"],
                          "hng_lateness": result["hng_lateness"]}))
        return 0 if result["intervals_equal"] else 1
    except (ValueError, TypeError, KeyError, OSError, UnicodeError) as error:
        parser.exit(2, f"comparison failed: {error}\n")


if __name__ == "__main__":
    raise SystemExit(main())
