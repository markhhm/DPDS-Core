# SPDX-License-Identifier: Apache-2.0
"""Optional DPDK pcap I/O around the real engine, with source timestamps retained."""
import os
from pathlib import Path
import sys
import tempfile
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))


@unittest.skipUnless(os.environ.get('HNG_PCAP_BINARY'), 'supply compiled optional DPDK pcap adapter')
class PcapReplayTests(unittest.TestCase):
    def test_full_40s_sized_cohort_has_sufficient_pcap_heap(self):
        import replay_trace
        result = replay_trace.verify_fixed(ROOT / 'scripts/pcap_replay.py', '0', 150_000, delta_ns=0)
        self.assertEqual(result['packets'], 150_000)
        self.assertEqual(result['deadline_mismatches'], 0)

    def test_pcap_downstep_preserves_identities_and_exact_deadlines(self):
        runner = ROOT / 'scripts/pcap_replay.py'
        self.assertTrue(runner.exists(), 'pcap engine runner is absent')
        import garcia_trace
        import replay_trace
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'synthetic.zip'
            with zipfile.ZipFile(path, 'w') as archive:
                archive.writestr('synthetic.csv', (ROOT / 'tests/fixtures/garcia-synthetic.csv').read_bytes())
            document, _ = garcia_trace.convert(path, 0, 15_000_000, queue_drain_ns=1_000_000)
            result = replay_trace.verify_trace(garcia_trace.read_source(path), document, runner)
            self.assertEqual(result['deadline_mismatches'], 0)
            self.assertEqual(result['unexpected_reordering'], 0)
            self.assertGreater(result['clamp_packets'], 0)
            self.assertEqual(result['realized_lateness']['status'], 'INCONCLUSIVE')
