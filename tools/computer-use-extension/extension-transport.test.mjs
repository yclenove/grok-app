import { test } from "node:test";
import assert from "node:assert/strict";
import { ExtensionTransport, validRequest } from "./extension-transport.mjs";
import { ExecutionSuspended, waitExecutionReady } from "./pairing-client.mjs";
const id = "00000000-0000-4000-8000-000000000001";
function request() {
  const request = { protocol: 2, requestId: id, sequence: 1, deadlineMs: Date.now() + 9000,
    connection: { instanceId: id, connectionNonce: id, generation: 1 }, session: "owner", runId: "run",
    tabId: "101", documentId: "0".repeat(32), documentGeneration: 7, grantGeneration: 1,
    command: { kind: "observe", snapshotId: id, screenshot: false, preview: false } };
  const { sequence: _seq, deadlineMs: _deadline, command: _command, ...binding } = request;
  request.completion = { purpose: "observation", proof: { binding: { ...binding, snapshotId: id }, completionKey: "a".repeat(64) } };
  return request;
}

function transport(options) {
  // Synthetic execution/receipt fixture still follows the real lifecycle; the
  // original transport tests keep their sequence/cancellation assertions.
  const client = options.client;
  client.negotiateObservations ??= async () => {};
  client.claimObservation ??= async () => {};
  client.observationScope ??= () => ({ prepare: async () => {},
    status: async () => ({ phase: "claimed", cancelRequested: false }), settle: async () => {} });
  return new ExtensionTransport({ ...options, observationJournal: { begin: async () => {} } });
}
test("transport accepts only typed bounded fresh requests", () => {
  const r = request(); assert(validRequest(r, 0));
  for (const bad of [{ ...r, script: "x" }, { ...r, sequence: 0 }, { ...r, deadlineMs: Date.now() - 1 },
    { ...r, command: { kind: "eval", source: "x" } }, { ...r, grantGeneration: 0 }, { ...r, session: "x".repeat(257) },
    { ...r, tabId: 101 }, { ...r, tabId: "0101" }, { ...r, tabId: "9999999999999999" },
    { ...r, command: { ...r.command, script: "x" } }]) assert(!validRequest(bad, 0));
  assert(!validRequest(r, 1));
});

test("duplicate transport delivery cannot inspect twice and retires the failed connection", async () => {
  let observed = 0; let submitted = 0; let retired = 0;
  const r = request();
  const client = { connectionEpoch: 1, async executionReady() { return true; }, async pollExtension() { return r; },
    async completeExtension() { submitted++; }, async retireTransport(epoch) { assert.equal(epoch, 1); retired++; } };
  const running = transport({ client, observe: async () => { observed++; return {}; }, wait: async () => {} });
  running.start(); await running.task;
  assert.equal(observed, 1); assert.equal(submitted, 1); assert.equal(retired, 1);
});

test("late observation after connection retirement sends no result", async () => {
  let release; let reached;
  const barrier = new Promise(resolve => { reached = resolve; }); let submitted = 0;
  const client = { connectionEpoch: 1, async executionReady() { return true; }, async pollExtension() { return request(); },
    async completeExtension() { submitted++; }, async retireTransport() { assert.fail("intentional reset is not a failure"); } };
  let originalSignal;
  const running = transport({ client, observe: async (_request, signal) => {
    originalSignal = signal; reached(); await new Promise(resolve => { release = resolve; }); return {};
  } });
  running.start(); await barrier;
  const originalTask = running.task;
  let joined = false; void originalTask.then(() => { joined = true; });
  client.connectionEpoch = 2; running.reset();
  assert.equal(originalSignal.aborted, true);
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(joined, false, "the original loop must retain its browser promise after reset");
  release(); await originalTask;
  assert.equal(submitted, 0);
});

test("ambiguous result is never resent and an observation failure sends only a typed rejection", async () => {
  let polls = 0; const submitted = [];
  const client = { connectionEpoch: 1, async executionReady() { return true; }, async pollExtension() { if (++polls > 1) throw new Error("offline"); return request(); },
    async completeExtension(_epoch, result) { submitted.push(result); throw new Error("timeout"); }, async retireTransport() {} };
  const running = transport({ client, observe: async () => { throw new Error("private page details"); }, wait: async () => {} });
  running.start(); await running.task;
  assert.equal(submitted.length, 1);
  assert.deepEqual(submitted[0].outcome, { kind: "rejected", reason: "documentChanged" });
  assert(!JSON.stringify(submitted).includes("private page details"));
});

