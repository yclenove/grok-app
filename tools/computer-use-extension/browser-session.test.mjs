import { test } from "node:test";
import assert from "node:assert/strict";
import { BrowserSession } from "./browser-session.mjs";

function fixture(data = {}) {
  const listeners = []; let writes = 0; let rejectWrite = false;
  const storage = {
    async setAccessLevel(value) { assert.equal(value.accessLevel, "TRUSTED_CONTEXTS"); },
    async get(key) { return structuredClone({ [key]: data[key] }); },
    async set(value) { writes++; if (rejectWrite) throw new Error("storage failed"); Object.assign(data, structuredClone(value)); },
  };
  const runtime = { onStartup: { addListener(fn) { listeners.push(fn); } } };
  return { data, storage, runtime, fire() { for (const listener of listeners) listener(); },
    writes: () => writes, fail: () => { rejectWrite = true; } };
}

test("missing storage and ordinary worker initialization never manufacture a browser startup", async () => {
  const f = fixture(); const first = new BrowserSession(f); const before = await first.snapshot();
  assert.equal(before.started, false); assert.equal(before.previousId, null);
  const next = new BrowserSession({ ...f, runtime: fixture().runtime });
  assert.deepEqual(await next.snapshot(), before);
  const reloaded = new BrowserSession(fixture()); const fresh = await reloaded.snapshot();
  assert.notEqual(fresh.id, before.id); assert.equal(fresh.started, false); assert.equal(fresh.previousId, null);
});

test("fresh session binds the first startup and the next startup rotates", async () => {
  const f = fixture(); const session = new BrowserSession(f);
  f.fire(); const started = await session.snapshot();
  assert.equal(started.started, true); assert.equal(started.previousId, null); assert.equal(f.writes(), 2);
  f.fire(); const nextStart = await session.snapshot(); assert.notEqual(nextStart.id, started.id);
  assert.equal(nextStart.previousId, started.id); assert.equal(f.writes(), 3);
  const reloaded = new BrowserSession({ ...f, runtime: fixture().runtime });
  assert.deepEqual(await reloaded.snapshot(), nextStart);
});

test("persisted started=false rotates on the first real startup", async () => {
  const priorId = "00000000-0000-4000-8000-000000000011";
  const f = fixture({ cuBrowserSession: { version: 1, id: priorId, started: false, previousId: null } });
  const session = new BrowserSession(f);
  const before = await session.snapshot();
  assert.equal(before.id, priorId); assert.equal(before.started, false); assert.equal(before.previousId, null);
  f.fire(); const started = await session.snapshot();
  assert.notEqual(started.id, priorId); assert.equal(started.previousId, priorId); assert.equal(started.started, true);
  const reloaded = new BrowserSession({ ...f, runtime: fixture().runtime });
  assert.deepEqual(await reloaded.snapshot(), started);
});

test("service worker reload does not rotate a started session", async () => {
  const priorId = "00000000-0000-4000-8000-000000000012";
  const f = fixture({ cuBrowserSession: { version: 1, id: priorId, started: true, previousId: null } });
  const session = new BrowserSession(f);
  assert.equal((await session.snapshot()).id, priorId);
  const reloaded = new BrowserSession({ ...f, runtime: fixture().runtime });
  assert.equal((await reloaded.snapshot()).id, priorId);
  assert.equal((await reloaded.snapshot()).started, true);
  assert.equal((await reloaded.snapshot()).previousId, null);
});

test("startup write failure does not publish a rotated persisted identity", async () => {
  const priorId = "00000000-0000-4000-8000-000000000013";
  const f = fixture({ cuBrowserSession: { version: 1, id: priorId, started: false, previousId: null } });
  const session = new BrowserSession(f);
  await session.snapshot();
  f.fail(); f.fire();
  await assert.rejects(session.snapshot(), /storage failed/);
  assert.equal(f.data.cuBrowserSession.id, priorId);
  assert.equal(f.data.cuBrowserSession.started, false);
  assert.equal(f.data.cuBrowserSession.previousId, null);
});

test("late native startup creates a new identity while preserving the previous owner", async () => {
  const priorId = "00000000-0000-4000-8000-000000000010";
  const f = fixture({ cuBrowserSession: { version: 1, id: priorId, started: true, previousId: null } });
  const session = new BrowserSession(f); const issued = await session.snapshot();
  f.fire(); const arrived = await session.snapshot();
  assert.notEqual(arrived.id, issued.id); assert.equal(arrived.previousId, issued.id); assert.equal(arrived.started, true); assert(Object.isFrozen(arrived));
  assert.equal(await session.acknowledgeRestart(arrived), issued.id); assert.equal((await session.snapshot()).previousId, null);
});

test("late pairing cleanup cannot acknowledge a newer browser startup", async () => {
  const f = fixture(); const session = new BrowserSession(f);
  f.fire(); await session.snapshot();
  f.fire(); const registered = await session.snapshot();
  f.fire(); const newer = await session.snapshot();
  assert.equal(await session.acknowledgeRestart(registered), null);
  assert.deepEqual(await session.snapshot(), newer);
  assert.deepEqual(f.data.cuBrowserSession, newer);
  assert.equal(await session.acknowledgeRestart(newer), registered.id);
});

test("concurrent restart acknowledgements consume the original boundary once", async () => {
  const f = fixture(); const session = new BrowserSession(f);
  f.fire(); await session.snapshot();
  f.fire(); const started = await session.snapshot();
  const results = await Promise.all([
    session.acknowledgeRestart(started), session.acknowledgeRestart(started),
  ]);
  assert.deepEqual(results, [started.previousId, null]);
  assert.equal((await session.snapshot()).previousId, null);
});

test("invalid storage and failed startup persistence cannot publish a successful startup", async () => {
  const broken = new BrowserSession(fixture({ cuBrowserSession: { version: 1, id: "invalid", started: true } }));
  await assert.rejects(broken.snapshot(), /browserSessionUnavailable/);
  const f = fixture(); const session = new BrowserSession(f); await session.snapshot();
  f.fail(); f.fire(); await assert.rejects(session.snapshot(), /storage failed/);
  assert.equal(f.data.cuBrowserSession.started, false);
});
