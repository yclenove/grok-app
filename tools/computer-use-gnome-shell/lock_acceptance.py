"""Strict native lock observation oracle, not authorization or atomic-stop proof.

The harness subscribes before the unlocked baseline and collects Changed signals
before making its post-lock GetState call. Readback must not manufacture the only
evidence of a transition. Transport/session provenance is checked separately.
"""
import re

FIELDS = {'owner', 'shellPid', 'session', 'seat', 'nativeWayland',
          'sessionActive', 'remote', 'loginLocked', 'policy'}


def _state(state):
    if not isinstance(state, (list, tuple)) or len(state) != 4:
        raise ValueError('four policy fields required')
    version, epoch, serial, blocked = state
    if type(version) is not int or version != 1:
        raise ValueError('healthy version one policy required')
    if not isinstance(epoch, str) or not re.fullmatch(r'[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}', epoch):
        raise ValueError('original helper epoch required')
    if type(serial) is not int or not 0 < serial < 0xffffffff or type(blocked) is not bool:
        raise ValueError('invalid or exhausted policy serial')


def _frame(frame):
    if not isinstance(frame, dict) or set(frame) != FIELDS:
        raise ValueError('exact native session snapshot required')
    if not isinstance(frame['owner'], str) or not re.fullmatch(r':\d+\.\d+', frame['owner']):
        raise ValueError('unique Shell owner required')
    if type(frame['shellPid']) is not int or frame['shellPid'] <= 1:
        raise ValueError('original Shell process required')
    if not isinstance(frame['session'], str) or not re.fullmatch(r'[A-Za-z0-9]+', frame['session']):
        raise ValueError('login session required')
    if frame['seat'] != 'seat0' or frame['nativeWayland'] is not True or frame['sessionActive'] is not True or frame['remote'] is not False:
        raise ValueError('local active native seat0 Wayland session required')
    if type(frame['loginLocked']) is not bool:
        raise ValueError('authoritative login lock state required')
    _state(frame['policy'])


def evaluate(record):
    if not isinstance(record, dict) or set(record) != {'before', 'after', 'changedBeforeReadback'}:
        raise ValueError('baseline, readback and prior notifications required')
    before, after = record['before'], record['after']
    _frame(before)
    _frame(after)
    for key in FIELDS - {'policy', 'loginLocked'}:
        if before[key] != after[key]:
            raise ValueError('original compositor/session identity changed')
    if before['loginLocked'] or before['policy'][3]:
        raise ValueError('a genuinely unlocked baseline is required')
    epoch, serial = before['policy'][1:3]
    if after['policy'][1] != epoch or after['policy'][2] < serial:
        raise ValueError('helper restarted or counter regressed')
    signals = record['changedBeforeReadback']
    if not isinstance(signals, list) or len(signals) > 256:
        raise ValueError('bounded original notification list required')
    blocked_notification = False
    last = serial
    for signal in signals:
        if not isinstance(signal, dict) or set(signal) != {'sender', 'state'}:
            raise ValueError('exact original Changed notification required')
        _state(signal['state'])
        if signal['sender'] != before['owner'] or signal['state'][1] != epoch:
            raise ValueError('stale or replacement notification')
        current = signal['state'][2]
        if current < last or current > after['policy'][2]:
            raise ValueError('notification/readback order violated')
        blocked_notification |= signal['state'][3] and current > serial
        last = current
    readback_blocked = after['loginLocked'] and after['policy'][3] and after['policy'][2] > serial
    return {'passed': bool(blocked_notification and readback_blocked),
            'blockedNotificationBeforeReadback': bool(blocked_notification),
            'blockedReadbackAndLoginState': bool(readback_blocked),
            'nativeTransportProvenanceVerified': False,
            'atomicStopVerified': False, 'appGrantRevocationVerified': False,
            'unlockRecoveryVerified': False, 'fullInputCoverageVerified': False}
