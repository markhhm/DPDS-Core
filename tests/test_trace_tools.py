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

    def test_cli_writes_manifest_and_refuses_overwrite(self):
        path = self.source()
        output = self.directory / "normalized.json"
        command = [sys.executable, str(ROOT / "scripts/garcia_trace.py"), str(path), str(output),
                   "--start-ns", "0", "--duration-ns", "15000000", "--offset-ns", "0"]
        first = subprocess.run(command, capture_output=True, text=True)
        self.assertEqual(first.returncode, 0, first.stderr)
        manifest = json.loads(output.with_suffix(".manifest.json").read_text())
        self.assertEqual(manifest["normalized_sha256"], hashlib.sha256(output.read_bytes()).hexdigest())
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
