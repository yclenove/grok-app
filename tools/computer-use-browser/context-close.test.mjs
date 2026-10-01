import test from "node:test";
import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { ContextClose, waitForCleanup } from "./context-close.mjs";
import { RunOperations } from "./run-operations.mjs";

function fixture() {
  const context = new EventEmitter();
  let finish;
  let fail;
  let calls = 0;
  const nativeClose = new Promise((resolve, reject) => { finish = resolve; fail = reject; });
  context.close = async () => { if (++calls === 1) await nativeClose; };
  return { context, finish, fail, calls: () => calls, close: new ContextClose(context) };
}

test("concurrent close paths share the original physical close and reject new admission", async () => {
  const f = fixture();
  f.close.check();
  const first = f.close.close();
  const second = f.close.close();
  assert.equal(first, second);
  let done = false;
  second.then(() => { done = true; });
  await Promise.resolve();
  assert.equal(f.calls(), 1);
  assert.equal(done, false);
  assert.throws(() => f.close.check(), e => e.workerCode === "profile_closing" && e.completion === "not_started");
  f.context.emit("close");
  assert.equal(f.close.close(), first, "a close event cannot bypass the original pending promise");
  f.finish();
  await first;
  await f.close.close();
  assert.equal(done, true);
  assert.equal(f.calls(), 1);
});

test("failed close is retained, never replaced with Playwright's second-call success", async () => {
  const f = fixture();
  const failure = new Error("transport lost");
  const first = f.close.close();
  f.fail(failure);
  await assert.rejects(first, e => e === failure);
  assert.equal(f.close.close(), first);
  await assert.rejects(f.close.close(), e => e === failure);
  assert.equal(f.calls(), 1);
  f.context.emit("close");
  assert.equal(f.close.close(), first, "a transport close event cannot turn a failed physical close into success");
  await assert.rejects(f.close.close(), e => e === failure);
  assert.equal(f.calls(), 1, "a failed physical close cannot be redispatched");
  assert.throws(() => f.close.check());
});

test("resolved transport without actual close stays unknown and cannot release the slot", async () => {
  const f = fixture();
  f.finish();
  await assert.rejects(f.close.close(), e => e.workerCode === "context_close_unconfirmed" && e.completion === "unknown");
  await assert.rejects(f.close.close());
  assert.equal(f.calls(), 1);
  f.context.emit("close");
  await f.close.close();
});

test("naturally closed contexts are not dispatched again or reopened", async () => {
  const f = fixture();
  f.context.emit("close");
  await f.close.close();
  assert.equal(f.calls(), 0);
  assert.throws(() => f.close.check());
});

test("synchronous close failure is shared by all callers", async () => {
  const f = fixture();
  let calls = 0;
  f.context.close = () => { calls += 1; throw new Error("native dispatch failed"); };
  const results = await Promise.allSettled([f.close.close(), f.close.close()]);
  assert.equal(calls, 1);
  assert.deepEqual(results.map(r => r.status), ["rejected", "rejected"]);
  assert.equal(results[0].reason, results[1].reason);
});

test("Stop readback stays occupied through physical close even at full ordinary capacity", async () => {
  const runs = new RunOperations({ maxActive: 1 });
  const request = runs.begin("owner", 1);
  const f = fixture();
  const closer = new ContextClose(f.context, () => runs.holdResourceCleanup("owner", f.context));
  runs.stop("owner");
  const pending = closer.close();
  request.finish();
  assert.equal(runs.status("owner", 1).idle, false, "aborted requests do not mean browser cleanup finished");
  assert.equal(await runs.waitIdle("owner", 1), false);
  f.context.emit("close");
  assert.equal(runs.status("owner", 1).idle, false, "original close promise is still pending");
  f.finish();
  await pending;
  assert.equal(await runs.waitIdle("owner", 1), true);
});

test("uncertain close keeps cleanup occupancy after the request errors", async () => {
  const runs = new RunOperations();
  runs.begin("owner", 1).finish();
  const f = fixture();
  const closer = new ContextClose(f.context, () => runs.holdResourceCleanup("owner", f.context));
  const pending = closer.close();
  f.fail(new Error("transport gone"));
  await assert.rejects(pending);
  runs.pause("owner", 1);
  assert.equal(runs.status("owner", 1).idle, false);
  assert.throws(() => runs.resume("owner", 1, 2));
  f.context.emit("close");
  assert.equal(runs.status("owner", 1).idle, false,
    "a close event may precede native process exit and cannot redeem a rejected cleanup");
  assert.equal(runs.status("owner", 1).activeOperations, 1);
  assert.throws(() => runs.resume("owner", 1, 2));
});

