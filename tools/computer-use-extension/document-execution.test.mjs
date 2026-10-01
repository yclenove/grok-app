import { test } from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { randomUUID } from "node:crypto";
import { fenceDocument } from "./document-execution.mjs";
import { observeDocument } from "./observe-document.mjs";
import { actDocument, cancelDocumentOperation } from "./act-document.mjs";
import { ExecutionClock } from "./execution-clock.mjs";
import { withObservationClock } from "../computer-use-probe/dom-fixture-clock.mjs";
const { JSDOM, VirtualConsole } = createRequire(new URL("../../package.json", import.meta.url))("jsdom");
const stamp = (epoch, sequence) => ({ epoch, sequence });

function fixture() {
  const virtualConsole = new VirtualConsole(); virtualConsole.on("jsdomError", error => { throw error; });
  const dom = new JSDOM("<button>Run</button>", { url: "https://fixture.invalid/", pretendToBeVisual: true,
    runScripts: "outside-only", virtualConsole });
  const win = dom.window; let clicks = 0;
  win.Element.prototype.getBoundingClientRect = () => ({ width: 100, height: 30, top: 10, left: 10, right: 110, bottom: 40 });
  win.document.elementFromPoint = () => win.document.querySelector("button");
  win.document.querySelector("button").onclick = () => clicks++;
  const call = (fn, ...args) => {
    const invoke = () => win.eval(`(${fn.toString()})(...${JSON.stringify(args)})`);
    return fn === observeDocument ? withObservationClock(win, invoke) : invoke();
  };
  const observe = (identity, preview = false) => call(observeDocument, randomUUID(), !preview, identity);
  const command = (observed, action = "click") => ({ operationId: randomUUID(), snapshotId: observed.snapshotId,
    elementRef: observed.nodes[0].elementRef, action, parameters: action === "wait" ? { nameEquals: "Never", timeoutMs: 10000 } : {},
    deadlineMs: Date.now() + 10000 });
  return { win, call, observe, command, clicks: () => clicks,
    close: () => { win.__grokComputerUseSnapshot?.retire(); win.close(); } };
}

test("no observation can initialize execution or omit, duplicate, reorder or invent its identity", () => {
  const f = fixture();
  try {
    assert.throws(() => f.observe(stamp(1, 2)), /execution/);
    f.call(fenceDocument, stamp(1, 1));
    for (const bad of [undefined, null, {}, stamp(0, 1), stamp(1, 0), stamp(1, 1), stamp(2, 2),
      stamp(1, Number.MAX_SAFE_INTEGER + 1), { ...stamp(1, 2), extra: true }]) {
      assert.throws(() => f.observe(bad), /execution/);
      assert.equal(f.win.__grokComputerUseSnapshot, undefined);
    }
  } finally { f.close(); }
});

test("restart closes the old epoch, including late higher-sequence share/observe; old cleanup cannot close the new owner", async () => {
  const f = fixture();
  try {
    f.call(fenceDocument, stamp(1, 1));
    const old = f.observe(stamp(1, 2)); const action = f.command(old);
    assert.equal((await f.call(cancelDocumentOperation, old.snapshotId, action.operationId, true)).status, "settled");
    assert.throws(() => f.observe(stamp(1, 3)), /execution/);
    assert.throws(() => f.call(fenceDocument, stamp(1, 4)), /execution/);
    f.call(fenceDocument, stamp(2, 1));
    const current = f.observe(stamp(2, 2));
    assert.throws(() => f.observe(stamp(1, 1000)), /execution/);
    assert.throws(() => f.call(fenceDocument, stamp(1, 1001)), /execution/);
    assert.equal((await f.call(cancelDocumentOperation, old.snapshotId, action.operationId, true)).status, "snapshot_missing");
    assert.equal((await f.call(actDocument, action)).status, "rejected");
    assert.equal((await f.call(actDocument, f.command(current))).status, "applied");
    assert.equal(f.clicks(), 1);
  } finally { f.close(); }
});

test("late earlier observation cannot replace newer refs, while preview keeps their action usable", async () => {
  const f = fixture();
  try {
    f.call(fenceDocument, stamp(1, 1)); f.observe(stamp(1, 2));
    const current = f.observe(stamp(1, 4)); const retained = f.win.__grokComputerUseSnapshot;
    assert.throws(() => f.observe(stamp(1, 3)), /execution/);
    f.observe(stamp(1, 5), true);
    assert.equal(f.win.__grokComputerUseSnapshot, retained);
    assert.equal((await f.call(actDocument, f.command(current))).status, "applied"); assert.equal(f.clicks(), 1);
  } finally { f.close(); }
});

test("a new execution fence cannot cross an active Wait; only its exact cleanup cancels and joins it", async () => {
  const f = fixture(); let running;
  try {
    f.call(fenceDocument, stamp(1, 1)); const observed = f.observe(stamp(1, 2)); const command = f.command(observed, "wait");
    running = f.call(actDocument, command);
    assert(f.win.__grokComputerUseSnapshot.operation);
    assert.throws(() => f.call(fenceDocument, stamp(2, 1)), /busy/);
    assert.equal(f.win.__grokComputerUseExecution.epoch, 1);
    assert.equal((await f.call(cancelDocumentOperation, observed.snapshotId, randomUUID(), true)).status, "operation_mismatch");
    assert(f.win.__grokComputerUseExecution.current());
    assert.equal((await f.call(cancelDocumentOperation, observed.snapshotId, command.operationId, true)).status, "settled");
    assert.equal((await running).status, "rejected");
    assert.equal(f.win.__grokComputerUseSnapshot.operation, null);
    f.call(fenceDocument, stamp(2, 1)); assert(f.observe(stamp(2, 2)).nodes.length);
  } finally { f.close(); await running; }
});

test("clock waits for the committed epoch and concurrent calls get distinct increasing immutable identities", async () => {
  let release; const clock = new ExecutionClock(new Promise(resolve => { release = resolve; }));
  let completed = 0; const calls = Array.from({ length: 8 }, () => clock.next().then(value => { completed++; return value; }));
  await Promise.resolve(); assert.equal(completed, 0); release(3);
  const values = await Promise.all(calls);
  assert.deepEqual(values, Array.from({ length: 8 }, (_, i) => stamp(3, i + 1)));
  for (const value of values) assert(Object.isFrozen(value));
  for (const bad of [0, -1, null, undefined, Number.MAX_SAFE_INTEGER + 1, Promise.reject(new Error("storage failure"))]) {
    await assert.rejects(new ExecutionClock(bad).next());
  }
});
