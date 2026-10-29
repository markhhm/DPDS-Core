#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""List paired natural Garcia windows, their source hashes and exclusion reasons."""

import argparse
import json
from pathlib import Path

import numpy as np

from garcia_trace import read_source, window_has_gap, write_json


def detect_phase(source):
    delays = source.owd_ns / 1e6
    spikes = delays > np.median(delays) + 15.0
    if int(np.count_nonzero(spikes)) < 21:
        return None
    histogram, edges = np.histogram((source.send_ns[spikes] / 1e9) % 15.0,
                                   bins=300, range=(0, 15.0))
    scores = histogram + np.roll(histogram, -1) + np.roll(histogram, 1)
    return round(float(edges[int(np.argmax(scores))]) * 1e9)


def build_pool(directory, duration_ns):
    directory = Path(directory)
    if not directory.is_dir() or type(duration_ns) is not int or not 0 < duration_ns <= 45_000_000_000:
        raise ValueError("source directory and duration in (0,45 s] are required")
    candidates, excluded = [], []
    paths = {p.name: p for p in directory.glob("*__*.csv.zip") if not p.name.startswith(".")}
    stamps = sorted({name.rsplit("__", 1)[0] for name in paths if name.endswith(("__ul.csv.zip", "__dl.csv.zip"))})
    period, margin = 15_000_000_000, 500_000_000
    for stamp in stamps:
        names = {direction: f"{stamp}__{direction}.csv.zip" for direction in ("ul", "dl")}
        if any(name not in paths for name in names.values()):
            excluded.append({"stamp": stamp, "reason": "unpaired"})
            continue
        try:
            sources = {direction: read_source(paths[name]) for direction, name in names.items()}
        except (ValueError, OSError):
            excluded.append({"stamp": stamp, "reason": "invalid_source"})
            continue
        metadata = {direction: {"filename": source.filename, "sha256": source.sha256,
                                "member_sha256": source.member_sha256}
                    for direction, source in sources.items()}
        if any(source.direction != direction for direction, source in sources.items()):
            excluded.append({"stamp": stamp, "reason": "direction_mismatch", "sources": metadata})
            continue
        phase = detect_phase(sources["dl"])
        if phase is None:
            excluded.append({"stamp": stamp, "reason": "no_phase", "sources": metadata})
            continue
        earliest = max(margin, *(int(source.send_ns[0]) for source in sources.values()))
        start = phase + max(0, (earliest - phase + period - 1) // period) * period
        end = min(int(source.send_ns[-1]) for source in sources.values()) - margin
        if start + duration_ns > end:
            excluded.append({"stamp": stamp, "reason": "insufficient_coverage", "sources": metadata})
        while start + duration_ns <= end:
            window = {"stamp": stamp, "phase_ns": phase, "start_ns": start,
                      "duration_ns": duration_ns, "sources": metadata}
            if any(window_has_gap(source, start, duration_ns) for source in sources.values()):
                excluded.append({**window, "reason": "measurement_gap"})
            else:
                candidates.append(window)
            start += period
    return {"schema": "garcia-canonical-pool-v1", "duration_ns": duration_ns,
            "phase_rule": "DL median+15ms; >=21 spikes; 300 bins/15s; first neighbor-sum maximum",
            "source_margin_ns": margin, "candidates": candidates, "excluded": excluded}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--duration-ns", type=int, required=True)
    args = parser.parse_args()
    try:
        result = build_pool(args.directory, args.duration_ns)
        write_json(args.output, result)
        print(json.dumps({"candidates": len(result["candidates"]), "excluded": len(result["excluded"])}))
    except (ValueError, OSError) as error:
        parser.exit(2, f"pool selection failed: {error}\n")


if __name__ == "__main__":
    main()
