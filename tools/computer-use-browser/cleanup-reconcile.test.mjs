import test from "node:test";
import assert from "node:assert/strict";
import { confirmRunCancellation, confirmWorkerShutdown } from "./fixtures/cleanup-reconcile.mjs";

const pending = { status: 409, body: { error: { code: "run_cleanup_pending", completion: "unknown" } } };
const done = { status: 200, body: { ok: true, shutdown: true } };

test("already-confirmed shutdown is not dispatched twice", async () => {
  const calls = [];
  const reply = await confirmWorkerShutdown(async path => { calls.push(path); return done; });
  assert.equal(reply, done);
  assert.deepEqual(calls, ["/shutdown"]);
});

test("pending shutdown polls inventory but still needs explicit physical confirmation", async () => {
  const replies = [pending, { status: 200, body: { openProfiles: ["owned"] } },
    { status: 200, body: { openProfiles: [] } }, done];
  const calls = [];
  let reported = 0;
  const reply = await confirmWorkerShutdown(async (path, timeout) => {
    calls.push(path);
    assert.ok(timeout > 0 && timeout <= 1000);
    return replies.shift();
  }, { budgetMs: 1000, onPending: () => { reported += 1; } });
  assert.equal(reply, done);
  assert.equal(reported, 1);
  assert.deepEqual(calls, ["/shutdown", "/health", "/health", "/shutdown"]);
});

test("other failures and unknown transport do not trigger a lifecycle retry", async () => {
  for (const reply of [{ status: 500, body: pending.body },
    { status: 409, body: { error: { code: "conflict", completion: "unknown" } } },
    { status: 409, body: { error: { code: "run_cleanup_pending", completion: "not_started" } } }]) {
    let calls = 0;
    assert.equal(await confirmWorkerShutdown(async () => { calls += 1; return reply; }), reply);
    assert.equal(calls, 1);
  }
  const failure = new Error("transport lost");
  let calls = 0;
  await assert.rejects(confirmWorkerShutdown(async () => { calls += 1; throw failure; }), e => e === failure);
  assert.equal(calls, 1);
});

test("an exhausted cleanup budget stays unknown, never success", async () => {
  const calls = [];
  const reply = await confirmWorkerShutdown(async path => {
    calls.push(path);
    return path === "/shutdown" ? pending : { status: 200, body: { openProfiles: ["owned"] } };
  }, { budgetMs: 10 });
  assert.equal(reply, pending);
  assert.equal(calls.filter(path => path === "/shutdown").length, 1);
});

test("failed inventory read cannot declare a pending shutdown complete", async () => {
  const calls = [];
  const reply = await confirmWorkerShutdown(async path => {
    calls.push(path);
    return path === "/shutdown" ? pending : { status: 500, body: {} };
  });
  assert.equal(reply, pending);
  assert.deepEqual(calls, ["/shutdown", "/health"]);
});

const identity = { owner: "original-owner", runRevision: 3 };
const cancelled = { status: 200, body: { ok: true, owner: identity.owner, closedProfiles: ["owned"] } };
const stopped = (idle, activeOperations = idle ? 0 : 1) => ({ status: 200,
  body: { runRevision: identity.runRevision, phase: "stopped", idle, activeOperations } });

test("confirmed cancellation is not dispatched twice", async () => {
  const calls = [];
  assert.equal(await confirmRunCancellation(async (path, body) => {
    calls.push([path, body]); return cancelled;
  }, identity), cancelled);
  assert.deepEqual(calls, [["/cancel-run", { owner: identity.owner }]]);
});

test("pending cancellation observes the exact stopped run then requires final confirmation", async () => {
  const replies = [pending, stopped(false), stopped(true), cancelled];
  const calls = [];
  let reported = 0;
  assert.equal(await confirmRunCancellation(async (path, body, timeoutMs) => {
    calls.push([path, body]);
    assert.ok(timeoutMs > 0 && timeoutMs <= 1000);
    return replies.shift();
  }, identity, { budgetMs: 1000, onPending: () => { reported += 1; } }), cancelled);
  assert.equal(reported, 1);
  assert.deepEqual(calls, [["/cancel-run", { owner: identity.owner }],
    ["/run-status", identity], ["/run-status", identity], ["/cancel-run", { owner: identity.owner }]]);
});

test("unrelated revision and malformed or unavailable readback cannot confirm cancellation", async () => {
  for (const status of [{ status: 500, body: {} },
    { status: 200, body: { ...stopped(true).body, runRevision: 4 } },
    { status: 200, body: { ...stopped(true).body, phase: "paused" } },
    { status: 200, body: { ...stopped(true).body, idle: "true" } },
    { status: 200, body: { ...stopped(true).body, activeOperations: -1 } }]) {
    const calls = [];
    assert.equal(await confirmRunCancellation(async path => {
      calls.push(path); return path === "/cancel-run" ? pending : status;
    }, identity), pending);
    assert.deepEqual(calls, ["/cancel-run", "/run-status"]);
  }
});

test("occupied cancellation or contradictory idle readback exhausts the original budget without success", async () => {
  for (const status of [stopped(false), stopped(true, 1)]) {
    const calls = [];
    assert.equal(await confirmRunCancellation(async path => {
      calls.push(path); return path === "/cancel-run" ? pending : status;
    }, identity, { budgetMs: 10 }), pending);
    assert.equal(calls.filter(path => path === "/cancel-run").length, 1);
  }
});

test("cancellation transport and non-pending failures never trigger a retry", async () => {
  for (const response of [{ status: 500, body: pending.body },
    { status: 409, body: { error: { code: "run_fenced", completion: "unknown" } } },
    { status: 409, body: { error: { code: "run_cleanup_pending", completion: "not_started" } } }]) {
    let calls = 0;
    assert.equal(await confirmRunCancellation(async () => { calls += 1; return response; }, identity), response);
    assert.equal(calls, 1);
  }
  const transport = new Error("transport unavailable");
  let calls = 0;
  await assert.rejects(confirmRunCancellation(async () => { calls += 1; throw transport; }, identity), error => error === transport);
  assert.equal(calls, 1);
});
