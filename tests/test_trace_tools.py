# SPDX-License-Identifier: Apache-2.0
"""Synthetic-only tests; measured ZIPs and deployment values are never fixtures."""

import hashlib
import importlib
import json
import copy
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import zipfile

import numpy as np

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))


class ConverterTests(unittest.TestCase):
    def setUp(self):
        self.assertTrue((ROOT / "scripts/garcia_trace.py").exists(), "converter is not implemented")
        self.tool = importlib.import_module("garcia_trace")
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name)

    def source(self, text=None, name="synthetic.zip"):
        path = self.directory / name
        text = text or (ROOT / "tests/fixtures/garcia-synthetic.csv").read_text()
        with zipfile.ZipFile(path, "w") as archive:
            archive.writestr("synthetic.csv", text)
        return path

    def test_source_hash_direction_and_missing_are_preserved(self):
        path = self.source()
        source = self.tool.read_source(path)
        self.assertEqual(source.sha256, hashlib.sha256(path.read_bytes()).hexdigest())
        self.assertEqual(source.direction, "ul")
        self.assertEqual(source.missing, 1)
        document, manifest = self.tool.convert(path, 0, 15_000_000, 7, 1, 20_000_000)
        self.assertEqual(document["source_sha256"], source.sha256)
        self.assertEqual(document["schema"], "hng-trace-v1")
        self.assertEqual(document["profile"]["end_ns"], 15_000_000)
        self.assertEqual(document["profile"]["terminal"], "error")
        self.assertTrue(all(row["loss"] == "0" and not row["outage"] for row in document["profile"]["rows"]))
        recipe = json.loads(document["conversion"])
        self.assertEqual(recipe["offset_ns"], 7)
        self.assertEqual(recipe["seed"], 1)
        self.assertEqual(recipe["version"], "garcia-interval-v1")
        self.assertEqual(manifest["source"]["filename"], "synthetic.zip")
        self.assertNotIn(str(self.directory), json.dumps(document) + json.dumps(manifest))
        self.assertEqual(manifest["lateness"]["source"]["n_late"], 5)
        self.assertEqual(manifest["lateness"]["source"]["histogram"], {"1": 1, "4": 1})
        self.assertEqual(manifest["lateness"]["configured"]["histogram"], {"1": 1, "4": 1})

    def test_unregistered_window_is_explicitly_diagnostic(self):
        document, manifest = self.tool.convert(self.source(), 0, 15_000_000)
        recipe = json.loads(document["conversion"])
        self.assertEqual(recipe["eligibility"], "diagnostic")
        self.assertIsNone(recipe["phase_ns"])
        self.assertIsNone(recipe["pool_item"])
        self.assertEqual(recipe, manifest["recipe"])

    def test_linear_midpoints_adaptive_rows_and_latency_rounding(self):
        path = self.source("seq_nr,dl_t0,owd_t0_t1\n1,0,20000000\n2,1000000,20200000\n3,2000000,20400000\n")
        document, _ = self.tool.convert(path, 0, 1_000_000, 0, 1, 20_000_000)
        self.assertEqual(document["profile"]["rows"], [
            {"at_ns": 0, "t_ns": 20_025_000, "loss": "0", "outage": False},
            {"at_ns": 250_000, "t_ns": 20_075_000, "loss": "0", "outage": False},
            {"at_ns": 500_000, "t_ns": 20_125_000, "loss": "0", "outage": False},
            {"at_ns": 750_000, "t_ns": 20_175_000, "loss": "0", "outage": False},
        ])

    def test_registered_corridors_apply_exact_formula_and_record_components(self):
        self.assertTrue(hasattr(self.tool, "corridor_offset"), "registered corridor conversion is absent")
        path = self.source()
        native, _ = self.tool.convert(path, 0, 15_000_000)
        for corridor, expected, pop in (("A", 45_600_000, 52_600_000), ("B", 106_800_000, 113_800_000)):
            with self.subTest(corridor=corridor):
                document, manifest = self.tool.convert(path, 0, 15_000_000, corridor=corridor)
                self.assertEqual(self.tool.corridor_offset(corridor), expected)
                self.assertEqual([r["t_ns"] for r in document["profile"]["rows"]],
                                 [r["t_ns"] + expected for r in native["profile"]["rows"]])
                recipe = manifest["recipe"]
                self.assertEqual(recipe["offset_ns"], expected)
                self.assertEqual(recipe["propagation"], {"corridor": corridor, "baseline_ns": 11_000_000,
                                                        "geometry_ns": 4_000_000, "pop_delay_ns": pop})
        with self.assertRaises(ValueError):
            self.tool.convert(path, 0, 15_000_000, 123, corridor="A")

    def test_terminal_hold_records_actual_t_delta_queue_budget(self):
        self.assertIn("queue_drain_ns", self.tool.convert.__code__.co_varnames, "finite drain registration is absent")
        path = self.source()
        document, manifest = self.tool.convert(path, 0, 15_000_000, corridor="A",
                                               queue_drain_ns=50_000_000, delta_ns=5_000_000)
        self.assertEqual(document["profile"]["terminal"], "hold_last_sample")
        self.assertEqual(document["profile"]["terminal_horizon_ns"], 165_600_000)
        self.assertEqual(manifest["recipe"]["drain_budget"], {
            "t_max_ns": 95_600_000, "delta_ns": 5_000_000,
            "queue_drain_ns": 50_000_000, "extension_ns": 150_600_000})
        for budget in (-1, True, 2**64):
            with self.subTest(budget=budget), self.assertRaises(ValueError):
                self.tool.convert(path, 0, 15_000_000, queue_drain_ns=budget)

    def test_cli_accepts_registered_corridor_instead_of_explicit_offset(self):
        path = self.source()
        output = self.directory / "corridor.json"
        command = [sys.executable, str(ROOT / "scripts/garcia_trace.py"), str(path), str(output),
                   "--start-ns", "0", "--duration-ns", "15000000", "--corridor", "B"]
        result = subprocess.run(command, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(json.loads(output.read_text())["conversion"])["offset_ns"], 106_800_000)

    def test_seed_uses_full_source_state_when_cropping(self):
        path = self.source("seq_nr,ul_t0,owd_t0_t1\n1,0,20000000\n2,1000000,20000000\n3,2000000,25000000\n4,3000000,25000000\n5,4000000,20000000\n")
        full, _ = self.tool.convert(path, 0, 4_000_000, 0, 1, 20_000_000)
        cropped, _ = self.tool.convert(path, 2_000_000, 2_000_000, 0, 1, 20_000_000)
        rows = [{**row, "at_ns": row["at_ns"] - 2_000_000}
                for row in full["profile"]["rows"] if row["at_ns"] >= 2_000_000]
        # Seed 1 draws once per original sample pair, including pairs before this crop.
        self.assertEqual(cropped["profile"]["rows"][-1]["at_ns"], 1_949_000)
        self.assertEqual(cropped["profile"]["rows"][-1]["t_ns"], 20_000_000)
        self.assertEqual(rows[-1], cropped["profile"]["rows"][-1])

    def test_trend_drain_across_missing_samples_does_not_create_a_step(self):
        path = self.source("seq_nr,dl_t0,owd_t0_t1\n1,0,30000000\n2,1000000,29500000\n3,2000000,29000000\n6,5000000,27500000\n7,6000000,27000000\n")
        source = self.tool.read_source(path)
        curve = self.tool.DelayCurve(source, 1)
        self.assertEqual(len(curve.switches), 0)
        self.assertAlmostEqual(curve.evaluate(np.array([0.003]))[0][0], 28.5)

    def test_round_to_even_boundaries_and_latencies(self):
        path = self.source("seq_nr,dl_t0,owd_t0_t1\n1,0,20000500\n2,2000,20000500\n3,4000,20000500\n")
        document, manifest = self.tool.convert(path, 500, 2000, 1, 1, 20_000_000)
        self.assertEqual(document["profile"]["end_ns"], 2000)
        self.assertEqual(document["profile"]["rows"][0]["t_ns"], 20_000_001)
        self.assertEqual(manifest["recipe"]["effective_start_ns"], 0)

    def test_fractional_microsecond_end_matches_reference_float_addition(self):
        path = self.source("seq_nr,dl_t0,owd_t0_t1\n1,0,20000000\n2,1000000,20000000\n3,2000000,20000000\n")
        document, _ = self.tool.convert(path, 500, 6000, 0, 1, 20_000_000)
        # Reference conversion adds start_s and duration_s before rounding the end.
        self.assertEqual(document["profile"]["end_ns"], 7000)

    def test_large_offset_cancels_exactly_before_late_classification(self):
        path = self.source("seq_nr,ul_t0,owd_t0_t1\n1,0,20000000\n2,1000000,20000000\n3,2000000,40001000\n4,3000000,20000000\n5,4000000,20000000\n")
        _, native = self.tool.convert(path, 0, 4_000_000, 0, 1, 20_001_000)
        _, shifted = self.tool.convert(path, 0, 4_000_000, 2**60, 1, 20_001_000)
        self.assertEqual(native["lateness"], shifted["lateness"])

    def test_invalid_zip_headers_duplicates_negative_and_coverage_are_rejected(self):
        for text in (
            "a,b,c\n1,0,2\n2,1,2\n",
            "seq_nr,ul_t0,owd_t0_t1\n1,0,2\n1,1,2\n",
            "seq_nr,ul_t0,owd_t0_t1\n1,0,-1\n2,1,2\n",
            "seq_nr,ul_t0,owd_t0_t1\n1,2,2\n2,1,2\n",
        ):
            with self.subTest(text=text), self.assertRaises(ValueError):
                self.tool.read_source(self.source(text))
        path = self.source()
        for start, duration in ((-1, 1), (0, 0), (0, 46_000_000_000), (0, 16_000_000)):
            with self.subTest(window=(start, duration)), self.assertRaises(ValueError):
                self.tool.convert(path, start, duration, 0, 1, 20_000_000)
        with zipfile.ZipFile(path, "a") as archive:
            archive.writestr("extra.csv", "x")
        with self.assertRaises(ValueError):
            self.tool.read_source(path)

    def test_measurement_gap_and_negative_shift_do_not_become_outages(self):
        path = self.source("seq_nr,dl_t0,owd_t0_t1\n1,0,20000000\n2,1000000000,20000000\n")
        with self.assertRaisesRegex(ValueError, "measurement gap"):
            self.tool.convert(path, 0, 1_000_000_000, 0, 1, 20_000_000)
        path = self.source()
        with self.assertRaises(ValueError):
            self.tool.convert(path, 0, 15_000_000, -100_000_000, 1, 20_000_000)

    def test_gap_cutoff_is_strictly_greater_than_half_a_second(self):
        self.assertTrue(hasattr(self.tool, "window_has_gap"), "shared gap detector is absent")
        path = self.source("seq_nr,dl_t0,owd_t0_t1\n1,0,20000000\n2,500000000,20000000\n3,1000000000,20000000\n")
        self.assertFalse(self.tool.window_has_gap(self.tool.read_source(path), 0, 1_000_000_000))
        path = self.source("seq_nr,dl_t0,owd_t0_t1\n1,0,20000000\n2,500000001,20000000\n3,1000000000,20000000\n")
        self.assertTrue(self.tool.window_has_gap(self.tool.read_source(path), 0, 1_000_000_000))

    def test_cli_writes_manifest_and_refuses_overwrite(self):
        path = self.source()
        output = self.directory / "normalized.json"
        command = [sys.executable, str(ROOT / "scripts/garcia_trace.py"), str(path), str(output),
                   "--start-ns", "0", "--duration-ns", "15000000", "--offset-ns", "0", "--queue-drain-ns", "0"]
        first = subprocess.run(command, capture_output=True, text=True)
        self.assertEqual(first.returncode, 0, first.stderr)
        manifest = json.loads(output.with_suffix(".manifest.json").read_text())
        self.assertEqual(manifest["normalized_sha256"], hashlib.sha256(output.read_bytes()).hexdigest())
        self.assertEqual(json.loads(output.read_text())["profile"]["terminal_horizon_ns"], 65_000_000)
        before = output.read_bytes()
        second = subprocess.run(command, capture_output=True, text=True)
        self.assertNotEqual(second.returncode, 0)
        self.assertEqual(output.read_bytes(), before)

    def test_late_runs_cut_sequence_gaps_and_large_send_gaps(self):
        result = self.tool.late_runs(
            np.array([1, 2, 3, 5, 6, 7]), np.array([0, 1, 2, 3, 20, 21]),
            np.array([False, True, True, True, True, False]), 1.0)
        self.assertEqual(result["histogram"], {"1": 2, "2": 1})
        self.assertEqual(result["late_pair_numerator"], 1)
        self.assertEqual(result["late_pair_denominator"], 2)
        self.assertEqual(result["n_late"], 4)

    def test_empty_diagnostic_window_is_not_reported_as_observed_zero(self):
        path = self.source("seq_nr,ul_t0,owd_t0_t1\n1,0,20000000\n2,1000000,20000000\n")
        _, manifest = self.tool.convert(path, 100_000, 100_000, 0, 1, 20_000_000)
        self.assertEqual(manifest["lateness"]["source"]["n_samples"], 0)
        self.assertIsNone(manifest["lateness"]["source"]["p_next_late_given_late"])

    def test_threshold_is_strict_and_baseline_uses_full_source(self):
        path = self.source("seq_nr,ul_t0,owd_t0_t1\n1,0,20000000\n2,1000000,20000000\n3,2000000,40000000\n4,3000000,20000000\n5,4000000,20000000\n")
        _, manifest = self.tool.convert(path, 1_000_000, 2_000_000, 5_000_000, 1, 20_000_000)
        self.assertEqual(manifest["lateness"]["source"]["n_late"], 0)
        self.assertEqual(manifest["lateness"]["source"]["n_samples"], 2)