for (const eventFirst of [true, false]) {
  test(`failed physical cleanup stays owned with close event ${eventFirst ? "before" : "after"} rejection`, async () => {
    const runs = new RunOperations();
    runs.begin("owner", 1).finish();
    runs.begin("independent-owner", 1).finish();
    const f = fixture();
    let releases = 0;
    const closer = new ContextClose(f.context, () => {
      const lease = runs.holdResourceCleanup("owner", f.context);
      return { finish: () => { releases += 1; lease.finish(); } };
    });
    const pending = closer.close();
    if (eventFirst) f.context.emit("close");
    const failure = new Error("physical close failed after protocol disconnect");
    f.fail(failure);
    await assert.rejects(pending, e => e === failure);
    if (!eventFirst) f.context.emit("close");
    runs.stop("owner");
    assert.equal(runs.status("owner", 1).idle, false);
    assert.equal(runs.status("owner", 1).activeOperations, 1);
    assert.equal(runs.status("independent-owner", 1).idle, true);
    assert.equal(releases, 0);
    assert.equal(closer.close(), pending);
    await assert.rejects(waitForCleanup(closer.close(), 100), e => e === failure);
    assert.equal(f.calls(), 1);
  });
}

test("successful physical cleanup can reconcile a late close event exactly once", async () => {
  const runs = new RunOperations();
  runs.begin("owner", 1).finish();
  const f = fixture();
  let releases = 0;
  const closer = new ContextClose(f.context, () => {
    const lease = runs.holdResourceCleanup("owner", f.context);
    return { finish: () => { releases += 1; lease.finish(); } };
  });
  const pending = closer.close();
  f.finish();
  await assert.rejects(pending, { workerCode: "context_close_unconfirmed" });
  assert.equal(runs.status("owner", 1).idle, false);
  f.context.emit("close");
  await closer.close();
  f.context.emit("close");
  assert.equal(runs.status("owner", 1).idle, true);
  assert.equal(releases, 1);
  assert.equal(f.calls(), 1);
});

test("physical cleanup is counted above all 64 occupied ordinary slots", async () => {
  const runs = new RunOperations();
  const requests = Array.from({ length: 64 }, () => runs.begin("owner", 1));
  const f = fixture();
  const closer = new ContextClose(f.context, () => runs.holdResourceCleanup("owner", f.context));
  runs.stop("owner");
  const pending = closer.close();
  assert.deepEqual(runs.status("owner", 1), {
    runRevision: 1, phase: "stopped", activeOperations: 65, idle: false,
  });
  for (const request of requests) request.finish();
  assert.equal(runs.status("owner", 1).activeOperations, 1);
  f.context.emit("close");
  assert.equal(runs.status("owner", 1).idle, false);
  f.finish();
  await pending;
  assert.equal(runs.status("owner", 1).idle, true);
});

test("a bounded cleanup reply never releases physical ownership or repeats close", async () => {
  const runs = new RunOperations();
  runs.begin("owner", 1).finish();
  const f = fixture();
  const closer = new ContextClose(f.context, () => runs.holdResourceCleanup("owner", f.context));
  const pending = closer.close();
  await assert.rejects(waitForCleanup(pending, 10), e =>
    e.statusCode === 409 && e.workerCode === "run_cleanup_pending" && e.completion === "unknown");
  assert.equal(runs.status("owner", 1).activeOperations, 1);
  runs.pause("owner", 1);
  assert.throws(() => runs.resume("owner", 1, 2));
  assert.equal(closer.close(), pending);
  assert.equal(f.calls(), 1);
  f.context.emit("close");
  assert.equal(runs.status("owner", 1).idle, false);
  f.finish();
  await waitForCleanup(closer.close(), 1000);
  assert.equal(runs.status("owner", 1).idle, true);
  assert.equal(f.calls(), 1);
});

test("cleanup can finish after the reply deadline without losing its completion", async () => {
  const f = fixture();
  let completed = 0;
  const pending = f.close.close().then(() => { completed += 1; return "closed"; });
  await assert.rejects(waitForCleanup(pending, 10), { workerCode: "run_cleanup_pending" });
  assert.equal(completed, 0);
  f.context.emit("close");
  f.finish();
  assert.equal(await pending, "closed");
  assert.equal(completed, 1);
});

test("late cleanup rejection stays observed and cannot turn a retry into success", async () => {
  const f = fixture();
  const pending = f.close.close();
  await assert.rejects(waitForCleanup(pending, 10), { workerCode: "run_cleanup_pending" });
  const failure = new Error("late physical close failure");
  f.fail(failure);
  // Give Node's unhandled-rejection checkpoint a turn before attaching a new
  // observer: the expired response must still observe its original cleanup.
  await new Promise(resolve => setImmediate(resolve));
  await assert.rejects(waitForCleanup(f.close.close(), 1000), e => e === failure);
  assert.equal(f.calls(), 1);
});

test("cleanup response budget covers a whole batch, not one timeout per context", async () => {
  const fixtures = Array.from({ length: 3 }, fixture);
  const pending = Promise.all(fixtures.map(f => f.close.close()));
  await assert.rejects(waitForCleanup(pending, 10), { workerCode: "run_cleanup_pending" });
  assert.deepEqual(fixtures.map(f => f.calls()), [1, 1, 1]);
  for (const f of fixtures) { f.context.emit("close"); f.finish(); }
  await pending;
});

test("bounded cleanup preserves immediate results and failures", async () => {
  assert.equal(await waitForCleanup(Promise.resolve("closed"), 1000), "closed");
  const failure = new Error("physical close failed");
  await assert.rejects(waitForCleanup(Promise.reject(failure), 1000), e => e === failure);
});
