import { test } from "node:test";
import assert from "node:assert/strict";
import { ExtensionTransport, validRequest } from "./extension-transport.mjs";
const id = "00000000-0000-4000-8000-000000000001";
function request() {
  return { protocol: 1, requestId: id, sequence: 1, deadlineMs: Date.now() + 9000,
    connection: { instanceId: id, connectionNonce: id, generation: 1 }, session: "owner", runId: "run",
    tabId: "101", documentGeneration: 7, grantGeneration: 1, command: { kind: "observe", snapshotId: id, screenshot: false, preview: false } };
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
  const client = { connectionEpoch: 1, async pollExtension() { return r; },
    async completeExtension() { submitted++; }, async retireTransport(epoch) { assert.equal(epoch, 1); retired++; } };
  const transport = new ExtensionTransport({ client, observe: async () => { observed++; return {}; }, wait: async () => {} });
  transport.start(); await transport.task;
  assert.equal(observed, 1); assert.equal(submitted, 1); assert.equal(retired, 1);
});

test("late observation after connection retirement sends no result", async () => {
  let release; let reached;
  const barrier = new Promise(resolve => { reached = resolve; }); let submitted = 0;
  const client = { connectionEpoch: 1, async pollExtension() { return request(); },
    async completeExtension() { submitted++; }, async retireTransport() { assert.fail("intentional reset is not a failure"); } };
  const transport = new ExtensionTransport({ client, observe: async () => { reached(); await new Promise(resolve => { release = resolve; }); return {}; } });
  transport.start(); await barrier;
  client.connectionEpoch = 2; transport.reset(); release(); await transport.task;
  assert.equal(submitted, 0);
});

test("ambiguous result is never resent and an observation failure sends only a typed rejection", async () => {
  let polls = 0; const submitted = [];
  const client = { connectionEpoch: 1, async pollExtension() { if (++polls > 1) throw new Error("offline"); return request(); },
    async completeExtension(_epoch, result) { submitted.push(result); throw new Error("timeout"); }, async retireTransport() {} };
  const transport = new ExtensionTransport({ client, observe: async () => { throw new Error("private page details"); }, wait: async () => {} });
  transport.start(); await transport.task;
  assert.equal(submitted.length, 1);
  assert.deepEqual(submitted[0].outcome, { kind: "rejected", reason: "documentChanged" });
  assert(!JSON.stringify(submitted).includes("private page details"));
});