class PoolTests(unittest.TestCase):
    def setUp(self):
        self.assertTrue((ROOT / "scripts/garcia_pool.py").exists(), "canonical pool selector is absent")
        self.pool = importlib.import_module("garcia_pool")
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name)

    def pair(self, stamp, phase=True, gap=False):
        times = set(range(0, 20_000_000_001, 100_000_000))
        if phase:
            times.update(range(1_000_000_000, 1_025_000_000, 1_000_000))
        if gap:
            times = {t for t in times if not 1_100_000_000 <= t <= 1_800_000_000}
        for direction in ("ul", "dl"):
            text = f"seq_nr,{direction}_t0,owd_t0_t1\n"
            for index, sent in enumerate(sorted(times), 1):
                delay = 50_000_000 if phase and 1_000_000_000 <= sent < 1_025_000_000 else 20_000_000
                text += f"{index},{sent},{delay}\n"
            with zipfile.ZipFile(self.directory / f"{stamp}__{direction}.csv.zip", "w") as archive:
                archive.writestr("synthetic.csv", text)

    def test_no_phase_pair_is_excluded_and_valid_pair_is_aligned(self):
        self.pair("flat", phase=False)
        self.pair("valid")
        result = self.pool.build_pool(self.directory, 1_000_000_000)
        self.assertTrue(any(row["stamp"] == "flat" and row["reason"] == "no_phase" for row in result["excluded"]))
        self.assertTrue(result["candidates"])
        for row in result["candidates"]:
            self.assertEqual(row["stamp"], "valid")
            self.assertEqual((row["start_ns"] - row["phase_ns"]) % 15_000_000_000, 0)
            self.assertGreaterEqual(row["start_ns"], 500_000_000)
            self.assertEqual(set(row["sources"]), {"ul", "dl"})
            self.assertTrue(all(len(s["sha256"]) == 64 for s in row["sources"].values()))
        self.assertNotIn(str(self.directory), json.dumps(result))

    def test_conversion_checks_registered_pool_identity_phase_and_boundaries(self):
        self.pair("valid")
        pool = self.pool.build_pool(self.directory, 1_000_000_000)
        item = pool["candidates"][0]
        pool_path = self.directory / "pool.json"
        pool_path.write_text(json.dumps(pool))
        source = self.directory / item["sources"]["ul"]["filename"]
        converter = importlib.import_module("garcia_trace")
        document, manifest = converter.convert(source, item["start_ns"], item["duration_ns"],
                                                pool_path=pool_path, pool_index=0)
        recipe = json.loads(document["conversion"])
        self.assertEqual(recipe["eligibility"], "canonical")
        self.assertEqual(recipe["phase_ns"], item["phase_ns"])
        self.assertEqual(recipe["pool_item"], {"manifest_sha256": hashlib.sha256(pool_path.read_bytes()).hexdigest(),
                                               "index": 0, "candidate": item})
        self.assertEqual(recipe, manifest["recipe"])
        for mode in ("phase", "margin", "hash", "start"):
            altered = copy.deepcopy(pool)
            candidate = altered["candidates"][0]
            start = item["start_ns"]
            if mode == "phase":
                candidate["phase_ns"] += 1
            elif mode == "margin":
                candidate["start_ns"] = start = 0
            elif mode == "hash":
                candidate["sources"]["dl"]["sha256"] = "0" * 64
            else:
                start += 1000
            pool_path.write_text(json.dumps(altered))
            with self.subTest(mode=mode), self.assertRaises(ValueError):
                converter.convert(source, start, item["duration_ns"], pool_path=pool_path, pool_index=0)
        pool_path.write_text(json.dumps(pool))
        output = self.directory / "canonical.json"
        result = subprocess.run([sys.executable, str(ROOT / "scripts/garcia_trace.py"), str(source),
                                 str(output), "--start-ns", str(item["start_ns"]), "--duration-ns", str(item["duration_ns"]),
                                 "--offset-ns", "0", "--pool", str(pool_path), "--pool-index", "0"], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(json.loads(output.read_text())["conversion"])["eligibility"], "canonical")

    def test_canonical_rejects_missing_phase_gaps_and_rounded_window_changes(self):
        converter = importlib.import_module("garcia_trace")
        self.pair("flat", phase=False)
        self.pair("gapped", gap=True)
        pool = self.pool.build_pool(self.directory, 1_000_000_000)
        gap = next(row for row in pool["excluded"] if row["reason"] == "measurement_gap")
        flat = next(row for row in pool["excluded"] if row["reason"] == "no_phase")
        forged = [{key: value for key, value in gap.items() if key != "reason"},
                  {"stamp": "flat", "phase_ns": 0, "start_ns": 15_000_000_000,
                   "duration_ns": 1_000_000_000, "sources": flat["sources"]}]
        pool_path = self.directory / "pool.json"
        for item in forged:
            pool_path.write_text(json.dumps({"schema": pool["schema"], "candidates": [item]}))
            path = self.directory / item["sources"]["ul"]["filename"]
            with self.subTest(stamp=item["stamp"]), self.assertRaises(ValueError):
                converter.convert(path, item["start_ns"], item["duration_ns"], pool_path=pool_path, pool_index=0)
        self.pair("valid")
        pool = self.pool.build_pool(self.directory, 1_000_000_500)
        index = next(i for i, item in enumerate(pool["candidates"]) if item["stamp"] == "valid")
        item = pool["candidates"][index]
        pool_path.write_text(json.dumps(pool))
        with self.assertRaises(ValueError):
            converter.convert(self.directory / item["sources"]["ul"]["filename"], item["start_ns"],
                              item["duration_ns"], pool_path=pool_path, pool_index=index)

    def test_gap_window_is_excluded_and_later_natural_window_survives(self):
        self.pair("gapped", gap=True)
        result = self.pool.build_pool(self.directory, 1_000_000_000)
        self.assertTrue(any(row["reason"] == "measurement_gap" for row in result["excluded"]))
        self.assertTrue(result["candidates"])
        self.assertTrue(all(row["start_ns"] > 2_000_000_000 for row in result["candidates"]))


