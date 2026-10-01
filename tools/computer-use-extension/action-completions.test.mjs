import { test } from "node:test";
import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { ActionCompletions, copyBoundAction } from "./action-completions.mjs";

function action(tab = "101") {
  const binding = { protocol: 2, requestId: randomUUID(), connection: { instanceId: randomUUID(), connectionNonce: randomUUID(), generation: 1 },
    session: "owner", runId: "run", tabId: tab, documentId: "0".repeat(32), documentGeneration: 7, grantGeneration: 1, snapshotId: randomUUID() };
  const { snapshotId, ...fields } = binding;
  return { proof: { binding, completionKey: "a".repeat(64) },
    request: { ...fields, sequence: 1, deadlineMs: Date.now() + 10000,
      command: { kind: "act", snapshotId, elementRef: snapshotId + "-1", action: "click", parameters: {} } } };
}
function barrier() {
  let enter; let release;
  const reached = new Promise(resolve => { enter = resolve; });
  const done = new Promise(resolve => { release = resolve; });
  return { reached, release, wait: () => { enter(); return done; } };
}
function fixture(overrides = {}) {
  const calls = []; const timers = new Map();
  const scope = { async prepare() {}, async status() { calls.push("status"); return { phase: "claimed", cancelRequested: false }; },
    async settle() { calls.push("settle"); }, async recover() { return false; } };
  const client = { connectionEpoch: 1, completionScope() { calls.push("scope"); return scope; },
    async claimAction(_epoch, _dispatch, signal) { calls.push("claim"); if (signal.aborted) throw new Error("cancelled"); } };
  const owner = new ActionCompletions({ client, act: async () => {
    calls.push("act"); return { status: "applied", detail: "clicked", physicallySettled: true };
  }, schedule(fn, delay) { const id = randomUUID(); timers.set(id, { fn, delay }); return id; },
  cancel(id) { timers.delete(id); }, ...overrides });
  return { client, scope, owner, calls, timers };
}

test("action packet strictly binds every identity and only supported typed operations", () => {
  const { request, proof } = action();
  assert(copyBoundAction(request, proof));
  for (const key of ["protocol", "requestId", "session", "runId", "tabId", "documentId", "documentGeneration", "grantGeneration"]) {
    assert.throws(() => copyBoundAction({ ...request, [key]: "other" }, proof), /invalidCompletionAction/);
  }
  for (const command of [{ ...request.command, script: "x" }, { ...request.command, snapshotId: randomUUID() },
    { ...request.command, action: "eval", parameters: {} }, { ...request.command, parameters: { button: "right" } },
    { ...request.command, action: "type_text", parameters: { text: "x".repeat(4001) } },
    { ...request.command, action: "wait", parameters: { nameEquals: "yes", timeoutMs: null } }]) {
    assert.throws(() => copyBoundAction({ ...request, command }, proof), /invalidCompletionAction/);
  }
  for (const [action, parameters] of [["type_text", { text: "你好🚀" }], ["wait", { nameEquals: "Done" }], ["scroll", { delta: -500 }]]) {
    assert(copyBoundAction({ ...request, command: { ...request.command, action, parameters } }, proof));
  }
});

test("claim reply loss, pre-cancel and connection replacement never execute or retry actions", async () => {
  for (const mode of ["lost", "pre-cancel", "replace"]) {
    const f = fixture(); const { request, proof } = action(); const controller = new AbortController();
    if (mode === "pre-cancel") controller.abort();
    const claim = f.client.claimAction;
    f.client.claimAction = async (...args) => {
      await claim(...args);
      if (mode === "lost") throw new Error("claim response lost");
      if (mode === "replace") f.client.connectionEpoch++;
    };
    const result = await f.owner.run(1, request, proof, controller.signal);
    assert.equal(result.status, "rejected"); assert(f.owner.idle());
    assert.deepEqual(f.calls, ["scope", "claim", "settle"]);
    await f.owner.run(1, request, proof);
    assert.equal(f.calls.filter(call => call === "claim").length, 1);
  }
});

