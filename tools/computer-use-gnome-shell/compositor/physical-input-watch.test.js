import {test} from 'node:test';
import assert from 'node:assert/strict';
import {PhysicalInputWatch} from './physical-input-watch.js';

function harness() {
  const h = {version: 1, generation: 12, advance: [], faults: 0, removed: [], callbacks: new Map()};
  h.provider = {
    read: () => ({version: h.version, generation: h.generation}),
    subscribe(cb) { h.callbacks.set(23, cb); return 23; },
    disconnect(id) { h.removed.push(id); h.callbacks.delete(id); },
  };
  h.watch = new PhysicalInputWatch(h.provider, delta => h.advance.push(delta), () => h.faults++);
  h.notify = () => { for (const callback of h.callbacks.values()) callback(); };
  return h;
}

test('subscribes before baseline without fabricating prior physical activity', () => {
  const h = harness(); const read = h.provider.read;
  h.provider.read = () => { assert.equal(h.callbacks.size, 1); return read(); };
  h.watch.start(); h.watch.poll(); h.notify();
  assert.deepEqual(h.advance, []); assert.equal(h.faults, 0);
  h.watch.stop(); assert.deepEqual(h.removed, [23]);
});

test('readback detects input before delayed coalesced notify and never double counts', () => {
  const h = harness(); h.watch.start(); h.generation += 6;
  h.watch.poll(); assert.deepEqual(h.advance, [6]);
  h.notify(); assert.deepEqual(h.advance, [6]);
  h.generation++; h.notify(); assert.deepEqual(h.advance, [6, 1]);
  h.watch.stop();
});

test('stable counter does not synthesize takeover from a notification', () => {
  const h = harness(); h.watch.start();
  for (let i = 0; i < 100; i++) { h.notify(); h.watch.poll(); }
  assert.deepEqual(h.advance, []); assert.equal(h.faults, 0); h.watch.stop();
});

test('counter reset is sticky loss, never a new healthy baseline', () => {
  const h = harness(); h.watch.start(); const late = h.callbacks.get(23);
  h.generation--; h.notify(); assert.equal(h.faults, 1);
  h.generation += 20; late(); h.watch.poll();
  assert.deepEqual(h.advance, []); assert.equal(h.callbacks.size, 0);
  assert.throws(() => h.watch.start(), /already started/);
});

test('unknown ABI, missing counter, uint64 sentinel and JS precision loss fail closed', () => {
  for (const [version, generation] of [[2, 0], [undefined, 0], ['1', 0],
    [1, undefined], [1, null], [1, -1], [1, NaN], [1, Infinity], [1, 0.5],
    [1, '12'], [1, 2 ** 53], [1, Number(0xffffffffffffffffn)]]) {
    const h = harness(); h.version = version; h.generation = generation;
    assert.throws(() => h.watch.start(), /snapshot/);
    assert.equal(h.faults, 1); assert.deepEqual(h.removed, [23]);
  }
});

test('large valid deltas remain bounded single notifications', () => {
  const h = harness(); h.watch.start(); h.generation = Number.MAX_SAFE_INTEGER;
  h.notify(); assert.deepEqual(h.advance, [Number.MAX_SAFE_INTEGER - 12]);
  h.generation++; h.notify(); assert.equal(h.faults, 1);
});

test('stop is idempotent and queued callback cannot resurrect a subscription', () => {
  const h = harness(); h.watch.start(); const late = h.callbacks.get(23);
  h.watch.stop(); h.watch.stop(); h.generation++; late(); h.watch.poll();
  assert.deepEqual(h.advance, []); assert.equal(h.faults, 0);
  assert.deepEqual(h.removed, [23]);
});

test('read and publisher faults cannot escape into notification dispatch', () => {
  for (const failing of ['read', 'publish']) {
    const h = harness(); h.watch.start();
    if (failing === 'read') h.provider.read = () => { throw Error('seat gone'); };
    else h.watch._advance = () => { throw Error('bus gone'); };
    h.generation++;
    assert.doesNotThrow(h.notify); assert.equal(h.faults, 1);
    assert.equal(h.callbacks.size, 0); h.watch.poll(); assert.equal(h.faults, 1);
  }
});

test('synchronous subscribe callback fails and retires the returned original ID', () => {
  const h = harness(); h.provider.subscribe = cb => { cb(); return 23; };
  assert.throws(() => h.watch.start(), /subscription failed/);
  assert.deepEqual(h.removed, [23]); assert.equal(h.faults, 1);
});

test('invalid subscriptions and absent provider never establish readiness', () => {
  for (const id of [undefined, null, 0, -1, false, '23', 0.5, 2 ** 53]) {
    const h = harness(); h.provider.subscribe = () => id;
    assert.throws(() => h.watch.start(), /subscription failed/);
    assert.equal(h.faults, 1); assert.deepEqual(h.removed, []);
  }
  for (const provider of [null, {}, {read() {}}]) {
    let faults = 0;
    const watch = new PhysicalInputWatch(provider, () => {}, () => faults++);
    assert.throws(() => watch.start(), /unavailable/); assert.equal(faults, 1);
  }
});

test('reentrant read after advancement neither duplicates nor loses later input', () => {
  const h = harness(); h.watch.start();
  h.watch._advance = delta => {
    h.advance.push(delta);
    if (h.advance.length === 1) { h.generation++; h.watch.poll(); }
  };
  h.generation++; h.notify(); assert.deepEqual(h.advance, [1, 1]); h.watch.stop();
});

test('disconnect and fault callback errors cannot escape an observer fault', () => {
  const h = harness(); h.watch.start();
  h.provider.disconnect = () => { throw Error('gone'); };
  h.watch._fault = () => { h.faults++; throw Error('dead bus'); };
  h.generation = -1; assert.doesNotThrow(h.notify); assert.equal(h.faults, 1);
  h.watch.poll(); assert.equal(h.faults, 1);
});
