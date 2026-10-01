import { test } from "node:test";
import assert from "node:assert/strict";
import { SharedTabs } from "./shared-tabs.mjs";
import { ExecutionClock } from "./execution-clock.mjs";

const docA = "00000000000000000000000000000001";
const docB = "00000000000000000000000000000002";
function fixture() {
  const inspected = []; const offers = []; const removals = [];
  const client = { connectionEpoch: 1,
    async shareCurrentTab(tab, epoch) {
      assert.equal(epoch, client.connectionEpoch); offers.push(tab); return { shared: true };
    },
    async unshareTab(id, generation, epoch) {
      assert.equal(epoch, client.connectionEpoch); removals.push({ id, generation }); return { shared: false };
    },
  };
  const tabs = { async query(filter) {
    assert.deepEqual(filter, { active: true, lastFocusedWindow: true });
    return [{ id: 101, title: "fixture" }];
  } };
  const scripting = { async executeScript(request) {
    inspected.push(request);
    if (request.func.name === "fenceDocument") return [{ frameId: 0, documentId: docA, result: request.args[0] }];
    return [{ frameId: 0, documentId: docA, result: {
      url: "https://fixture.invalid/same", title: "fixture", visible: true,
    } }];
  } };
  const shared = new SharedTabs({ client, tabs, scripting, clock: new ExecutionClock(1) });
  return { client, tabs, scripting, shared, offers, removals, inspected };
}
function barrier() {
  let release; let arrive;
  return { reached: new Promise(resolve => { arrive = resolve; }),
    wait: () => { arrive(); return new Promise(resolve => { release = resolve; }); },
    release: () => release(),
  };
}

test("only explicit share inspects the main document and offers one candidate", async () => {
  const f = fixture();
  assert.deepEqual(await f.shared.current(), { tabId: 101, title: "fixture", shared: false });
  assert.equal(f.inspected.length, 0);
  assert.equal(f.offers.length, 0);
  assert.deepEqual(await f.shared.share(101), { tabId: 101, shared: true });
  assert.equal(f.offers.length, 1);
  assert.equal(f.inspected.length, 4);
  assert.deepEqual(f.inspected[0].target, { tabId: 101, frameIds: [0] });
  for (const request of f.inspected) assert.equal(request.world, "ISOLATED");
  assert.deepEqual(f.inspected[3].target, { tabId: 101, documentIds: [docA] });
  await f.shared.unshare(101);
  assert.deepEqual(f.removals, [{ id: 101, generation: f.offers[0].documentGeneration }]);
  assert.equal((await f.shared.current()).shared, false);
});

test("unpaired, switched active tab and denied activeTab permission never offer", async () => {
  const f = fixture();
  f.client.connectionEpoch = null;
  await assert.rejects(f.shared.share(101), /pairingRejected/);
  f.client.connectionEpoch = 1;
  await assert.rejects(f.shared.share(202), /cancelled/);
  assert.equal(f.inspected.length, 0);
  f.scripting.executeScript = async () => { throw new Error("no activeTab permission"); };
  await assert.rejects(f.shared.share(101), /activeTab/);
  assert.equal(f.offers.length, 0);
});

test("an uncommitted or failed worker epoch cannot inspect a document or publish a share", async () => {
  for (const succeeds of [true, false]) {
    const f = fixture(); let release; let reject;
    const pending = new Promise((resolve, fail) => { release = resolve; reject = fail; });
    const shared = new SharedTabs({ ...f, clock: new ExecutionClock(pending) });
    const task = shared.share(101); const checked = succeeds ? task : assert.rejects(task, /epoch failed/);
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(f.inspected.length, 0); assert.equal(f.offers.length, 0);
    if (succeeds) release(7); else reject(new Error("epoch failed"));
    await checked;
    assert.equal(f.offers.length, succeeds ? 1 : 0);
    if (succeeds) assert.equal(f.inspected.find(call => call.func.name === "fenceDocument").args[0].epoch, 7);
  }
});

test("a document fence refusal prevents sharing before any Host offer", async () => {
  const f = fixture(); const execute = f.scripting.executeScript;
  f.scripting.executeScript = async call => {
    if (call.func.name === "fenceDocument") throw new Error("stale execution");
    return execute(call);
  };
  await assert.rejects(f.shared.share(101), /stale execution/);
  assert.equal(f.offers.length, 0); assert.equal((await f.shared.current()).shared, false);
});

test("navigation during metadata collection cancels before Host offer", async () => {
  const f = fixture(); const b = barrier(); const inspect = f.scripting.executeScript;
  f.scripting.executeScript = async request => { await b.wait(); return inspect(request); };
  const task = f.shared.share(101); await b.reached;
  await f.shared.invalidate(101); b.release();
  await assert.rejects(task, /cancelled/);
  assert.equal(f.offers.length, 0);
  assert.equal(f.removals.length, 0);
});

