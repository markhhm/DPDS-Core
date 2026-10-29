# SPDX-License-Identifier: Apache-2.0
"""The shared entry point must fail clearly when offline dependencies are absent."""

import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class CheckDependencyTests(unittest.TestCase):
    def test_missing_numpy_fails_with_installation_help_before_rust(self):
        with tempfile.TemporaryDirectory() as directory:
            python = Path(directory) / "python-without-numpy"
            python.write_text("#!/bin/sh\nexit 1\n")
            python.chmod(0o755)
            env = dict(os.environ, PYTHON=str(python), CARGO="/bin/false")
            result = subprocess.run([str(ROOT / "scripts/check.sh")], cwd=ROOT,
                                    env=env, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("NumPy", result.stderr)
            self.assertIn("pip install -r scripts/trace-requirements.txt", result.stderr)
            self.assertNotIn("all checks passed", result.stdout)
            self.assertNotIn("CARGO=", result.stderr)
