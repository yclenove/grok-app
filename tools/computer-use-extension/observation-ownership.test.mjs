import { test } from "node:test";
import assert from "node:assert/strict";
import { SharedTabs } from "./shared-tabs.mjs";
import { ExecutionClock } from "./execution-clock.mjs";
import { randomUUID } from "node:crypto";
import { ObservationCompletions } from "./observation-completions.mjs";
import { ObservationJournal } from "./observation-journal.mjs";
import { CompletionScope } from "./completion-scope.mjs";

const documentId = "00000000000000000000000000000001";
const png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jEFAAAAAASUVORK5CYII=";
const turn = () => new Promise(resolve => setImmediate(resolve));
function barrier() {
  let arrive; let release;
  return { entered: new Promise(resolve => { arrive = resolve; }),
    hold: () => { arrive(); return new Promise(resolve => { release = resolve; }); },
    release: value => release(value) };
}
async function fixture() {
  const calls = [];
  const client = { connectionEpoch: 1, async shareCurrentTab() {}, async unshareTab() {} };
  const tabs = { async query() { calls.push("query"); return [{ id: 101, windowId: 7 }]; },
    async captureVisibleTab() { calls.push("capture"); return png; } };
  const scripting = { async executeScript(call) {
    calls.push(call.func.name);
    const result = call.func.name === "fenceDocument" ? call.args[0] : {
      visible: true, url: "https://fixture.invalid/", title: "fixture", text: "fixture",
      snapshotId: call.args?.[0], viewportWidth: 1, viewportHeight: 1, pixelRatio: 1,
      visualScale: 1, visualX: 0, visualY: 0,
    };
    return [{ frameId: 0, documentId, result }];
  } };
  const shared = new SharedTabs({ client, tabs, scripting, clock: new ExecutionClock(1) });
  await shared.share(101);
  const request = { requestId: "00000000-0000-4000-8000-000000000001", session: "owner", runId: "run",
    tabId: "101", documentId, documentGeneration: 1, grantGeneration: 1, deadlineMs: Date.now() + 10000,
    command: { kind: "observe", snapshotId: "00000000-0000-4000-8000-000000000002", screenshot: true, preview: false } };
  calls.length = 0;
  return { shared, client, tabs, scripting, request, calls };
}

test("Host cancellation between watcher polls fences the original SharedTabs capture before decoding, including stale pending status", async t => {
  const bitmap = decoder(t);
  for (const heldStatus of [false, true]) {
    const f = await fixture(); const capture = barrier(); const status = barrier();
    let cancelled = false; let statuses = 0; let settlements = 0; let finished = false;
    const data = {};
    const journal = new ObservationJournal({ storage: { async setAccessLevel() {}, async get(key) { return { [key]: data[key] }; },
      async set(value) { Object.assign(data, structuredClone(value)); }, async remove(key) { delete data[key]; } } });
    const connection = { instanceId: randomUUID(), connectionNonce: randomUUID(), generation: 1 };
    const binding = { protocol: 2, requestId: f.request.requestId, connection, session: f.request.session,
      runId: f.request.runId, tabId: f.request.tabId, documentId, documentGeneration: 1, grantGeneration: 1,
      snapshotId: f.request.command.snapshotId };
    const request = { ...f.request, protocol: 2, sequence: 1, connection,
      completion: { purpose: "observation", proof: { binding, completionKey: "a".repeat(64) } } };
    f.client.observationScope = (_epoch, completion, ownedJournal) => new CompletionScope({ endpoint: "http://127.0.0.1:32100",
      proof: completion.proof, purpose: "observation", journal: ownedJournal, fetch: async url => {
        if (url.endsWith("/status")) {
          statuses++; const cancelRequested = cancelled;
          if (heldStatus && statuses === 1) await status.hold();
          return Response.json({ ok: true, status: { phase: "claimed", cancelRequested } });
        }
        assert(url.endsWith("/settle")); settlements++; return Response.json({ ok: true });
      } });
    f.client.claimObservation = async () => {};
    f.tabs.captureVisibleTab = async () => { await capture.hold(); return png; };
    const owner = new ObservationCompletions({ client: f.client, journal,
      observe: (...args) => f.shared.observe(...args) });
    const running = owner.run(1, request).then(value => { finished = true; return value; });
    try {
      await capture.entered;
      if (heldStatus) await status.entered;
      assert.equal(statuses, 1); cancelled = true; capture.release(); await turn();
      if (heldStatus) {
        assert(!finished); assert.equal(bitmap.decoded, 0); assert.equal(settlements, 0);
        status.release();
      }
      assert.deepEqual(await running, { kind: "rejected", reason: "cancelled" });
      assert.equal(bitmap.decoded, 0); assert(statuses >= 2, "the stale watcher reply cannot replace a fresh post-capture check");
      assert.equal(settlements, 1); assert(owner.idle()); assert(!await journal.hasTab("101"));
    } finally { capture.release(); if (heldStatus) status.release(); await running; }
  }
});
function decoder(t, decode = async () => {}) {
  const counts = { decoded: 0, closed: 0 };
  const original = new Map(["createImageBitmap", "OffscreenCanvas"].map(key => [key, Object.getOwnPropertyDescriptor(globalThis, key)]));
  t.after(() => {
    for (const [key, descriptor] of original) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else delete globalThis[key];
    }
  });
  globalThis.createImageBitmap = async () => {
    counts.decoded++;
    await decode();
    return { width: 1, height: 1, close() { counts.closed++; } };
  };
  globalThis.OffscreenCanvas = function () {
    return { getContext: () => ({ drawImage() {} }),
      convertToBlob: async () => new Blob([Buffer.from(png.slice(22), "base64")], { type: "image/png" }) };
  };
  return counts;
}

