"""Synthetic oracle tests only; no claim of installed consent or native effects."""
import copy
import unittest
from portal_pointer_acceptance import one
from portal_pointer_acceptance_test import rows as pointer_rows, diagnostic
from portal_recovery_acceptance import evaluate


def fixture():
    cycles = []
    history = []
    for n in (1, 2):
        rows = pointer_rows()
        pending = one(rows, 'PICKER_PENDING')
        pending.update(probePid=123, cycle=n, run=f'r{n}', attempt=f'a{n}', authorizationGeneration=n)
        picture = one(rows, 'PORTAL_POINTER_OBSERVATION')['observation']
        picture.update(runId=f'r{n}', targetId=f't{n}', snapshotId=f'p{n}')
        effect = one(rows, 'POINTER_EFFECT')
        effect.update(originalSnapshot=f'p{n}', before=[n-1, [[1, True], [1, False]]*(n-1)],
                      after=[n, [[1, True], [1, False]]*n])
        ready = one(rows, 'GRANT_READY')
        ready.update(cycle=n, target=f't{n}', snapshot=f's{n}', helperBaseline=[1, 'epoch', n*10, False],
                     gtkHistoryBefore=copy.deepcopy(history), gtkHistoryAfter=history+ready['gtkReceipt'])
        history = ready['gtkHistoryAfter'] + [[65505, True], [65505, False]]
        frame = copy.deepcopy(picture)
        frame['snapshotId'] = f's{n}'
        rows.insert(rows.index(ready), {'event':'PORTAL_OBSERVATION','observation':frame})
        one(rows, 'AUTOMATIC_REVOCATION')['helperAfter'] = [1, 'epoch', n*10+1, False]
        one(rows, 'ORIGINAL_OWNERS_JOINED').update(cycle=n, run=f'r{n}', hostAttemptFenced=True, brokerStopped=True)
        rows.insert(0, {'event':'CYCLE_BEGIN','cycle':n,'probePid':123})
        rows.append({'event':'CYCLE_END','cycle':n,'probePid':123})
        if n == 2:
            for stage, at in [('before-fresh-input', 2), ('after-fresh-input', 5)]:
                rows.insert(at, {'event':'STALE_GRANT_REJECTED','stage':stage,'probePid':123,
                    'newRun':'r2','newTarget':'t2','oldTicketCurrent':False,'oldSelectionRejected':True,
                    'freshTargetAlive':True,'oldRequests':[
                        {'run':'r1','target':'t1','snapshot':'s1','action':'key'},
                        {'run':'r1','target':'t1','snapshot':'p1','action':'click'}]})
        cycles.append(rows)
    return cycles[0] + [
        {'event':'RECOVERY_READY','probePid':123,'oldRun':'r1','oldAttempt':'a1',
         'oldAuthorizationGeneration':1,'oldOwnersJoined':True,'oldBrokerStopped':True,'automaticRegrant':False},
        {'event':'RECOVERY_REQUESTED','probePid':123,'gtkEdges':[[1,True],[1,False]],
         'explicitUiIntent':True,'automaticRegrant':False},
    ] + cycles[1] + [
        {'event':'RECOVERY_COMPLETE','probePid':123,'oldRun':'r1','newRun':'r2',
         'sameProcessRegistryBrokerGrantsGtk':True,'freshRecoveryVerified':True,'bothOriginalOwnersJoined':True,
         'automaticRegrant':False,'appAcpMcpVerified':False,'hostAuthorizationCommitted':False,
         'stockGnomeSupported':False,'atomicStopVerified':False}]


class RecoveryOracle(unittest.TestCase):
    def reject(self, change):
        rows = fixture()
        change(rows)
        with self.assertRaises((ValueError, KeyError)):
            evaluate(rows)

    def test_valid_but_not_provenance_or_completion(self):
        result = evaluate(fixture())
        self.assertTrue(result['passed'])
        self.assertFalse(result['nativeProvenanceIndependentlyVerified'])
        self.assertFalse(result['fullGoalComplete'])

    def test_no_automatic_regrant(self):
        self.reject(lambda r: one(r,'RECOVERY_REQUESTED').update(automaticRegrant=True))

    def test_real_ui_intent_required(self):
        self.reject(lambda r: one(r,'RECOVERY_REQUESTED').update(gtkEdges=[]))

    def test_original_process_required(self):
        self.reject(lambda r: one(r,'RECOVERY_COMPLETE').update(probePid=124))

    def test_fresh_pending_and_attempt_required(self):
        for field, value in [('run','r1'),('attempt','a1'),('authorizationGeneration',1)]:
            self.reject(lambda r, f=field, v=value: [x for x in r if x['event']=='PICKER_PENDING'][1].update({f:v}))

    def test_owner_join_before_explicit_request(self):
        self.reject(lambda r: one(r,'RECOVERY_READY').update(oldOwnersJoined=False))
        self.reject(lambda r: one(r,'RECOVERY_READY').update(oldBrokerStopped=False))

    def test_new_target_and_snapshot_required(self):
        self.reject(lambda r: [x for x in r if x['event']=='GRANT_READY'][1].update(target='t1'))

    def test_no_counter_reset(self):
        self.reject(lambda r: [x for x in r if x['event']=='POINTER_EFFECT'][1].update(before=[0,[]]))

    def test_no_keyboard_history_reset(self):
        def reset(r):
            row = [x for x in r if x['event']=='GRANT_READY'][1]
            row.update(gtkHistoryBefore=[], gtkHistoryAfter=row['gtkReceipt'])
        self.reject(reset)

    def test_both_stale_checks_required(self):
        self.reject(lambda r: r.remove(next(x for x in r if x['event']=='STALE_GRANT_REJECTED')))

    def test_original_requests_not_substituted(self):
        self.reject(lambda r: next(x for x in r if x['event']=='STALE_GRANT_REJECTED')['oldRequests'][0].update(snapshot='other'))

    def test_no_old_authority_resurrection(self):
        self.reject(lambda r: next(x for x in r if x['event']=='STALE_GRANT_REJECTED').update(oldTicketCurrent=True))

    def test_stale_handle_cannot_disrupt_fresh_grant(self):
        self.reject(lambda r: next(x for x in r if x['event']=='STALE_GRANT_REJECTED').update(freshTargetAlive=False))

    def test_no_external_cleanup_substituted(self):
        self.reject(lambda r: [x for x in r if x['event']=='AUTOMATIC_REVOCATION'][1].update(externalStopRequired=True))

    def test_scope_cannot_be_upgraded(self):
        for field in ('appAcpMcpVerified','stockGnomeSupported','atomicStopVerified','hostAuthorizationCommitted'):
            self.reject(lambda r, f=field: one(r,'RECOVERY_COMPLETE').update({f:True}))

    def test_no_hidden_error_or_third_cycle(self):
        self.reject(lambda r: r.insert(1, {'event':'ERROR','message':'native owner failed'}))
        self.reject(lambda r: r.insert(1, copy.deepcopy(r[0])))

    def test_same_original_helper(self):
        self.reject(lambda r: [x for x in r if x['event']=='GRANT_READY'][1].update(helperBaseline=[1,'replacement',20,False]))

    def test_diagnostics_stay_per_cycle(self):
        rows=fixture()
        for at in reversed([i+1 for i,r in enumerate(rows) if r['event']=='CYCLE_BEGIN']):
            rows.insert(at, diagnostic())
        self.assertTrue(evaluate(rows, diagnostics=True)['passed'])
        with self.assertRaises(ValueError): evaluate(rows)


if __name__ == '__main__': unittest.main()