test("cleanup record storage must acknowledge before claim, including cancellation during the write", async () => {
  for (const cancel of [false, true]) {
    const f = fixture(); const persistence = barrier(); const { request, proof } = action();
    f.scope.prepare = () => persistence.wait();
    const controller = new AbortController();
    const running = f.owner.run(1, request, proof, controller.signal);
    try {
      await new Promise(resolve => setImmediate(resolve));
      assert(!f.calls.includes("claim"), "unacknowledged storage must not acquire execution permission");
      assert(!f.calls.includes("act"));
      if (cancel) controller.abort();
      persistence.release();
      const result = await running;
      assert.equal(result.status, cancel ? "rejected" : "applied");
      assert.equal(f.calls.filter(call => call === "act").length, cancel ? 0 : 1);
    } finally { persistence.release(); await running; }
  }
});

test("failed cleanup-record persistence cannot claim or execute the action", async () => {
  const f = fixture(); const { request, proof } = action();
  f.scope.prepare = async () => { throw new Error("storage unavailable"); };
  assert.equal((await f.owner.run(1, request, proof)).status, "rejected");
  assert(!f.calls.includes("claim")); assert(!f.calls.includes("act"));
});

test("reset retains original action and status request until both have settled", async () => {
  const acting = barrier(); const status = barrier(); let signal;
  const f = fixture({ act: async (_request, incoming) => {
    signal = incoming; await acting.wait(); return { status: "applied", detail: "clicked", physicallySettled: true };
  } });
  f.scope.status = async () => { await status.wait(); return { phase: "claimed", cancelRequested: false }; };
  const { request, proof } = action(); let finished = false;
  const running = f.owner.run(1, request, proof).then(value => { finished = true; return value; });
  await acting.reached; await status.reached;
  f.client.connectionEpoch++; f.owner.reset(); assert(signal.aborted);
  assert(!f.owner.idle()); assert(!f.calls.includes("settle"));
  acting.release(); await Promise.resolve();
  assert(!finished); assert(!f.calls.includes("settle"));
  status.release(); assert.equal((await running).status, "unknown");
  assert(f.owner.idle()); assert.equal(f.calls.filter(call => call === "settle").length, 1);
});

test("cancellation after the action result but before watcher cleanup cannot publish success", async () => {
  const status = barrier(); const completed = barrier();
  const f = fixture({ act: async () => { await completed.wait(); return { status: "verified", detail: "verified", physicallySettled: true }; } });
  f.scope.status = async () => { await status.wait(); return { phase: "claimed", cancelRequested: false }; };
  const { request, proof } = action(); const running = f.owner.run(1, request, proof);
  await completed.reached; await status.reached;
  completed.release(); await new Promise(resolve => setImmediate(resolve));
  assert(!f.owner.idle()); f.owner.reset(); status.release();
  assert.equal((await running).status, "unknown"); assert(f.owner.idle());
});

test("Host cancellation and status failure cancel the owned action and never replay it", async () => {
  for (const mode of ["cancel", "failure"]) {
    let dispatched = 0;
    const f = fixture({ act: async (_request, signal) => {
      dispatched++;
      await new Promise(resolve => { signal.addEventListener("abort", resolve, { once: true }); if (signal.aborted) resolve(); });
      return { status: "rejected", detail: "cancelled", physicallySettled: true };
    } });
    f.scope.status = async () => { if (mode === "failure") throw new Error("offline"); return { phase: "claimed", cancelRequested: true }; };
    const { request, proof } = action();
    assert.equal((await f.owner.run(1, request, proof)).status, "unknown");
    assert.equal(dispatched, 1); assert(f.owner.idle());
    await f.owner.retryCleanup(); assert.equal(dispatched, 1);
  }
});

test("unproven native completion holds capacity across reset and does not send a false receipt", async () => {
  let dispatched = 0;
  const f = fixture({ act: async () => { dispatched++; return { status: "unknown", detail: "lost", physicallySettled: false }; } });
  for (let index = 0; index < 8; index++) {
    const { request, proof } = action(String(index));
    assert.equal((await f.owner.run(1, request, proof)).status, "unknown");
  }
  f.owner.reset(); await f.owner.retryCleanup();
  const ninth = action("9"); assert.equal((await f.owner.run(1, ninth.request, ninth.proof)).status, "rejected");
  assert.equal(dispatched, 8); assert(!f.owner.idle()); assert(!f.calls.includes("settle")); assert.equal(f.timers.size, 1);
});