// Unit decoder hooks are deliberately synthetic. The original source82 PNG/native
// counterexample remains independent and cannot be closed by these tests.
test("a cancelled original capture is joined and discarded before decoding", async t => {
  const f = await fixture(); const gate = barrier(); const controller = new AbortController();
  const counts = decoder(t);
  f.tabs.captureVisibleTab = async () => { f.calls.push("capture"); return gate.hold(); };
  const task = f.shared.observe(f.request, controller.signal);
  const checked = assert.rejects(task, /cancelled/);
  let returned = false; void task.then(() => { returned = true; }, () => { returned = true; });
  await gate.entered; controller.abort(); await turn();
  assert.equal(returned, false, "abort does not abandon the original native capture");
  gate.release(png); await checked;
  assert.equal(counts.decoded, 0, "the same late image must never reach createImageBitmap");
  assert.equal(f.calls.filter(call => call === "capture").length, 1);
  assert.equal(f.shared.observationsIdle(), true);
});

test("pre-cancelled observation performs no clock, browser query, script or capture", async () => {
  const f = await fixture(); const controller = new AbortController(); controller.abort();
  await assert.rejects(f.shared.observe(f.request, controller.signal), /cancelled/);
  assert.deepEqual(f.calls, []);
  assert(f.shared.observationsIdle());
});

test("a phase timeout retires results but retains the original capture and tab slot", async t => {
  const f = await fixture(); const gate = barrier();
  t.mock.timers.enable({ apis: ["setTimeout", "Date"], now: Date.now() });
  f.request.deadlineMs = Date.now() + 10000;
  f.tabs.captureVisibleTab = () => gate.hold();
  const task = f.shared.observe(f.request); const checked = assert.rejects(task, /cancelled/);
  let returned = false; void task.then(() => { returned = true; }, () => { returned = true; });
  await gate.entered; t.mock.timers.tick(5001); await turn();
  assert.equal(returned, false, "five seconds is not proof that captureVisibleTab has finished");
  assert.equal(f.shared.observationsIdle(), false);
  await assert.rejects(f.shared.observe(f.request), /busy/);
  gate.release(png); await checked;
  assert(f.shared.observationsIdle());
});

test("reset and re-pair cannot replace an observation owner before its original promise ends", async () => {
  const f = await fixture(); const gate = barrier();
  f.tabs.captureVisibleTab = () => gate.hold();
  const task = f.shared.observe(f.request); const checked = assert.rejects(task, /cancelled/);
  await gate.entered; f.shared.reset(); f.client.connectionEpoch = 2;
  assert.equal(f.shared.observationsIdle(), false);
  const before = f.calls.length;
  await assert.rejects(f.shared.share(101), /busy/);
  assert.equal(f.calls.length, before, "replacement share cannot inspect the old owner");
  gate.release(png); await checked;
  assert(f.shared.observationsIdle());
  await f.shared.share(101);
});

