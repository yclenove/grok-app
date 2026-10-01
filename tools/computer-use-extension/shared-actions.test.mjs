import { test } from "node:test";
import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { SharedTabs } from "./shared-tabs.mjs";
import { ExecutionClock } from "./execution-clock.mjs";

const documentId = "00000000000000000000000000000001";
function barrier() {
  let enter; let release;
  const reached = new Promise(resolve => { enter = resolve; });
  const done = new Promise(resolve => { release = resolve; });
  return { reached, release, wait: () => { enter(); return done; } };
}
async function fixture() {
  const calls = [];
  const client = { connectionEpoch: 1, shareCurrentTab: async () => {}, unshareTab: async () => {} };
  const tabs = { query: async () => [{ id: 101, title: "Fixture" }] };
  const scripting = { async executeScript(req) {
    calls.push(req.func.name);
    let result;
    if (req.func.name === "fenceDocument") result = req.args[0];
    if (req.func.name === "inspectDocument") result = { url: "https://fixture.invalid/", title: "Fixture", visible: true };
    if (req.func.name === "observeDocument") result = { snapshotId: req.args[0], url: "https://fixture.invalid/", nodes: [] };
    if (req.func.name === "actDocument") result = { status: "applied", detail: "semantic_click_dispatched" };
    if (req.func.name === "cancelDocumentOperation") result = { status: "settled" };
    assert(result, "unexpected injected function");
    return [{ frameId: 0, documentId, result }];
  } };
  const shared = new SharedTabs({ client, tabs, scripting, clock: new ExecutionClock(1) });
  await shared.share(101);
  const observed = { requestId: randomUUID(), tabId: "101", session: "owner", runId: "run", grantGeneration: 1,
    documentGeneration: 1, deadlineMs: Date.now() + 10000,
    command: { kind: "observe", snapshotId: randomUUID(), screenshot: false, preview: false } };
  await shared.observe(observed);
  const action = { ...observed, documentId, requestId: randomUUID(), command: { kind: "act", snapshotId: observed.command.snapshotId,
    elementRef: `${observed.command.snapshotId}-1`, action: "click", parameters: {} } };
  return { shared, client, tabs, scripting, calls, observed, action };
}

test("activity changes, stale owner/grant and stale snapshots are rejected before script dispatch", async () => {
  for (const change of [f => f.shared.activityChanged(), f => { f.action.session = "other"; },
    f => { f.action.runId = "other"; }, f => { f.action.grantGeneration++; }, f => { f.action.command.snapshotId = randomUUID(); },
    f => { f.action.documentId = "1".repeat(32); }]) {
    const f = await fixture(); change(f);
    assert.equal((await f.shared.act(f.action)).status, "rejected");
    assert(!f.calls.includes("actDocument")); assert(f.shared.actionsIdle());
  }
});

test("preview preserves the captured action scope; a successful mutation consumes it", async () => {
  const f = await fixture();
  await f.shared.observe({ ...f.observed, command: { ...f.observed.command, snapshotId: randomUUID(), preview: true } });
  assert.equal((await f.shared.act(f.action)).status, "applied");
  assert.equal((await f.shared.act({ ...f.action, requestId: randomUUID() })).status, "rejected");
  assert.equal(f.calls.filter(name => name === "actDocument").length, 1);
  assert(f.shared.actionsIdle());
});

test("cancel/reset/unshare retain physical admission until both scripting and cancellation finish", async () => {
  for (const mode of ["signal", "reset", "unshare", "activity", "deadline"]) {
    const f = await fixture(); const action = barrier(); const cancellation = barrier();
    const original = f.scripting.executeScript;
    f.scripting.executeScript = async req => {
      if (req.func.name === "actDocument") await action.wait();
      if (req.func.name === "cancelDocumentOperation") await cancellation.wait();
      return original(req);
    };
    const controller = new AbortController();
    if (mode === "deadline") f.action.deadlineMs = Date.now() + 60;
    let settled = false;
    const running = f.shared.act(f.action, controller.signal).then(value => { settled = true; return value; });
    try {
      await action.reached;
      if (mode === "signal") controller.abort();
      if (mode === "reset") f.shared.reset();
      if (mode === "unshare") await f.shared.invalidate(101);
      if (mode === "activity") f.shared.activityChanged();
      await cancellation.reached;
      assert.equal(f.shared.actionsIdle(), false); assert.equal(settled, false);
      await assert.rejects(f.shared.share(101), /busy/);
      cancellation.release(); await Promise.resolve();
      assert.equal(f.shared.actionsIdle(), false); assert.equal(settled, false, "cancel reply cannot release the original native call");
      action.release(); const result = await running;
      assert.equal(result.status, "unknown"); assert.equal(result.physicallySettled, true); assert(f.shared.actionsIdle());
    } finally { action.release(); cancellation.release(); await running; }
  }
});

test("an ambiguous native failure with no cancellation proof keeps the tab busy", async () => {
  const f = await fixture(); const original = f.scripting.executeScript;
  f.scripting.executeScript = async req => {
    if (req.func.name === "actDocument") throw new Error("native result lost");
    if (req.func.name === "cancelDocumentOperation") return [{ frameId: 0, documentId, result: { status: "snapshot_missing" } }];
    return original(req);
  };
  const result = await f.shared.act(f.action);
  assert.equal(result.status, "unknown"); assert.equal(result.physicallySettled, false);
  assert.equal(f.shared.actionsIdle(), false);
  await assert.rejects(f.shared.observe(f.observed), /busy/);
  f.shared.reset();
  await assert.rejects(f.shared.share(101), /busy/);
  const binding = { requestId: f.action.requestId, tabId: f.action.tabId, documentId,
    snapshotId: f.action.command.snapshotId };
  assert.throws(() => f.shared.releaseRecovered({ ...binding, requestId: "different" }), /recoveryOwnershipChanged/);
  assert(!f.shared.actionsIdle()); f.shared.releaseRecovered(binding); assert(f.shared.actionsIdle());
});

test("a cancellation proof may settle unknown completion but cannot claim the action succeeded", async () => {
  const f = await fixture(); const original = f.scripting.executeScript;
  f.scripting.executeScript = req => req.func.name === "actDocument" ? Promise.reject(new Error("reply lost")) : original(req);
  const result = await f.shared.act(f.action);
  assert.equal(result.status, "unknown"); assert.equal(result.physicallySettled, true); assert(f.shared.actionsIdle());
});

test("cancelling while preflight is pending never starts an action or a cancellation script", async () => {
  const f = await fixture(); const pending = barrier(); const original = f.tabs.query;
  f.tabs.query = async () => { await pending.wait(); return original(); };
  const controller = new AbortController(); const running = f.shared.act(f.action, controller.signal);
  await pending.reached; controller.abort(); pending.release();
  assert.equal((await running).status, "rejected"); assert(f.shared.actionsIdle());
  assert(!f.calls.includes("actDocument")); assert(!f.calls.includes("cancelDocumentOperation"));
});