class ComparatorTests(unittest.TestCase):
    def setUp(self):
        self.assertTrue((ROOT / "scripts/compare_trace.py").exists(), "comparator is not implemented")
        self.compare = importlib.import_module("compare_trace")
        self.converter = importlib.import_module("garcia_trace")
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name)
        self.path = self.directory / "synthetic.zip"
        with zipfile.ZipFile(self.path, "w") as archive:
            archive.writestr("synthetic.csv", (ROOT / "tests/fixtures/garcia-synthetic.csv").read_text())
        self.source = self.converter.read_source(self.path)
        self.document, _ = self.converter.convert(self.path, 0, 15_000_000, 0, 1, 20_000_000)
        self.rows = self.compare.read_theaterq(ROOT / "tests/fixtures/theaterq-synthetic.csv")

    def test_independent_synthetic_rows_match_and_diagnostics_include_source(self):
        result = self.compare.compare(self.document, self.rows, self.source)
        self.assertTrue(result["intervals_equal"])
        self.assertIsNone(result["first_mismatch"])
        self.assertEqual(result["hng_lateness"]["source"]["histogram"], {"1": 1, "4": 1})
        self.assertEqual(result["theaterq_lateness"]["configured"]["histogram"], {"1": 1, "4": 1})
        self.assertEqual(result["scope"], "configured intervals only")

    def test_latency_boundary_and_tail_changes_are_reported(self):
        for change in ("latency", "boundary", "tail"):
            rows = copy.deepcopy(self.rows)
            if change == "latency":
                rows[1][1] += 1
            elif change == "boundary":
                rows[0][0] += 1
                rows[1][0] -= 1
            else:
                rows.pop()
            with self.subTest(change=change):
                result = self.compare.compare(self.document, rows, self.source)
                self.assertFalse(result["intervals_equal"])
                self.assertIsNotNone(result["first_mismatch"])

    def test_declared_offset_is_applied_to_reference_without_requantization(self):
        document, _ = self.converter.convert(self.path, 0, 15_000_000, 731, 1, 20_000_000)
        self.assertTrue(self.compare.compare(document, self.rows, self.source)["intervals_equal"])

    def test_registered_terminal_hold_preserves_row_equality_and_rejects_short_budget(self):
        self.assertIn("queue_drain_ns", self.converter.convert.__code__.co_varnames)
        document, _ = self.converter.convert(self.path, 0, 15_000_000, queue_drain_ns=50_000_000)
        self.assertTrue(self.compare.compare(document, self.rows, self.source)["intervals_equal"])
        document["profile"]["terminal_horizon_ns"] -= 1
        with self.assertRaises(ValueError):
            self.compare.compare(document, self.rows, self.source)

    def test_invalid_source_schema_rows_and_nonportable_metadata_are_rejected(self):
        for mutate in (
            lambda d: d.update(source_sha256="0" * 64),
            lambda d: d.update(direction="dl"),
            lambda d: d["profile"]["rows"][0].update(at_ns=1),
            lambda d: d["profile"]["rows"][0].update(t_ns=-1),
            lambda d: d["profile"]["rows"][0].update(outage=True),
            lambda d: d.update(time_scale=2),
        ):
            document = copy.deepcopy(self.document)
            mutate(document)
            with self.subTest(document=document), self.assertRaises(ValueError):
                self.compare.compare(document, self.rows, self.source)
        for column in (2, 4, 6, 7):
            rows = copy.deepcopy(self.rows)
            rows[0][column] = 1
            with self.subTest(column=column), self.assertRaises(ValueError):
                self.compare.compare(self.document, rows, self.source)

    def test_cli_reports_mismatch_and_rejects_duplicate_json_keys(self):
        normalized = self.directory / "normalized.json"
        report = self.directory / "comparison.json"
        document = copy.deepcopy(self.document)
        document["profile"]["rows"][0]["t_ns"] += 1
        normalized.write_text(json.dumps(document))
        command = [sys.executable, str(ROOT / "scripts/compare_trace.py"), str(normalized),
                   str(ROOT / "tests/fixtures/theaterq-synthetic.csv"), "--source", str(self.path),
                   "--output", str(report)]
        result = subprocess.run(command, capture_output=True, text=True)
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertFalse(json.loads(report.read_text())["intervals_equal"])
        normalized.write_text('{"schema":"hng-trace-v1","schema":"another"}')
        result = subprocess.run(command[:-2] + ["--output", str(self.directory / "bad.json")],
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 2)


if __name__ == "__main__":
    unittest.main()
