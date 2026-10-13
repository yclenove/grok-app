import { test } from "node:test";
import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { ExtensionTransport } from "./extension-transport.mjs";
import { ExtensionActionTransport } from "./action-transport.mjs";
import { ExecutionSuspended } from "./pairing-client.mjs";

function packet(sequence = 1) {
  const binding = { protocol: 2, requestId: randomUUID(), connection: { instanceId: randomUUID(), connectionNonce: randomUUID(), generation: 1 },
    session: "owner", runId: "run", tabId: "101", documentId: "0".repeat(32), documentGeneration: 7, grantGeneration: 2, snapshotId: randomUUID() };
  const { snapshotId, ...fields } = binding;
  return { request: { ...fields, sequence, deadlineMs: Date.now() + 10000,
    command: { kind: "act", action: "click", snapshotId, elementRef: snapshotId + "-1", parameters: {} } },
  proof: { binding, completionKey: "a".repeat(64) } };
}

function barrier() {
  let enter; let release;
  const reached = new Promise(resolve => { enter = resolve; });
  const finished = new Promise(resolve => { release = resolve; });
  return { reached, release, wait: () => { enter(); return finished; } };
}

const untilAbort = signal => new Promise(resolve => {
  signal.addEventListener("abort", resolve, { once: true }); if (signal.aborted) resolve();
});

function fixture() {
  const calls = []; const result = barrier(); const dispatch = packet();
  let supplied = false;
  const client = { connectionEpoch: 1,
    async executionReady() { return true; },
    async negotiateActions() { calls.push("negotiate"); },
    async pollExtension(_epoch, signal) { calls.push("observe-poll"); await untilAbort(signal); return null; },
    async pollAction(_epoch, signal) { calls.push("action-poll"); if (!supplied) { supplied = true; return dispatch; } await untilAbort(signal); return null; },
    async claimAction() { calls.push("claim"); },
    completionScope() { return { async prepare() {}, async status() { return { phase: "claimed", cancelRequested: false }; }, async settle() { calls.push("settle"); } }; },
    async completeAction(_epoch, body) { calls.push("result"); result.release(body); },
    async retireTransport() { calls.push("retire"); client.connectionEpoch = null; transport.reset(); },
  };
  client.negotiateObservations = async () => {};
  const transport = new ExtensionTransport({ client, journal: {}, observationJournal: {}, recovered: () => {}, observe: async () => { throw new Error("no observation request"); },
    act: async () => { calls.push("act"); return { status: "applied", detail: "clicked", physicallySettled: true }; },
    wait: async () => {} });
  return { client, transport, calls, result, dispatch };
}

test("production transport negotiates and submits one bound claim, one action, settlement and one result", async () => {
  const f = fixture(); f.transport.start();
  const result = await f.result.wait();
  f.transport.reset(); await f.transport.task;
  assert.equal(result.outcome.status, "applied");
  assert.deepEqual(f.calls.filter(call => ["negotiate", "claim", "act", "settle", "result"].includes(call)),
    ["negotiate", "claim", "act", "settle", "result"]);
});

test("suspended negotiation or polling retains its original action task until readiness returns", async () => {
  for (const stage of ["negotiate", "poll"]) {
    const f = fixture(); const suspended = barrier(); const resume = barrier();
    let ready = true; let negotiations = 0; let polls = 0;
    const controller = new AbortController();
    f.client.executionReady = async () => ready;
    f.client.negotiateActions = async () => {
      negotiations++;
      if (stage === "negotiate" && negotiations === 1) { ready = false; throw new ExecutionSuspended(); }
    };
    f.client.pollAction = async (_epoch, signal) => {
      polls++;
      if (stage === "poll" && polls === 1) { ready = false; throw new ExecutionSuspended(); }
      if (polls === (stage === "poll" ? 2 : 1)) return f.dispatch;
      await untilAbort(signal); return null;
    };
    const transport = new ExtensionActionTransport({ client: f.client, journal: {}, recovered: () => {},
      act: async () => { f.calls.push("act"); return { status: "applied", detail: "clicked", physicallySettled: true }; },
      wait: async signal => {
        if (!ready) { suspended.release(); await resume.wait(); }
        else await untilAbort(signal);
      } });
    const original = transport.run(1, controller.signal);
    await suspended.wait();
    assert.equal(f.calls.includes("act"), false); assert.equal(f.client.connectionEpoch, 1);
    ready = true; resume.release(); await f.result.wait();
    controller.abort(); await original;
    assert.equal(f.calls.filter(call => call === "act").length, 1);
    assert.equal(f.calls.filter(call => call === "claim").length, 1);
    assert.equal(f.calls.includes("retire"), false);
    assert.equal(negotiations, stage === "negotiate" ? 2 : 1);
  }
});

test("failed negotiation leaves v1 observing without polling or executing actions", async () => {
  const f = fixture(); const negotiation = barrier();
  f.client.negotiateActions = async () => { await negotiation.wait(); throw new Error("unsupported"); };
  f.transport.start(); await negotiation.reached; negotiation.release();
  await new Promise(resolve => setImmediate(resolve));
  assert(f.calls.includes("observe-poll")); assert(!f.calls.includes("action-poll")); assert(!f.calls.includes("act"));
  assert.equal(f.client.connectionEpoch, 1);
  f.transport.reset(); await f.transport.task;
});

test("a lost business result is not retried and does not repeat the browser action", async () => {
  const f = fixture(); const submitted = barrier();
  f.client.completeAction = async () => { f.calls.push("result"); submitted.release(); throw new Error("lost"); };
  f.transport.start(); await submitted.wait();
  await new Promise(resolve => setImmediate(resolve)); f.transport.reset(); await f.transport.task;
  assert.equal(f.calls.filter(call => call === "result").length, 1);
  assert.equal(f.calls.filter(call => call === "act").length, 1);
});

test("replayed action sequence retires the connection without a second effect", async () => {
  const f = fixture(); const retired = barrier();
  f.client.pollAction = async () => f.dispatch;
  const retire = f.client.retireTransport;
  f.client.retireTransport = async (...args) => { await retire(...args); retired.release(); };
  f.transport.start(); await retired.wait(); await f.transport.task;
  assert.equal(f.calls.filter(call => call === "claim").length, 1);
  assert.equal(f.calls.filter(call => call === "act").length, 1);
});

test("reset retains original action and watcher promises and discards old business results", async () => {
  const f = fixture(); const action = barrier(); const watcher = barrier(); const calls = [];
  f.client.completionScope = () => ({ async prepare() {}, async status() { await watcher.wait(); return { phase: "claimed", cancelRequested: false }; },
    async settle() { calls.push("settle"); } });
  const transport = new ExtensionTransport({ client: f.client, journal: {}, observationJournal: {}, recovered: () => {}, observe: async () => {},
    act: async () => { await action.wait(); return { status: "applied", detail: "clicked", physicallySettled: true }; } });
  transport.start(); const task = transport.task;
  await action.reached; await watcher.reached;
  transport.reset(); f.client.connectionEpoch++;
  let finished = false; void task.then(() => { finished = true; });
  action.release(); await new Promise(resolve => setImmediate(resolve));
  assert(!finished); assert.deepEqual(calls, []);
  watcher.release(); await task;
  assert.deepEqual(calls, ["settle"]); assert(!f.calls.includes("result"));
});
