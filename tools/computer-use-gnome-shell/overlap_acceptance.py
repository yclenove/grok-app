"""Strict same-key native EI/physical overlap acceptance; never a grant source.

The EI source holds N while owned USB sends N down/up. Both physical edges must
advance the loss token even when seat-wide suppression forwards neither edge.
The six EI edges must still compose/commit and must never advance that token.
Transport provenance and original owner retirement require separate evidence.
"""
from ime_acceptance import COUNTS, _snapshot

CASES = ('ei-compose-start-down', 'physical-overlap-down', 'physical-overlap-up',
         'ei-compose-start-up', 'ei-compose-continue-down', 'ei-compose-continue-up',
         'ei-chinese-commit-down', 'ei-chinese-commit-up')


def evaluate(rows):
    if not isinstance(rows, list) or len(rows) != len(CASES):
        raise ValueError('all eight ordered overlap edges required')
    previous = None
    results = []
    for row, name in zip(rows, CASES):
        if not isinstance(row, dict) or set(row) != {'name', 'before', 'after'} or row['name'] != name:
            raise ValueError('missing, duplicate, or reordered overlap edge')
        before, after = row['before'], row['after']
        _snapshot(before)
        _snapshot(after)
        if previous is not None and before != previous:
            raise ValueError('unaccounted input or state between overlap edges')
        if before['owner'] != after['owner'] or before['pid'] != after['pid'] or before['policy'][1] != after['policy'][1]:
            raise ValueError('original owner or helper epoch changed')
        if before['policy'][2] > after['policy'][2] or any(before['counts'][k] > after['counts'][k] for k in COUNTS):
            raise ValueError('counter regressed')
        if previous is None and (any(before['counts'].values()) or before['preeditActive'] or before['expectedCommit']):
            raise ValueError('overlap fixture not pristine')
        delta = {k: after['counts'][k] - before['counts'][k] for k in COUNTS}
        physical = name.startswith('physical-')
        if physical:
            delivered = (not any(delta.values()) and before['preeditActive'] and after['preeditActive']
                         and not before['expectedCommit'] and not after['expectedCommit'])
        elif name.endswith('-up'):
            delivered = (delta['key_release'] == 1 and all(delta[k] == 0 for k in COUNTS - {'key_release'})
                         and all(before[k] == after[k] for k in ('preeditActive', 'expectedCommit')))
        elif name == 'ei-chinese-commit-down':
            delivered = (before['preeditActive'] and not before['expectedCommit']
                         and not after['preeditActive'] and after['expectedCommit']
                         and delta['commit'] == 1 and delta['key_press'] == delta['key_release'] == 0)
        else:
            delivered = (delta['preedit'] > 0 and delta['commit'] == delta['key_press'] == delta['key_release'] == 0
                         and after['preeditActive'] and not after['expectedCommit'])
        advanced = after['policy'][2] > before['policy'][2]
        results.append({'name': name, 'deliveryOrSuppressionVerified': delivered,
                        'helperAdvanced': advanced, 'passed': delivered and advanced == physical})
        previous = after
    return {'passed': all(case['passed'] for case in results), 'cases': results,
            'nativeTransportProvenanceVerified': False, 'humanPhysicalInputVerified': False,
            'appGrantRevocationVerified': False, 'fullInputCoverageVerified': False}
