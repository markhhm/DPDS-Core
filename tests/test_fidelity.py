# SPDX-License-Identifier: Apache-2.0
"""Adopted fidelity budgets and packet-identity run matching."""
import sys
from pathlib import Path
import unittest
import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'scripts'))
import replay_trace as replay


def stats(late=100, runs=20, pairs=100, joint=80, samples=1000):
    return dict(n_samples=samples, n_late=late, n_runs=runs,
                late_pair_denominator=pairs, late_pair_numerator=joint)


class FidelityTests(unittest.TestCase):
    def test_count_and_run_ceiling_boundaries(self):
        self.assertTrue(hasattr(replay, 'evaluate_fidelity'), 'adopted fidelity gate is absent')
        for source_l, output_l, changed, want_l, want_r in [
            (100, 105, 1, True, True), (100, 106, 1, False, True),
            (101, 107, 2, True, False), (101, 108, 0, False, True),
            (20, 22, 0, True, True), (20, 23, 0, False, True)]:
            with self.subTest(source_l=source_l, output_l=output_l, changed=changed):
                result = replay.evaluate_fidelity(stats(late=source_l), stats(late=output_l), changed)
                self.assertEqual(result['checks']['late_count'], want_l)
                self.assertEqual(result['checks']['changed_runs'], want_r)
        result = replay.evaluate_fidelity(stats(runs=21), stats(runs=21), 2)
        self.assertTrue(result['checks']['changed_runs'])
        self.assertFalse(replay.evaluate_fidelity(stats(runs=21), stats(runs=21), 3)['checks']['changed_runs'])
        self.assertTrue(replay.evaluate_fidelity(stats(runs=0), stats(runs=0), 1)['checks']['changed_runs'])
        self.assertFalse(replay.evaluate_fidelity(stats(runs=0), stats(runs=0), 2)['checks']['changed_runs'])

    def test_probability_boundary_is_exact(self):
        self.assertTrue(hasattr(replay, 'evaluate_fidelity'), 'adopted fidelity gate is absent')
        for joint, want in [(81, 'PASS'), (82, 'FAIL')]:
            self.assertEqual(replay.evaluate_fidelity(stats(), stats(joint=joint), 0)['status'], want)

    def test_minimum_on_either_side_precedes_fidelity(self):
        self.assertTrue(hasattr(replay, 'evaluate_fidelity'), 'adopted fidelity gate is absent')
        self.assertEqual(replay.evaluate_fidelity(stats(), stats(), 0)['status'], 'PASS')
        for side in ('source', 'engine'):
            for key, value in [('n_late', 99), ('n_runs', 19), ('late_pair_denominator', 99),
                               ('late_pair_denominator', 0)]:
                source, engine = stats(), stats()
                (source if side == 'source' else engine)[key] = value
                if value == 0:
                    (source if side == 'source' else engine)['late_pair_numerator'] = 0
                result = replay.evaluate_fidelity(source, engine, 100)
                self.assertEqual(result['status'], 'INCONCLUSIVE')
                self.assertIn(f'{side}.{key}', result['insufficient_evidence'])
        self.assertIsNone(replay.evaluate_fidelity(stats(pairs=0, joint=0), stats(), 0)['checks']['p_next'])

    def test_count_mismatch_is_correctness_failure_even_with_low_evidence(self):
        self.assertTrue(hasattr(replay, 'evaluate_fidelity'), 'adopted fidelity gate is absent')
        self.assertEqual(replay.evaluate_fidelity(stats(late=1), stats(samples=999), 0)['status'], 'FAIL')

    def test_identity_runs_splits_merges_and_gaps(self):
        self.assertTrue(hasattr(replay, 'changed_run_count'), 'identity-based run comparison is absent')
        for before, after, want in [
            ([0,1,1,0], [0,1,1,0], 0),
            ([0,1,1,0], [0,0,1,1], 1),
            ([0,1,1,0], [0,0,0,0], 1),
            ([0,0,0,0], [0,1,1,0], 1),
            ([1,1,1,1,1], [1,1,0,1,1], 2),
            ([1,1,0,1,1], [1,1,1,1,1], 2),
            ([1,0,1,1,0], [0,1,1,0,1], 3),
            ([1,0,0], [0,0,1], 2)]:
            n = len(before)
            self.assertEqual(replay.changed_run_count(np.arange(n), np.arange(n),
                np.array(before, bool), np.array(after, bool), 1), want)
        # Identical late flags across a missing sequence still form two runs.
        self.assertEqual(replay.changed_run_count(np.array([1,2,4,5]), np.arange(4),
            np.ones(4, bool), np.array([1,0,0,1], bool), 1), 2)
        for arrivals, want in [([0,1,4,5],2), ([0,1,5,6],1)]:
            self.assertEqual(replay.changed_run_count(np.arange(4), np.array(arrivals),
                np.ones(4, bool), np.array([1,1,0,1], bool), 1), want)

    def test_measurement_rejects_wrong_packet_identity_or_coverage(self):
        for events in [[], [{'id':2,'ingress_seq':0,'release_ns':1}],
                       [{'id':1,'ingress_seq':1,'release_ns':1}]]:
            with self.subTest(events=events):
                with self.assertRaises(ValueError):
                    replay.measure_lateness(np.array([10]), np.array([0]), np.array([0]),
                                            0, events, 0, 20, 1)


if __name__ == '__main__':
    unittest.main()