test("navigation during offer immediately revokes and late success cannot restore sharing", async () => {
  const f = fixture(); const b = barrier(); const offer = f.client.shareCurrentTab;
  f.client.shareCurrentTab = async (...args) => { await offer(...args); await b.wait(); return { shared: true }; };
  const task = f.shared.share(101); await b.reached;
  await f.shared.invalidate(101);
  assert.equal(f.removals.length, 1);
  b.release(); await assert.rejects(task, /cancelled/);
  assert.equal(f.removals.length, 1, "late callback does not send another unshare");
  assert.equal((await f.shared.current()).shared, false);
});

test("same-URL document replacement after offer revokes the candidate", async () => {
  const f = fixture(); const inspect = f.scripting.executeScript; let count = 0;
  f.scripting.executeScript = async request => {
    const result = await inspect(request);
    if (++count === 4) result[0].documentId = docB;
    return result;
  };
  await assert.rejects(f.shared.share(101), /shareUnavailable/);
  assert.equal(f.offers.length, 1);
  assert.equal(f.removals.length, 1);
});

test("old pairing callback cannot retire a new connection or new tab share", async () => {
  const f = fixture(); const b = barrier(); const offer = f.client.shareCurrentTab;
  f.client.shareCurrentTab = async (...args) => { await offer(...args); await b.wait(); return { shared: true }; };
  const task = f.shared.share(101); await b.reached;
  f.client.connectionEpoch = 2; f.shared.reset(); f.client.shareCurrentTab = offer;
  await f.shared.share(101);
  b.release(); await assert.rejects(task, /cancelled/);
  assert.equal(f.removals.length, 0);
  assert.equal((await f.shared.current()).shared, true);
});

test("re-share after a same-URL navigation gets a new generation", async () => {
  const f = fixture();
  await f.shared.share(101);
  await assert.rejects(f.shared.share(101), /busy/);
  await f.shared.invalidate(101);
  await f.shared.share(101);
  assert(f.offers[1].documentGeneration > f.offers[0].documentGeneration);
});

test("non-main-frame, ambiguous, hidden and unsafe URL snapshots are rejected", async () => {
  for (const kind of ["frame", "many", "hidden", "url", "missing-id"]) {
    const f = fixture(); const inspect = f.scripting.executeScript;
    f.scripting.executeScript = async request => {
      const result = await inspect(request);
      if (kind === "frame") result[0].frameId = 1;
      if (kind === "many") result.push(result[0]);
      if (kind === "hidden") result[0].result.visible = false;
      if (kind === "url") result[0].result.url = "chrome://settings";
      if (kind === "missing-id") delete result[0].documentId;
      return result;
    };
    await assert.rejects(f.shared.share(101), /shareUnavailable/);
    assert.equal(f.offers.length, 0);
  }
});

test("a pending unshare fences the tab and blocks replacement share until retirement finishes", async () => {
  const f = fixture(); const b = barrier(); const remove = f.client.unshareTab;
  await f.shared.share(101);
  f.client.unshareTab = async (...args) => { await remove(...args); await b.wait(); };
  const retiring = f.shared.invalidate(101); await b.reached;
  assert.equal((await f.shared.current()).shared, false);
  await assert.rejects(f.shared.share(101), /busy/);
  b.release(); await retiring;
  await f.shared.share(101);
  assert.equal(f.offers.length, 2);
});

test("observe requires an explicitly shared current document and uses fixed document-bound code", async () => {
  const f = fixture();
  const request = { tabId: "101", documentGeneration: 1, deadlineMs: Date.now() + 10000,
    command: { kind: "observe", snapshotId: "00000000-0000-4000-8000-000000000001" } };
  await assert.rejects(f.shared.observe(request), /cancelled/);
  assert.equal(f.inspected.length, 0);
  await f.shared.share(101); request.documentGeneration = f.offers[0].documentGeneration;
  const inspect = f.scripting.executeScript;
  f.scripting.executeScript = async call => {
    const result = await inspect(call);
    if (call.func.name === "observeDocument") result[0].result = { snapshotId: call.args[0], text: "visible fixture", url: "https://fixture.invalid/same" };
    return result;
  };
  const result = await f.shared.observe(request);
  assert.equal(result.documentId, docA); assert.equal(result.text, "visible fixture");
  const call = f.inspected.find(call => call.func.name === "observeDocument");
  assert.deepEqual(call.target, { tabId: 101, documentIds: [docA] });
  assert.equal(call.world, "ISOLATED"); assert.equal(call.func.name, "observeDocument");
  await f.shared.unshare(101);
  const before = f.inspected.length;
  await assert.rejects(f.shared.observe(request), /cancelled/);
  assert.equal(f.inspected.length, before);
});

