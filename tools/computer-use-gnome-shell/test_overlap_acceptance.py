import copy
import unittest

from overlap_acceptance import CASES, evaluate


def fixture():
    state = {'owner': ':1.32', 'pid': 5000,
             'policy': [1, '00000000-0000-0000-0000-000000000001', 0, False],
             'counts': {'preedit': 0, 'commit': 0, 'key_press': 0, 'key_release': 0},
             'nativeWayland': True, 'windowActive': True, 'entryFocused': True,
             'imeModule': 'wayland', 'engine': 'libpinyin', 'preeditActive': False,
             'expectedCommit': False}
    rows = []
    for name in CASES:
        before = copy.deepcopy(state)
        if name.startswith('physical-'):
            state['policy'][2] += 1
        elif name.endswith('-up'):
            state['counts']['key_release'] += 1
        elif name == 'ei-chinese-commit-down':
            state['counts']['preedit'] += 1
            state['counts']['commit'] += 1
            state['preeditActive'] = False
            state['expectedCommit'] = True
        else:
            state['counts']['preedit'] += 2
            state['preeditActive'] = True
        rows.append({'name': name, 'before': before, 'after': copy.deepcopy(state)})
    return rows


def mutate_from(rows, index, change):
    change(rows[index]['after'])
    for row in rows[index + 1:]:
        change(row['before'])
        change(row['after'])


class OverlapAcceptance(unittest.TestCase):
    def test_accepts_both_physical_edges_and_all_six_ei_edges(self):
        result = evaluate(fixture())
        self.assertTrue(result['passed'])
        self.assertEqual([c['helperAdvanced'] for c in result['cases']], [False, True, True, False, False, False, False, False])
        self.assertFalse(result['nativeTransportProvenanceVerified'])
        self.assertFalse(result['fullInputCoverageVerified'])

    def test_missing_suppressed_physical_down_fails(self):
        rows = fixture()
        mutate_from(rows, 1, lambda s: s['policy'].__setitem__(2, s['policy'][2] - 1))
        self.assertFalse(evaluate(rows)['passed'])

    def test_missing_suppressed_physical_up_fails(self):
        rows = fixture()
        mutate_from(rows, 2, lambda s: s['policy'].__setitem__(2, s['policy'][2] - 1))
        self.assertFalse(evaluate(rows)['passed'])

    def test_ei_false_positive_fails(self):
        rows = fixture()
        mutate_from(rows, 3, lambda s: s['policy'].__setitem__(2, s['policy'][2] + 1))
        self.assertFalse(evaluate(rows)['passed'])

    def test_forwarded_physical_edge_not_suppressed_fails(self):
        rows = fixture()
        mutate_from(rows, 1, lambda s: s['counts'].__setitem__('key_press', s['counts']['key_press'] + 1))
        self.assertFalse(evaluate(rows)['passed'])

    def test_no_actual_chinese_commit_fails(self):
        rows = fixture()
        mutate_from(rows, 6, lambda s: s.__setitem__('expectedCommit', False))
        self.assertFalse(evaluate(rows)['passed'])

    def test_partial_or_reordered_run_rejected(self):
        with self.assertRaises(ValueError):
            evaluate(fixture()[:-1])
        rows = fixture()
        rows[1], rows[2] = rows[2], rows[1]
        with self.assertRaises(ValueError):
            evaluate(rows)

    def test_original_owner_change_rejected(self):
        rows = fixture()
        mutate_from(rows, 2, lambda s: s.__setitem__('owner', ':1.99'))
        with self.assertRaises(ValueError):
            evaluate(rows)

    def test_unaccounted_gap_rejected(self):
        rows = fixture()
        rows[3]['before']['policy'][2] += 1
        with self.assertRaises(ValueError):
            evaluate(rows)

    def test_blocked_or_wrong_ime_rejected(self):
        for field, value in [('engine', 'xkb:us::eng'), ('nativeWayland', False)]:
            rows = fixture()
            rows[1]['after'][field] = value
            with self.assertRaises(ValueError):
                evaluate(rows)
        rows = fixture()
        rows[1]['after']['policy'][3] = True
        with self.assertRaises(ValueError):
            evaluate(rows)


if __name__ == '__main__':
    unittest.main()