test("an in-progress decoder is joined and its bitmap closed before occupancy is returned", async t => {
  const f = await fixture(); const gate = barrier(); const controller = new AbortController();
  const counts = decoder(t, () => gate.hold());
  const task = f.shared.observe(f.request, controller.signal); const checked = assert.rejects(task, /cancelled/);
  let returned = false; void task.then(() => { returned = true; }, () => { returned = true; });
  await gate.entered; controller.abort(); await turn();
  assert.equal(counts.decoded, 1); assert.equal(counts.closed, 0); assert.equal(returned, false);
  assert.equal(f.shared.observationsIdle(), false);
  await assert.rejects(f.shared.observe(f.request), /busy/);
  gate.release(); await checked;
  assert.equal(counts.closed, 1); assert(f.shared.observationsIdle());
});

test("cancelled body and active-tab reads retain their original promises and cannot dispatch a capture", async () => {
  for (const phase of ["query", "inspectDocument", "observeDocument"]) {
    const f = await fixture(); const gate = barrier(); const controller = new AbortController();
    if (phase === "query") {
      const query = f.tabs.query;
      f.tabs.query = async (...args) => { await gate.hold(); return query(...args); };
    } else {
      const execute = f.scripting.executeScript;
      f.scripting.executeScript = async call => {
        if (call.func.name === phase) await gate.hold();
        return execute(call);
      };
    }
    const task = f.shared.observe(f.request, controller.signal); const checked = assert.rejects(task, /cancelled/);
    let returned = false; void task.then(() => { returned = true; }, () => { returned = true; });
    await gate.entered; controller.abort(); await turn();
    assert.equal(returned, false, phase); assert.equal(f.shared.observationsIdle(), false, phase);
    gate.release(); await checked;
    assert(!f.calls.includes("capture"), phase); assert(f.shared.observationsIdle(), phase);
  }
});

test("an observation keeps one tab slot and prevents input from using its previous snapshot", async () => {
  const f = await fixture();
  await f.shared.observe({ ...f.request, command: { ...f.request.command, screenshot: false } });
  const gate = barrier(); f.tabs.captureVisibleTab = () => gate.hold();
  const task = f.shared.observe({ ...f.request, command: { ...f.request.command, preview: true } });
  const checked = assert.rejects(task, /cancelled/);
  await gate.entered;
  const result = await f.shared.act({ ...f.request, documentId,
    command: { kind: "act", snapshotId: f.request.command.snapshotId, elementRef: "fixture-ref",
      action: "click", parameters: {} } });
  assert.equal(result.status, "rejected"); assert.equal(result.physicallySettled, true);
  assert(!f.calls.includes("actDocument"));
  f.shared.activityChanged(); gate.release(png); await checked;
});

test("the request deadline cannot release a still-running capture even before its phase timeout", async t => {
  const f = await fixture(); const gate = barrier();
  t.mock.timers.enable({ apis: ["setTimeout", "Date"], now: Date.now() });
  f.request.deadlineMs = Date.now() + 50;
  f.tabs.captureVisibleTab = () => gate.hold();
  const task = f.shared.observe(f.request); const checked = assert.rejects(task, /cancelled/);
  let returned = false; void task.then(() => { returned = true; }, () => { returned = true; });
  await gate.entered; t.mock.timers.tick(51); await turn();
  assert.equal(returned, false); assert.equal(f.shared.observationsIdle(), false);
  gate.release(png); await checked;
  assert(f.shared.observationsIdle());
});

test("natural screenshot and preview finish without extra captures or losing a shared candidate", async t => {
  const counts = decoder(t);
  for (const preview of [false, true]) {
    const f = await fixture();
    const result = await f.shared.observe({ ...f.request, command: { ...f.request.command, preview } });
    assert.equal(result.documentId, documentId); assert.equal(result.text, "fixture");
    assert.equal(result.screenshot.width, 1); assert.equal(result.screenshot.height, 1);
    assert.equal(f.calls.filter(call => call === "capture").length, 1);
    assert(f.shared.observationsIdle()); assert.equal((await f.shared.current()).shared, true);
  }
  assert.equal(counts.decoded, 2); assert.equal(counts.closed, 2);
});