test("navigation while an observation is pending cannot return old content", async () => {
  const f = fixture(); const b = barrier();
  await f.shared.share(101);
  const inspect = f.scripting.executeScript;
  f.scripting.executeScript = async call => {
    const result = await inspect(call);
    if (call.func.name === "observeDocument") { await b.wait(); result[0].result = { snapshotId: call.args[0], text: "old fixture" }; }
    return result;
  };
  const task = f.shared.observe({ tabId: "101", documentGeneration: f.offers[0].documentGeneration,
    deadlineMs: Date.now() + 10000, command: { kind: "observe", snapshotId: "snapshot" } });
  await b.reached; await f.shared.invalidate(101); b.release();
  await assert.rejects(task, /cancelled/);
});

test("switch-away-and-back, scroll-away-and-back and unshare fence a pending observation", async () => {
  for (const mode of ["activity", "view", "unshare", "reset"]) {
    const f = fixture(); const b = barrier(); let generation = 0;
    await f.shared.share(101);
    const inspect = f.scripting.executeScript;
    f.scripting.executeScript = async call => {
      const result = await inspect(call);
      result[0].result.viewGeneration = generation;
      if (call.func.name === "observeDocument") {
        await b.wait();
        result[0].result = { snapshotId: call.args[0], text: "old fixture", url: "https://fixture.invalid/same" };
      }
      return result;
    };
    const task = f.shared.observe({ tabId: "101", documentGeneration: f.offers[0].documentGeneration,
      deadlineMs: Date.now() + 10000, command: { kind: "observe", snapshotId: "snapshot", screenshot: false } });
    await b.reached;
    if (mode === "activity") { f.shared.activityChanged(); f.shared.activityChanged(); }
    if (mode === "view") generation += 2;
    if (mode === "unshare") await f.shared.unshare(101);
    if (mode === "reset") f.shared.reset();
    b.release(); await assert.rejects(task, /cancelled/, mode);
  }
});

test("screenshot permission denial returns no text-only success and never activates a tab", async () => {
  const f = fixture(); await f.shared.share(101);
  const inspect = f.scripting.executeScript;
  f.scripting.executeScript = async call => {
    const result = await inspect(call);
    if (call.func.name === "observeDocument") result[0].result = { snapshotId: call.args[0], url: "https://fixture.invalid/same" };
    return result;
  };
  f.tabs.query = async () => [{ id: 101, windowId: 7 }];
  let captures = 0;
  f.tabs.captureVisibleTab = async (windowId, options) => {
    assert.equal(windowId, 7); assert.deepEqual(options, { format: "png" }); captures++;
    throw new Error("activeTab grant missing");
  };
  await assert.rejects(f.shared.observe({ tabId: "101", documentGeneration: f.offers[0].documentGeneration,
    deadlineMs: Date.now() + 10000, command: { kind: "observe", snapshotId: "snapshot", screenshot: true } }), /activeTab/);
  assert.equal(captures, 1);
  assert.equal((await f.shared.current()).shared, true, "a denied capture cannot change the user's tab or pairing");
});

test("a capture interrupted by focus, geometry, unshare or replacement is discarded before decoding", async () => {
  for (const mode of ["activity", "view", "unshare", "reset"]) {
    const f = fixture(); const b = barrier(); let generation = 0;
    await f.shared.share(101);
    const inspect = f.scripting.executeScript;
    f.scripting.executeScript = async call => {
      const result = await inspect(call);
      result[0].result.viewGeneration = generation;
      if (call.func.name === "observeDocument") result[0].result = { snapshotId: call.args[0], url: "https://fixture.invalid/same" };
      return result;
    };
    f.tabs.query = async () => [{ id: 101, windowId: 7 }];
    f.tabs.captureVisibleTab = async () => { await b.wait(); return "NOT_AN_IMAGE_FROM_A_SUSPECT_CAPTURE"; };
    const task = f.shared.observe({ tabId: "101", documentGeneration: f.offers[0].documentGeneration,
      deadlineMs: Date.now() + 10000, command: { kind: "observe", snapshotId: "snapshot", screenshot: true } });
    await b.reached;
    if (mode === "activity") { f.shared.activityChanged(); f.shared.activityChanged(); }
    if (mode === "view") generation++;
    if (mode === "unshare") await f.shared.unshare(101);
    if (mode === "reset") f.shared.reset();
    b.release();
    await assert.rejects(task, /cancelled/, `${mode}: no image parser or transport must receive this capture`);
  }
});