test("later physical evidence releases only the original slot and never resubmits its unknown business action", async () => {
  const released = []; let attempts = 0;
  const f = fixture({ recovered: binding => released.push(binding.requestId),
    act: async () => { attempts++; return { status: "unknown", detail: "lost", physicallySettled: false }; } });
  const original = action();
  assert.equal((await f.owner.run(1, original.request, original.proof)).status, "unknown");
  f.owner.reset(); await f.owner.retryCleanup(); assert(!f.owner.idle()); assert.deepEqual(released, []);
  f.scope.recover = async () => true;
  await f.owner.retryCleanup(); assert(f.owner.idle()); assert.deepEqual(released, [original.request.requestId]);
  assert.equal(attempts, 1); assert.equal(f.calls.filter(call => call === "claim").length, 1);
  assert.equal(f.calls.filter(call => call === "settle").length, 1); assert.equal(f.timers.size, 0);
});

test("a pending native cleanup on one tab does not block another tab's bounded retry", async () => {
  const first = action("101"); const second = action("102"); const held = barrier(); const settled = []; let otherReady = false;
  const f = fixture({ act: async () => ({ status: "unknown", detail: "lost", physicallySettled: false }) });
  f.client.completionScope = (_epoch, proof) => ({ ...f.scope,
    async recover() { if (proof.binding.requestId === first.proof.binding.requestId) { await held.wait(); return true; } return otherReady; },
    async settle() { settled.push(proof.binding.requestId); } });
  await f.owner.run(1, first.request, first.proof); await f.owner.run(1, second.request, second.proof);
  const retry = f.owner.retryCleanup();
  try {
    await held.reached; await new Promise(resolve => setImmediate(resolve));
    otherReady = true;
    const [id, task] = f.timers.entries().next().value; f.timers.delete(id); task.fn();
    await new Promise(resolve => setImmediate(resolve));
    assert.deepEqual(settled, [second.proof.binding.requestId]); assert(!f.owner.idle());
    held.release(); await retry; assert(f.owner.idle()); assert.equal(f.timers.size, 0);
  } finally { held.release(); await retry; }
});

test("only failed cleanup is retried with bounded backoff and no retained command input", async () => {
  const f = fixture(); let attempts = 0;
  f.scope.settle = async () => { f.calls.push("settle"); if (++attempts < 3) throw new Error("reply lost"); };
  const { request, proof } = action();
  assert.equal((await f.owner.run(1, request, proof)).status, "applied");
  assert(!f.owner.idle()); assert.equal(f.timers.size, 1);
  f.client.connectionEpoch = null; f.owner.reset();
  await f.owner.retryCleanup(); assert(!f.owner.idle());
  await f.owner.retryCleanup(); assert(f.owner.idle()); assert.equal(f.timers.size, 0);
  assert.equal(f.calls.filter(call => call === "act").length, 1);
  assert.equal(f.calls.filter(call => call === "claim").length, 1);
  assert.equal(attempts, 3);
});

test("an exception after dispatch is unknown and cannot manufacture physical completion", async () => {
  const f = fixture({ act: async () => { throw new Error("sensitive task input"); } });
  const { request, proof } = action();
  assert.deepEqual(await f.owner.run(1, request, proof), { status: "unknown", detail: "action_completion_unknown" });
  assert(!f.owner.idle()); assert(!f.calls.includes("settle"));
});

test("scheduled retries retain only one timer and cap the delay without repeating a browser action", async () => {
  const f = fixture(); let online = false;
  f.scope.settle = async () => { if (!online) throw new Error("offline"); };
  const { request, proof } = action(); await f.owner.run(1, request, proof);
  for (let index = 0; index < 9; index++) {
    assert.equal(f.timers.size, 1);
    const [id, task] = f.timers.entries().next().value;
    assert.equal(task.delay, Math.min(30000, 1000 * 2 ** index));
    f.timers.delete(id); task.fn(); await f.owner.retryCleanup();
  }
  online = true; await f.owner.retryCleanup();
  assert(f.owner.idle()); assert.equal(f.timers.size, 0);
  assert.equal(f.calls.filter(call => call === "act").length, 1);
  assert.equal(f.calls.filter(call => call === "claim").length, 1);
});
