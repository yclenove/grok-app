"""Synthetic oracle regression, explicitly not native compositor acceptance."""
import copy
import unittest
from portal_pointer_acceptance import evaluate, matched_regions


def rows():
    return [
        {'event': 'PICKER_PENDING', 'run': 'r'},
        {'event': 'PORTAL_POINTER_OBSERVATION', 'imagePoint': [20, 20], 'targetBox': [10, 10, 30, 30],
         'observation': {'runId': 'r', 'targetId': 't', 'snapshotId': 's', 'coordinateSpace': 'image-pixels', 'image': {'width': 40, 'height': 40}}},
        {'event': 'POINTER_EFFECT', 'imagePoint': [20, 20], 'originalSnapshot': 's', 'before': [0, []],
         'after': [1, [[1, True], [1, False]]], 'nativeGtkEffect': True, 'appliedAloneIsNotProof': True},
        {'event': 'GRANT_READY', 'target': 't', 'gtkReceipt': [[65293, True], [65293, False]],
         'realPortalConsent': True, 'productionRegistry': True, 'experimentalObserver': True, 'helperBaseline': [1, 'epoch', 7, False]},
        {'event': 'AUTOMATIC_REVOCATION', 'helperAfter': [1, 'epoch', 8, False], 'oldTargetRejected': True,
         'externalStopRequired': False, 'freshRecoveryVerified': False, 'terminal': 'Closed { reason: None }'},
        {'event': 'ORIGINAL_OWNERS_JOINED', 'result': {'Ok': None}, 'state': 'Closed { reason: None }',
         'appAcpMcpVerified': False, 'atomicStopVerified': False, 'stockGnomeSupported': False},
    ]


def diagnostic():
    mapping = {'bytes': [97], 'truncated': False}
    return {'event': 'OWNED_EI_DIAGNOSTIC', 'streamMapping': mapping, 'matchedRegions': 1,
            'authorityChangedByDiagnostic': False, 'devices': [{'type': 1, 'resumed': True, 'capabilities': [2],
            'regions': [{'mapping': mapping, 'width': 40, 'height': 40, 'physicalScale': 1.0}]}]}


class PortalPointerOracle(unittest.TestCase):
    def test_valid_receipts_do_not_claim_provenance(self):
        self.assertFalse(evaluate(rows())['nativeProvenanceIndependentlyVerified'])

    def test_exact_one_ordered_event(self):
        for mutated in (rows()[:-1], rows()+[rows()[2]], list(reversed(rows()))):
            with self.assertRaises(ValueError): evaluate(mutated)

    def test_wrong_snapshot(self):
        value=rows();value[2]['originalSnapshot']='stale'
        with self.assertRaises(ValueError): evaluate(value)

    def test_wrong_point_or_geometry(self):
        for field, replacement in [('imagePoint', [35, 20]), ('targetBox', [10, 10, 50, 30])]:
            value=rows();value[1][field]=replacement
            with self.assertRaises(ValueError): evaluate(value)

    def test_no_effect_or_wrong_edges(self):
        for after in ([0, []], [1, [[1, True]]], [2, [[1, True], [1, False]]]):
            value=rows();value[2]['after']=after
            with self.assertRaises(ValueError): evaluate(value)

    def test_takeover_needs_same_epoch_new_generation(self):
        for after in ([1,'other',8,False],[1,'epoch',7,False],[1,'epoch',8,True]):
            value=rows();value[4]['helperAfter']=after
            with self.assertRaises(ValueError): evaluate(value)

    def test_original_owner_failure(self):
        value=rows();value[5]['result']={'Err':'join failed'}
        with self.assertRaises(ValueError): evaluate(value)

    def test_no_false_release_claim(self):
        value=rows();value[5]['stockGnomeSupported']=True
        with self.assertRaises(ValueError): evaluate(value)

    def test_default_excludes_instrumentation(self):
        with self.assertRaises(ValueError): evaluate([diagnostic()]+rows())
        self.assertTrue(evaluate([diagnostic()]+rows(), diagnostics=True)['passed'])

    def test_duplicate_device_not_ignored(self):
        row=diagnostic();row['devices']*=2;row['matchedRegions']=2
        self.assertEqual(matched_regions(row),2)
        with self.assertRaises(ValueError): evaluate([row]+rows(),diagnostics=True)

    def test_bad_diagnostic_summary(self):
        row=diagnostic();row['devices']*=2
        with self.assertRaises(ValueError): matched_regions(row)

    def test_unpaired_or_paused_region_is_not_a_match(self):
        for key,value in [('resumed',False),('type',0),('capabilities',[])]:
            row=copy.deepcopy(diagnostic());row['devices'][0][key]=value;row['matchedRegions']=0
            self.assertEqual(matched_regions(row),0)


if __name__ == '__main__': unittest.main()
