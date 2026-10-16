import assert from 'node:assert/strict';
import {test} from 'node:test';
import {WindowInventory, screenBlocked} from './inventory.mjs';

test('session state is fail-closed for missing fields, lock, shield animation and greeter', () => {
    const shield = {actor: {visible: false}, active: false, locked: false};
    const mode = {isLocked: false, isGreeter: false, hasWindows: true};
    assert.equal(screenBlocked(shield, mode), false);
    assert.equal(screenBlocked(undefined, mode), true);
    assert.equal(screenBlocked(shield, undefined), true);
    for (const field of ['active', 'locked']) {
        assert.equal(screenBlocked({...shield, [field]: undefined}, mode), true);
        assert.equal(screenBlocked({...shield, [field]: true}, mode), true);
    }
    for (const field of ['isLocked', 'isGreeter', 'hasWindows']) {
        assert.equal(screenBlocked(shield, {...mode, [field]: undefined}), true);
    }
    assert.equal(screenBlocked({...shield, actor: {visible: true}}, mode), true);
    assert.equal(screenBlocked(shield, {...mode, isLocked: true}), true);
    assert.equal(screenBlocked(shield, {...mode, hasWindows: false}), true);
});

function fixture() {
    const state = {windows: [{id: '7', title: 'Fixture', appId: 'org.fixture', pid: 123, peerPid: true}], blocked: false, calls: 0};
    const inventory = new WindowInventory('epoch', () => state.windows,
        window => window, () => state.calls++, () => state.blocked);
    return {state, inventory};
}

test('activation requires exact window, lifetime, PID, policy and trusted sender', () => {
    for (const change of [args => args[0] = 'other-epoch', args => args[1] = 'Fixture',
        args => args[2]++, args => args[3]++, args => args[4] = false,
        args => args[5] = ':1.9', args => args[6] = '']) {
        const {state, inventory} = fixture();
        const args = ['epoch', '7', inventory.list()[0][1], 123, true, ':1.2', ':1.2'];
        change(args);
        assert.equal(inventory.activate(...args), false);
        assert.equal(state.calls, 0);
    }
    const {state, inventory} = fixture();
    assert.equal(inventory.activate('epoch', '7', inventory.list()[0][1], 123, true, ':1.2', ':1.2'), true);
    assert.equal(state.calls, 1);
});

test('same ID and PID on a replacement object cannot inherit activation', () => {
    const {state, inventory} = fixture();
    const generation = inventory.list()[0][1];
    state.windows = [{...state.windows[0]}];
    assert.equal(inventory.activate('epoch', '7', generation, 123, true, ':1.2', ':1.2'), false);
    assert.equal(state.calls, 0);
});

test('X11 PID and locked session never permit activation', () => {
    const {state, inventory} = fixture();
    const generation = inventory.list()[0][1];
    state.windows[0].peerPid = false;
    assert.equal(inventory.activate('epoch', '7', generation, 123, true, ':1.2', ':1.2'), false);
    state.blocked = true;
    assert.throws(() => inventory.list());
    assert.equal(state.calls, 0);
});

test('duplicate IDs and changing PID during final native read never activate', () => {
    const {state, inventory} = fixture();
    const generation = inventory.list()[0][1];
    state.windows.push({...state.windows[0]});
    assert.equal(inventory.activate('epoch', '7', generation, 123, true, ':1.2', ':1.2'), false);
    state.windows.pop();
    let reads = 0;
    inventory.describe = window => ({...window, pid: ++reads >= 3 ? 999 : 123});
    assert.equal(inventory.activate('epoch', '7', generation, 123, true, ':1.2', ':1.2'), false);
    assert.equal(state.calls, 0);
});
