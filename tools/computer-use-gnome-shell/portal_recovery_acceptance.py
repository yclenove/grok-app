"""Strict two-consent, same-original-process recovery receipt oracle.

Not a provenance checker: verify exact binaries, real PNG pixels/portal objects,
native GTK/QMP input, compositor identity and original process joins separately.
Does not certify stock GNOME, App/ACP/MCP, lock recovery or atomic Stop.
"""
from portal_pointer_acceptance import evaluate as pointer, one, require


def evaluate(rows, diagnostics=False):
    allowed = {'CYCLE_BEGIN', 'CYCLE_END', 'PICKER_PENDING', 'PORTAL_POINTER_OBSERVATION',
               'POINTER_EFFECT', 'PORTAL_OBSERVATION', 'GRANT_READY', 'AUTOMATIC_REVOCATION',
               'ORIGINAL_OWNERS_JOINED', 'RECOVERY_READY', 'RECOVERY_REQUESTED',
               'STALE_GRANT_REJECTED', 'RECOVERY_COMPLETE', 'OWNED_EI_DIAGNOSTIC'}
    require(all(r.get('event') in allowed for r in rows), 'unexpected/error receipt')
    starts = [i for i, r in enumerate(rows) if r.get('event') == 'CYCLE_BEGIN']
    ends = [i for i, r in enumerate(rows) if r.get('event') == 'CYCLE_END']
    require(len(starts) == len(ends) == 2 and starts[0] < ends[0] < starts[1] < ends[1],
            'exactly two nonoverlapping original cycles required')
    ready = one(rows, 'RECOVERY_READY')
    intent = one(rows, 'RECOVERY_REQUESTED')
    done = one(rows, 'RECOVERY_COMPLETE')
    require(ends[0] < rows.index(ready) < rows.index(intent) < starts[1]
            and ends[1] < rows.index(done) == len(rows) - 1, 'no automatic or premature recovery')
    pid = ready['probePid']
    require(type(pid) is int and pid > 1 and intent['probePid'] == done['probePid'] == pid,
            'recovery changed process')
    require(ready['oldOwnersJoined'] is True and ready['oldBrokerStopped'] is True
            and ready['automaticRegrant'] is False, 'old owners/Host not retired before UI request')
    require(intent['explicitUiIntent'] is True and intent['automaticRegrant'] is False
            and intent['gtkEdges'] == [[1, True], [1, False]], 'native explicit UI intent required')
    cycles = [rows[a:b+1] for a, b in zip(starts, ends)]
    pending, grants, effects, pictures, frames = [], [], [], [], []
    for index, cycle in enumerate(cycles):
        require(cycle[0]['cycle'] == cycle[-1]['cycle'] == index + 1
                and cycle[0]['probePid'] == cycle[-1]['probePid'] == pid, 'cycle identity changed')
        pointer(cycle, diagnostics, prior_clicks=index)
        p = one(cycle, 'PICKER_PENDING')
        g = one(cycle, 'GRANT_READY')
        c = one(cycle, 'ORIGINAL_OWNERS_JOINED')
        frame = one(cycle, 'PORTAL_OBSERVATION')['observation']
        require(p['probePid'] == pid and p['cycle'] == g['cycle'] == c['cycle'] == index + 1,
                'picker identity or cycle changed')
        require(c['run'] == p['run'] and c['hostAttemptFenced'] is True
                and c['brokerStopped'] is True, 'Host cleanup missing')
        require(frame['runId'] == p['run'] and frame['targetId'] == g['target']
                and frame['snapshotId'] == g['snapshot'], 'keyboard frame not original')
        require(g['gtkHistoryAfter'] == g['gtkHistoryBefore'] + g['gtkReceipt'],
                'native keyboard history reset or changed')
        pending.append(p)
        grants.append(g)
        effects.append(one(cycle, 'POINTER_EFFECT'))
        pictures.append(one(cycle, 'PORTAL_POINTER_OBSERVATION')['observation'])
        frames.append(frame)
    first, second = pending
    retired = one(cycles[0], 'AUTOMATIC_REVOCATION')['helperAfter']
    require(grants[1]['helperBaseline'][1] == retired[1]
            and grants[1]['helperBaseline'][2] > retired[2],
            'original helper changed or fresh physical consent evidence missing')
    require(first['run'] != second['run'] and first['attempt'] != second['attempt']
            and type(first['authorizationGeneration']) is int
            and type(second['authorizationGeneration']) is int
            and 0 < first['authorizationGeneration'] < second['authorizationGeneration'],
            'fresh Host attempt/run required')
    require(ready['oldRun'] == done['oldRun'] == first['run']
            and ready['oldAttempt'] == first['attempt']
            and ready['oldAuthorizationGeneration'] == first['authorizationGeneration']
            and done['newRun'] == second['run'], 'recovery refers to different attempts')
    require(grants[0]['target'] != grants[1]['target']
            and len({f['snapshotId'] for f in frames + pictures}) == 4,
            'fresh targets and observations required')
    require(effects[1]['before'] == effects[0]['after'], 'native pointer counter/history reset')
    before = grants[1]['gtkHistoryBefore']
    require(before[:len(grants[0]['gtkHistoryAfter'])] == grants[0]['gtkHistoryAfter'],
            'original GTK keyboard history lost')
    stale = [r for r in rows if r.get('event') == 'STALE_GRANT_REJECTED']
    require(len(stale) == 2 and [r['stage'] for r in stale] == ['before-fresh-input', 'after-fresh-input'],
            'old grant must be rejected both before and after fresh input')
    second_cycle = cycles[1]
    require(second_cycle.index(second) < second_cycle.index(stale[0])
            < second_cycle.index(one(second_cycle, 'PORTAL_POINTER_OBSERVATION'))
            < second_cycle.index(effects[1]) < second_cycle.index(stale[1])
            < second_cycle.index(grants[1]), 'stale check ordering changed')
    expected = [{'run':first['run'], 'target':grants[0]['target'],
                 'snapshot':frame['snapshotId'], 'action':action}
                for frame, action in ((frames[0], 'key'), (pictures[0], 'click'))]
    for row in stale:
        require(row['probePid'] == pid and row['newRun'] == second['run']
                and row['newTarget'] == grants[1]['target'] and row['oldRequests'] == expected
                and row['oldTicketCurrent'] is False and row['oldSelectionRejected'] is True
                and row['freshTargetAlive'] is True, 'old grant resurrected or fresh grant disrupted')
    require(all(done[k] is True for k in ('sameProcessRegistryBrokerGrantsGtk',
                                         'freshRecoveryVerified', 'bothOriginalOwnersJoined'))
            and all(done[k] is False for k in ('automaticRegrant', 'appAcpMcpVerified',
                                              'hostAuthorizationCommitted', 'stockGnomeSupported',
                                              'atomicStopVerified')), 'recovery scope overclaimed')
    # No extra evidence from a third run or out-of-cycle action is allowed.
    for event in ('PICKER_PENDING', 'GRANT_READY', 'PORTAL_POINTER_OBSERVATION',
                  'PORTAL_OBSERVATION', 'POINTER_EFFECT', 'AUTOMATIC_REVOCATION', 'ORIGINAL_OWNERS_JOINED'):
        require(sum(r.get('event') == event for r in rows) == 2, 'extra or missing ' + event)
    require(sum(r.get('event') == 'OWNED_EI_DIAGNOSTIC' for r in rows)
            == sum(r.get('event') == 'OWNED_EI_DIAGNOSTIC' for cycle in cycles for r in cycle),
            'out-of-cycle diagnostics')
    return {'passed': True, 'sameProcessFreshConsent': True, 'oldGrantRemainsRejected': True,
            'nativeProvenanceIndependentlyVerified': False, 'fullGoalComplete': False}
