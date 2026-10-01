import {test} from 'node:test';
import assert from 'node:assert/strict';
import {NativeEventWatch, PolicyState, classifySource, XML, screenBlocked} from './policy.js';
const epoch = 'f0194527-4330-4029-91e1-7911127739ab';
// Test constants intentionally differ from GI enum values; production passes
// the actual Clutter.InputMode.PHYSICAL constant, never a hardcoded number.
const physicalMode = 47;
const logicalMode = 81;
const device = (node, mode = physicalMode) => ({get_device_node: () => node, get_device_mode: () => mode});
const make = () => { const events = []; return {events, state: new PolicyState(epoch, s => events.push(s), physicalMode)}; };

test('native kernel source differs from libei despite identical PHYSICAL modes', () => {
  assert.equal(classifySource(device('/dev/input/event12'), physicalMode), 'physical');
  assert.equal(classifySource(device(null), physicalMode), 'virtual');
});
test('unknown, malformed, absent and throwing sources never qualify as virtual', () => {
  for (const d of [null, {}, device(''), device(undefined), device('/tmp/event1'), device('/dev/input/event1/x'), device(4), {get_device_node() {throw Error('gone');}}])
    assert.equal(classifySource(d, physicalMode), 'unknown');
});

test('logical aggregate keyboard is unknown despite its null native device node', () => {
  assert.equal(classifySource(device(null, logicalMode), physicalMode), 'unknown');
  const {state} = make(); state.setBlocked(false);
  state.input(device(null, logicalMode));
  assert.equal(state.serial, 2);
  state.input(device(null)); assert.equal(state.serial, 2);
});

test('absent malformed and throwing mode identity never certifies virtual input', () => {
  for (const source of [{...device(null), get_device_mode: () => undefined}, device(null, null), device(null, true),
    device(null, -1), {get_device_node: () => null},
    {get_device_node: () => null, get_device_mode() {throw Error('device gone');}}])
    assert.equal(classifySource(source, physicalMode), 'unknown');
});

test('caller must supply actual physical-mode enum before virtual exclusion', () => {
  for (const mode of [undefined, null, false, '47', NaN])
    assert.equal(classifySource(device(null), mode), 'unknown');
  const {events} = make();
  const state = new PolicyState(epoch, snapshot => events.push(snapshot));
  state.setBlocked(false); state.input(device(null));
  assert.equal(state.serial, 2);
});
test('input records only epoch/generation and never event content', () => {
  const {state, events} = make(); state.setBlocked(false);
  state.input(device('/dev/input/event2')); state.input(null);
  assert.deepEqual(events, [[1, epoch, 1, false], [1, epoch, 2, false], [1, epoch, 3, false]]);
  state.input(device(null)); assert.equal(events.length, 3);
});
test('lock/unlock pulse changes generation even if snapshot is unlocked again', () => {
  const {state} = make(); state.setBlocked(false); const before = state.snapshot();
  state.setBlocked(true); state.setBlocked(false);
  assert.equal(state.snapshot()[3], false); assert.equal(state.snapshot()[2], before[2] + 2);
});
test('fault/disable is sticky and duplicate calls do not restore readiness', () => {
  const {state, events} = make(); state.setBlocked(false); state.fail(); state.setBlocked(false); state.fail();
  state.input(device('/dev/input/event0'));
  assert.deepEqual(state.snapshot(), [1, epoch, 2, true]); assert.equal(events.length, 2);
});
test('serial exhaustion fails closed rather than wrapping into an old grant', () => {
  const {state} = make(); state.setBlocked(false); state.serial = 0xffffffff;
  state.input(device('/dev/input/event0')); state.setBlocked(false);
  assert.deepEqual(state.snapshot(), [1, epoch, 0xffffffff, true]);
});
test('invalid state fails closed; repeated state does not fabricate activity', () => {
  const {state, events} = make(); state.setBlocked(true); assert.equal(events.length, 0);
  state.setBlocked('false'); assert.equal(state.failed, true); assert.equal(state.blocked, true);
});
test('D-Bus surface has only a read method and content-free loss signal', () => {
  assert.equal((XML.match(/<method /g) || []).length, 1);
  assert.ok(XML.includes('name="GetState"')); assert.ok(!XML.includes('direction="in"'));
});
test('Ubuntu inherited user mode is accepted; early visible shield blocks before delayed active', () => {
  const shield = {actor: {visible: false}, active: false, locked: false};
  const mode = {currentMode: 'ubuntu', isLocked: false, isGreeter: false, hasWindows: true};
  assert.equal(screenBlocked(shield, mode), false);
  shield.actor.visible = true; assert.equal(screenBlocked(shield, mode), true);
  shield.actor.visible = false; mode.isLocked = true; assert.equal(screenBlocked(shield, mode), true);
  mode.isLocked = false; mode.isGreeter = true; assert.equal(screenBlocked(shield, mode), true);
  mode.isGreeter = false; delete shield.active; assert.equal(screenBlocked(shield, mode), true);
});
test('broadcast failure neither escapes an input callback nor restores permission', () => {
  const state = new PolicyState(epoch, () => {throw Error('bus disconnected');});
  assert.doesNotThrow(() => state.setBlocked(false));
  assert.equal(state.failed, true); assert.equal(state.blocked, true);
  state.setBlocked(false); assert.equal(state.blocked, true);
});

