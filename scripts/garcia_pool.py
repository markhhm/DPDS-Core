#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""List paired natural Garcia windows, their source hashes and exclusion reasons."""

import argparse
import hashlib
import json
from pathlib import Path


from garcia_trace import detect_phase, read_source, window_has_gap, write_json


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



def validate_selection(source, source_path, start_ns, duration_ns, pool_path, index):
    """Verify a registered candidate against both unchanged, colocated sources."""
    pool_path = Path(pool_path)
    if type(index) is not int or index < 0 or pool_path.stat().st_size > 128 * 1024**2:
        raise ValueError("invalid canonical pool index or manifest size")
    payload = pool_path.read_bytes()
    pool = json.loads(payload)
    if (not isinstance(pool, dict) or pool.get("schema") != "garcia-canonical-pool-v1" or
            not isinstance(pool.get("candidates"), list) or index >= len(pool["candidates"])):
        raise ValueError("invalid canonical pool or candidate index")
    item = pool["candidates"][index]
    if (not isinstance(item, dict) or set(item) != {"stamp", "phase_ns", "start_ns", "duration_ns", "sources"} or
            any(type(item.get(key)) is not int for key in ("phase_ns", "start_ns", "duration_ns")) or
            item["start_ns"] != start_ns or item["duration_ns"] != duration_ns or
            not isinstance(item["stamp"], str) or not item["stamp"] or
            Path(item["stamp"]).name != item["stamp"] or "\\" in item["stamp"] or
            not isinstance(item["sources"], dict) or set(item["sources"]) != {"ul", "dl"}):
        raise ValueError("canonical candidate identity or window mismatch")
    sources = {}
    for direction in ("ul", "dl"):
        filename = f"{item['stamp']}__{direction}.csv.zip"
        current = source if direction == source.direction else read_source(Path(source_path).parent / filename)
        metadata = {"filename": current.filename, "sha256": current.sha256, "member_sha256": current.member_sha256}
        if current.direction != direction or current.filename != filename or metadata != item["sources"][direction]:
            raise ValueError("canonical paired source identity/hash mismatch")
        sources[direction] = current
    phase = detect_phase(sources["dl"])
    earliest = max(500_000_000, *(int(s.send_ns[0]) for s in sources.values()))
    latest = min(int(s.send_ns[-1]) for s in sources.values()) - 500_000_000
    if (phase is None or item["phase_ns"] != phase or start_ns < earliest or
            start_ns + duration_ns > latest or start_ns < phase or
            (start_ns - phase) % 15_000_000_000 != 0 or
            any(window_has_gap(s, start_ns, duration_ns) for s in sources.values())):
        raise ValueError("canonical phase, source margins or measurement gaps invalid")
    return phase, {"manifest_sha256": hashlib.sha256(payload).hexdigest(), "index": index, "candidate": item}


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
