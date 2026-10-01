import copy
import unittest

from lock_acceptance import evaluate


def fixture():
    state = [1, 'ed30c04d-6431-4be0-93da-4975bbdf4090', 4, False]
    before = {'owner': ':1.42', 'shellPid': 1200, 'session': '7', 'seat': 'seat0',
              'nativeWayland': True, 'sessionActive': True, 'remote': False,
              'loginLocked': False, 'policy': state}
    after = copy.deepcopy(before)
    after.update(loginLocked=True, policy=[*state[:2], 5, True])
    return {'before': before, 'after': after,
            'changedBeforeReadback': [{'sender': ':1.42', 'state': after['policy'][:]}]}


class LockAcceptance(unittest.TestCase):
    def test_actual_sequence_passes_without_claiming_revocation(self):
        result = evaluate(fixture())
        self.assertTrue(result['passed'])
        for key in ('nativeTransportProvenanceVerified', 'atomicStopVerified',
                    'appGrantRevocationVerified', 'unlockRecoveryVerified', 'fullInputCoverageVerified'):
            self.assertFalse(result[key])

    def test_poll_only_observation_cannot_pass(self):
        row = fixture()
        row['changedBeforeReadback'] = []
        self.assertFalse(evaluate(row)['passed'])

    def test_initial_naturally_locked_state_rejected(self):
        for key in ('loginLocked', 'policy'):
            row = fixture()
            if key == 'policy':
                row['before'][key][3] = True
            else:
                row['before'][key] = True
            with self.assertRaises(ValueError):
                evaluate(row)

    def test_after_must_be_locked_in_both_sources(self):
        for login, helper in ((False, True), (True, False), (False, False)):
            row = fixture()
            row['after']['loginLocked'] = login
            row['after']['policy'][3] = helper
            self.assertFalse(evaluate(row)['passed'])

    def test_no_generation_advance_is_not_lock_observation(self):
        row = fixture()
        row['after']['policy'][2] = 4
        row['changedBeforeReadback'][0]['state'][2] = 4
        self.assertFalse(evaluate(row)['passed'])

    def test_unblocked_notification_does_not_substitute_for_lock(self):
        row = fixture()
        row['changedBeforeReadback'][0]['state'][3] = False
        self.assertFalse(evaluate(row)['passed'])

    def test_original_identity_cannot_change(self):
        for key, value in (('owner', ':1.43'), ('shellPid', 1201), ('session', '8')):
            row = fixture()
            row['after'][key] = value
            with self.assertRaises(ValueError):
                evaluate(row)

    def test_epoch_reset_and_regression_rejected(self):
        for index, value in ((1, 'fd30c04d-6431-4be0-93da-4975bbdf4090'), (2, 3)):
            row = fixture()
            row['after']['policy'][index] = value
            with self.assertRaises(ValueError):
                evaluate(row)

    def test_foreign_or_stale_signal_rejected(self):
        for change in ('sender', 'epoch'):
            row = fixture()
            if change == 'sender':
                row['changedBeforeReadback'][0]['sender'] = ':1.41'
            else:
                row['changedBeforeReadback'][0]['state'][1] = 'fd30c04d-6431-4be0-93da-4975bbdf4090'
            with self.assertRaises(ValueError):
                evaluate(row)

    def test_signal_cannot_postdate_readback_or_regress(self):
        for serial in (3, 6):
            row = fixture()
            row['changedBeforeReadback'][0]['state'][2] = serial
            with self.assertRaises(ValueError):
                evaluate(row)

    def test_nonnative_or_remote_session_rejected(self):
        for key, value in (('nativeWayland', False), ('remote', True), ('sessionActive', False), ('seat', '')):
            row = fixture()
            row['before'][key] = value
            with self.assertRaises(ValueError):
                evaluate(row)

    def test_missing_or_extra_fields_rejected(self):
        row = fixture()
        row['after']['syntheticFallback'] = True
        with self.assertRaises(ValueError):
            evaluate(row)
        with self.assertRaises(ValueError):
            evaluate({'before': row['before']})

    def test_bool_cannot_impersonate_numeric_policy(self):
        for index in (0, 2):
            row = fixture()
            row['before']['policy'][index] = True
            with self.assertRaises(ValueError):
                evaluate(row)

    def test_failed_or_exhausted_policy_rejected(self):
        for index, value in ((0, 0), (2, 0xffffffff)):
            row = fixture()
            row['after']['policy'][index] = value
            with self.assertRaises(ValueError):
                evaluate(row)


if __name__ == '__main__':
    unittest.main()
