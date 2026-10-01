"""Strict installed portal pointer receipt oracle; not a grant or image decoder.

Caller must independently verify binaries, original process/portal retirement,
actual PNG pixels, VM identity and compositor provenance. Synthetic rows test
this oracle only; they do not constitute installed acceptance.
"""


def require(condition, message):
    if not condition:
        raise ValueError(message)


def one(rows, event):
    matches = [row for row in rows if row.get('event') == event]
    require(len(matches) == 1, 'exactly one ' + event + ' required')
    return matches[0]


def matched_regions(row):
    require(row.get('event') == 'OWNED_EI_DIAGNOSTIC', 'not an EI diagnostic')
    stream = row.get('streamMapping')
    require(isinstance(stream, dict) and stream.get('truncated') is False
            and isinstance(stream.get('bytes'), list) and 0 < len(stream['bytes']) <= 512
            and all(type(b) is int and 0 <= b <= 255 for b in stream['bytes']), 'invalid stream mapping')
    count = 0
    for device in row['devices']:
        if device['type'] != 1 or device['resumed'] is not True or 2 not in device['capabilities']:
            continue
        for region in device['regions']:
            require('overflow' not in region, 'region overflow')
            mapping = region['mapping']
            if mapping is not None and mapping == stream:
                require(all(type(region[k]) is int and region[k] > 0 for k in ('width', 'height'))
                        and region['physicalScale'] > 0, 'invalid region geometry')
                count += 1
    require(row.get('matchedRegions') == count, 'summary is not actual region count')
    require(row.get('authorityChangedByDiagnostic') is False, 'diagnostic changed authority')
    return count


def evaluate(rows, diagnostics=False, *, prior_clicks=0):
    # The second cycle must retain the native counter and edge history, not
    # reset them to manufacture an independent first-click receipt.
    require(type(prior_clicks) is int and prior_clicks in (0, 1), 'invalid prior click count')
    events = ['PICKER_PENDING', 'PORTAL_POINTER_OBSERVATION', 'POINTER_EFFECT',
              'GRANT_READY', 'AUTOMATIC_REVOCATION', 'ORIGINAL_OWNERS_JOINED']
    values = [one(rows, event) for event in events]
    require([rows.index(value) for value in values] == sorted(rows.index(value) for value in values),
            'receipt ordering changed')
    pending, picture, effect, ready, revoked, closed = values
    obs = picture['observation']
    require(obs['runId'] == pending['run'] and obs['targetId'] == ready['target'], 'wrong run/target')
    require(obs['coordinateSpace'] == 'image-pixels', 'coordinates are not image pixels')
    require(effect['originalSnapshot'] == obs['snapshotId'], 'click uses another snapshot')
    require(effect['imagePoint'] == picture['imagePoint'], 'click point changed')
    x, y = picture['imagePoint']
    left, top, right, bottom = picture['targetBox']
    require(all(type(v) is int for v in (x, y, left, top, right, bottom)), 'non-integer pixels')
    require(0 <= left <= x <= right < obs['image']['width']
            and 0 <= top <= y <= bottom < obs['image']['height'], 'point outside target/image')
    edges = [[1, True], [1, False]]
    require(effect['before'] == [prior_clicks, edges * prior_clicks]
            and effect['after'] == [prior_clicks + 1, edges * (prior_clicks + 1)],
            'exact native GTK click edges/effect required')
    require(effect.get('nativeGtkEffect') is True and effect.get('appliedAloneIsNotProof') is True,
            'submission is not native effect evidence')
    require(ready['gtkReceipt'] == [[65293, True], [65293, False]]
            and ready['realPortalConsent'] is True and ready['productionRegistry'] is True
            and ready['experimentalObserver'] is True, 'incorrect acceptance path')
    before, after = ready['helperBaseline'], revoked['helperAfter']
    require(len(before) == len(after) == 4 and before[0] == after[0] == 1
            and before[1] == after[1] and bool(before[1])
            and type(before[2]) is int and type(after[2]) is int
            and 0 <= before[2] < after[2] < 0xffffffff
            and before[3] is False and after[3] is False, 'not same-epoch physical takeover')
    require(revoked['oldTargetRejected'] is True and revoked['externalStopRequired'] is False
            and revoked['freshRecoveryVerified'] is False, 'revocation proof missing or overclaimed')
    require(closed['result'] == {'Ok': None} and closed['state'] == revoked['terminal']
            and closed['state'].startswith('Closed {'), 'original owners did not close successfully')
    require(all(closed[k] is False for k in ('appAcpMcpVerified', 'atomicStopVerified', 'stockGnomeSupported')),
            'experimental result overclaims product support')
    snapshots = [r for r in rows if r.get('event') == 'OWNED_EI_DIAGNOSTIC']
    if diagnostics:
        require(bool(snapshots) and matched_regions(snapshots[-1]) == 1, 'final mapping not unique')
        require(all(matched_regions(r) <= 1 for r in snapshots), 'duplicate absolute mapping observed')
    else:
        require(not snapshots, 'diagnostics present in default-build control')
    return {'passed': True, 'pointerEffect': True, 'oldGrantRetired': True,
            'nativeProvenanceIndependentlyVerified': False, 'fullGoalComplete': False}
