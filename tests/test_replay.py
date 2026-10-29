# SPDX-License-Identifier: Apache-2.0
"""Converted traces must execute in the actual scheduler, not only a row model."""

import importlib
import os
from pathlib import Path
import sys
import tempfile
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))


class ConvertedReplayTests(unittest.TestCase):
    def setUp(self):
        self.assertTrue((ROOT / "scripts/replay_trace.py").exists(), "engine replay verifier is absent")
        self.verifier = importlib.import_module("replay_trace")
        self.converter = importlib.import_module("garcia_trace")
        self.runner = ROOT / "target/debug/trace_replay"
        self.assertTrue(self.runner.exists(), "build trace_replay before running integration tests")

    def run_source(self, path, start_ns, duration_ns):
        source = self.converter.read_source(path)
        document, _ = self.converter.convert(path, start_ns, duration_ns, queue_drain_ns=1_000_000)
        result = self.verifier.verify_trace(source, document, self.runner)
        self.assertGreater(result["packets"], 0)
        self.assertEqual(result["deadline_mismatches"], 0)
        self.assertEqual(result["unexpected_reordering"], 0)
        self.assertGreater(result["clamp_packets"], 0)
        return result

    def test_converted_synthetic_down_step_matches_independent_oracle(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "synthetic.zip"
            with zipfile.ZipFile(path, "w") as archive:
                archive.writestr("synthetic.csv", (ROOT / "tests/fixtures/garcia-synthetic.csv").read_bytes())
            self.run_source(path, 0, 15_000_000)

    def test_real_garcia_converted_segment_matches_independent_oracle(self):
        path = os.environ.get("REAL_GARCIA_ZIP")
        if path is None:
            self.skipTest("external raw Garcia source not bundled; supply REAL_GARCIA_ZIP")
        result = self.run_source(Path(path), 38_000_000_000, 11_000_000_000)
        self.assertGreater(result["packets"], 40_000)


if __name__ == "__main__":
    unittest.main()