// Model Mutter's snapshot-of-watch-IDs reset and removal AFTER the callback.
// These contract tests are not evidence of native event delivery; the installed
// GNOME client-input probe must independently exercise the actual GI callback.
function nativeHarness() {
  const watches = new Map();
  const removed = [];
  let next = 1;
  let event = null;
  const {state} = make();
  state.setBlocked(false);
  const monitor = {
    add_user_active_watch(callback) { const id = next++; watches.set(id, callback); return id; },
    remove_watch(id) { removed.push(id); watches.delete(id); },
  };
  const watch = new NativeEventWatch(monitor, () => event, e => state.input(e.source), () => state.fail());
  function fire(value) {
    event = value;
    try {
      for (const id of [...watches.keys()]) {
        const callback = watches.get(id);
        if (callback) { callback(); monitor.remove_watch(id); }
      }
    } finally { event = null; }
  }
  return {watch, monitor, watches, removed, state, fire};
}

test('synchronous native one-shot re-arms for every event without same-reset recursion', () => {
  const h = nativeHarness(); h.watch.start();
  for (let i = 0; i < 100; i++) {
    h.fire({source: device('/dev/input/event4')});
    assert.equal(h.state.serial, i + 2);
    assert.equal(h.watches.size, 1);
  }
  assert.equal(h.state.blocked, false);
  h.watch.stop(); assert.equal(h.watches.size, 0);
});
test('native virtual events do not fabricate physical loss but keep observation armed', () => {
  const h = nativeHarness(); h.watch.start();
  h.fire({source: device(null)}); assert.equal(h.state.serial, 1);
  h.fire({source: device('/dev/input/event5')}); assert.equal(h.state.serial, 2);
  assert.equal(h.watches.size, 1); h.watch.stop();
});
test('non-event reset faults closed rather than treating idle reset as user input', () => {
  const h = nativeHarness(); h.watch.start(); h.fire(null);
  assert.deepEqual(h.state.snapshot(), [1, epoch, 2, true]);
  assert.equal(h.state.failed, true); assert.equal(h.watches.size, 0);
  h.state.setBlocked(false); h.fire({source: device('/dev/input/event4')});
  assert.deepEqual(h.state.snapshot(), [1, epoch, 2, true]);
});
test('re-arm failure retires the original one-shot and does not escape into Mutter', () => {
  const h = nativeHarness(); h.watch.start();
  h.monitor.add_user_active_watch = () => { throw Error('registration lost'); };
  assert.doesNotThrow(() => h.fire({source: device('/dev/input/event4')}));
  assert.equal(h.watches.size, 0); assert.equal(h.state.failed, true);
});
test('stop is idempotent and late or already-fired callbacks cannot re-arm', () => {
  const h = nativeHarness(); h.watch.start();
  const stale = [...h.watches.values()][0];
  h.fire({source: device('/dev/input/event4')});
  stale(); assert.equal(h.state.serial, 2); assert.equal(h.watches.size, 1);
  const queued = [...h.watches.values()][0];
  h.watch.stop(); h.watch.stop(); queued();
  assert.equal(h.state.serial, 2); assert.equal(h.watches.size, 0);
  assert.throws(() => h.watch.start(), /already started/);
});
test('zero, malformed and throwing registration never start a healthy observer', () => {
  for (const id of [0, -1, 1.5, true, null, undefined, 0x100000000]) {
    const removed = []; let faults = 0;
    const w = new NativeEventWatch({add_user_active_watch: () => id, remove_watch: i => removed.push(i)},
      () => null, () => {}, () => faults++);
    assert.throws(() => w.start(), /registration failed/); assert.equal(faults, 1);
    assert.deepEqual(removed, id === 0 ? [0] : []);
  }
  for (const monitor of [null, {}, {add_user_active_watch() {throw Error('gone');}, remove_watch() {}}]) {
    let faults = 0;
    const w = new NativeEventWatch(monitor, () => null, () => {}, () => faults++);
    assert.throws(() => w.start()); assert.equal(faults, 1);
  }
});
test('unsupported synchronous registration callback is bounded and removes its returned ID', () => {
  const removed = []; let calls = 0; let faults = 0;
  const w = new NativeEventWatch({add_user_active_watch(cb) { calls++; cb(); return 12; },
    remove_watch: id => removed.push(id)}, () => ({}), () => {}, () => faults++);
  assert.throws(() => w.start(), /registration failed/);
  assert.equal(calls, 1); assert.deepEqual(removed, [12]); assert.ok(faults > 0);
});
test('event getter, observer and fault publisher exceptions never escape the native callback', () => {
  for (const part of ['getter', 'observer']) {
    let next = 1; const watches = new Map(); let faults = 0;
    const w = new NativeEventWatch({add_user_active_watch(cb) {const id = next++; watches.set(id, cb); return id;},
      remove_watch: id => watches.delete(id)},
      () => {if (part === 'getter') throw Error('event lost'); return {};},
      () => {throw Error('observer fault');}, () => {faults++; throw Error('dead bus');});
    w.start(); const first = [...watches.keys()][0];
    assert.doesNotThrow(() => watches.get(first)()); watches.delete(first);
    assert.equal(faults, 1); assert.equal(watches.size, 0);
  }
});