test("metadata preparation waits before either execution loop and never retires a healthy pairing", async () => {
  let ready = false; let release; let polls = 0; let negotiated = 0; let retired = 0;
  const client = { connectionEpoch: 1, async executionReady() { return ready; },
    async pollExtension() { polls++; throw new Error("finished fixture"); },
    async negotiateActions() { negotiated++; throw new Error("unsupported fixture"); },
    async retireTransport() { retired++; } };
  const running = transport({ client, observe: async () => assert.fail("no observe"),
    journal: {}, recovered: () => {}, act: async () => assert.fail("no action"), wait: async () => {
      await new Promise(resolve => { release = resolve; });
    } });
  running.start(); await new Promise(resolve => setImmediate(resolve));
  assert.equal(polls, 0); assert.equal(negotiated, 0); assert.equal(retired, 0);
  ready = true; release(); await running.task;
  assert.equal(polls, 1); assert.equal(negotiated, 1); assert.equal(retired, 1);
});

test("cancelled readiness retains its original promise and cannot launch late execution", async () => {
  let release; let entered;
  const barrier = new Promise(resolve => { entered = resolve; });
  const client = { connectionEpoch: 1, async executionReady() {
    entered(); return new Promise(resolve => { release = resolve; });
  }, async pollExtension() { assert.fail("late execution"); },
  async retireTransport() { assert.fail("reset is not connection failure"); } };
  const running = transport({ client, observe: async () => assert.fail("no observe") });
  running.start(); const original = running.task; await barrier;
  running.reset(); client.connectionEpoch = 2;
  let exited = false; void original.then(() => { exited = true; });
  await new Promise(resolve => setImmediate(resolve)); assert(!exited);
  release(true); await original;
});

test("suspension retains observation sequence and cannot replay a request after execution becomes ready", async () => {
  let polls = 0; let observed = 0; let retired = 0; let ready = true; let reached; let release;
  const entered = new Promise(resolve => { reached = resolve; });
  const originalRequest = request();
  const client = { connectionEpoch: 1, async executionReady() { return ready; },
    async pollExtension() { if (++polls === 2) { ready = false; throw new ExecutionSuspended(); } return originalRequest; },
    async completeExtension() {}, async retireTransport() { retired++; } };
  const running = transport({ client, observe: async () => { observed++; return {}; }, wait: async () => {
    if (!ready) { reached(); await new Promise(resolve => { release = resolve; }); }
  } });
  running.start(); const original = running.task; await entered;
  assert.equal(polls, 2); assert.equal(observed, 1); assert.equal(retired, 0);
  let exited = false; void original.then(() => { exited = true; });
  await new Promise(resolve => setImmediate(resolve)); assert.equal(exited, false);
  ready = true; release(); await original;
  assert.equal(polls, 3); assert.equal(observed, 1); assert.equal(retired, 1);
});

test("reset joins a suspended original poll without launching late execution or retiring Pair", async () => {
  let ready = true; let polls = 0; let reached; let release;
  const entered = new Promise(resolve => { reached = resolve; });
  const client = { connectionEpoch: 1, async executionReady() { return ready; },
    async pollExtension() { polls++; ready = false; throw new ExecutionSuspended(); },
    async retireTransport() { assert.fail("suspension reset is not a connection failure"); } };
  const running = transport({ client, observe: async () => assert.fail("no observe"),
    wait: async () => { reached(); await new Promise(resolve => { release = resolve; }); } });
  running.start(); const original = running.task; await entered;
  running.reset(); client.connectionEpoch = 2; ready = true;
  release(); await original; assert.equal(polls, 1);
});

test("observation and action share one original readiness waiter without multiplying status requests", async () => {
  let calls = 0; let ready = false; let reached; let release;
  const entered = new Promise(resolve => { reached = resolve; });
  const client = { connectionEpoch: 1, async executionReady() { calls++; return ready; } };
  const signal = new AbortController().signal;
  const wait = async () => { reached(); await new Promise(resolve => { release = resolve; }); };
  const first = waitExecutionReady(client, 1, signal, wait);
  const second = waitExecutionReady(client, 1, signal, wait);
  assert.equal(first, second);
  await entered; assert.equal(calls, 1);
  ready = true; release(); assert.deepEqual(await Promise.all([first, second]), [true, true]);
  assert.equal(calls, 2);
  const next = waitExecutionReady(client, 1, signal, wait);
  assert.notEqual(next, first); assert.equal(await next, true);
});
