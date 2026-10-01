// Probe regression only: real MCP stdio/HTTP, simulated Host and page effects.
// Full Broker/MV3/browser evidence requires the live extension-pairing gate.
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { test } from "node:test";
import { observeWithAdmissionBackoff, sessionClient, verifyMcpClickOnce } from "./existing-mcp-live.mjs";

const args = {
  version: 1, actionId: "fixture-mcp-click", runId: "fixture-mcp-run",
  targetId: "tab:1", targetGeneration: 1,
  snapshotId: "00000000-0000-4000-8000-000000000001", geometryRevision: 1,
  action: "click", target: { elementRef: "fixture-ref" }, parameters: {},
};
const applied = { kind: "applied", executed: true };
const toolReply = (value, isError = false) => ({
  isError, content: [{ type: "text", text: JSON.stringify(value) }],
});

async function setup(t, mode = "once") {
  const state = { effects: 0, actions: [], observations: 0, passed: [], stages: [] };
  const binding = { token: "fixture-only-token", session: "fixture-only-session" };
  const server = createServer(async (request, response) => {
    const chunks = [];
    for await (const chunk of request) chunks.push(chunk);
    const body = JSON.parse(Buffer.concat(chunks).toString("utf8"));
    let reply = toolReply({});
    if (body.name === "computer_act") {
      state.actions.push({
        method: request.method, path: request.url,
        authorization: request.headers.authorization, body,
      });
      if (mode === "error") reply = toolReply({ kind: "rejected", executed: false }, true);
      else {
        if (mode === "twice" || (mode === "once" && state.actions.length === 1)) state.effects += 1;
        reply = toolReply(applied);
      }
    } else if (body.name === "computer_observe") {
      state.observations += 1;
      if (mode.startsWith("backpressure") && (state.observations === 1 || mode === "backpressure-always")) {
        reply = { ...toolReply("rate limited", true), code: "rate_limited",
          completion: mode === "backpressure-unknown" ? "unknown" : "not_started",
          retryAfterMs: mode === "backpressure-unbounded" ? 30000 : 1 };
      }
    }
    response.writeHead(200, { "content-type": "application/json" });
    response.end(JSON.stringify(reply));
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const client = sessionClient({ ...binding, endpoint: `http://127.0.0.1:${server.address().port}` });
  t.after(async () => {
    await client.close();
    server.closeAllConnections();
    await new Promise(resolve => server.close(resolve));
  });
  const verify = () => verifyMcpClickOnce({
    client, args,
    effectCount: async () => state.effects,
    stage: name => state.stages.push(name),
    passed: name => state.passed.push(name),
  });
  return { binding, client, state, verify };
}

test("live action probe sends click and dedupe through actual MCP stdio and loopback HTTP", async t => {
  const { binding, state, verify } = await setup(t);
  await verify();
  assert.equal(state.effects, 1);
  assert.equal(state.actions.length, 2, "retrieval must also reach the production MCP child");
  for (const request of state.actions) {
    assert.equal(request.method, "POST");
    assert.equal(request.path, "/cu/tool");
    assert.equal(request.authorization, `Bearer ${binding.token}`);
    assert.deepEqual(request.body, { session: binding.session, name: "computer_act", arguments: args });
  }
  assert.deepEqual(state.passed, ["mcp-act-through-existing-adapter", "mcp-act-action-id-dedupe"]);
});

test("live action probe does not accept a success envelope without an independent effect", async t => {
  const { state, verify } = await setup(t, "no-effect");
  await assert.rejects(verify(), /must change the owned page once/);
  assert.equal(state.actions.length, 1);
  assert.deepEqual(state.passed, []);
});

test("live action probe rejects a duplicate that returns the same result but acts twice", async t => {
  const { state, verify } = await setup(t, "twice");
  await assert.rejects(verify(), /must not click again/);
  assert.equal(state.effects, 2);
  assert.equal(state.actions.length, 2);
  assert.deepEqual(state.passed, ["mcp-act-through-existing-adapter"]);
});

test("live action probe propagates transport tool failure without private-helper fallback", async t => {
  const { state, verify } = await setup(t, "error");
  await assert.rejects(verify(), /MCP tool must succeed/);
  assert.equal(state.actions.length, 1);
  assert.equal(state.effects, 0);
  assert.deepEqual(state.passed, []);
});

test("cancellation probe follows explicit not-started observation backpressure once", async t => {
  const { client, state } = await setup(t, "backpressure");
  assert.equal((await observeWithAdmissionBackoff(client)).isError, false);
  assert.equal(state.observations, 2);
  assert.equal(state.actions.length, 0);
});

test("cancellation probe does not retry unknown or unbounded responses", async t => {
  for (const mode of ["backpressure-unknown", "backpressure-unbounded"]) {
    await t.test(mode, async t => {
      const { client, state } = await setup(t, mode);
      assert.equal((await observeWithAdmissionBackoff(client)).isError, true);
      assert.equal(state.observations, 1);
    });
  }
});

test("cancellation probe bounds repeated rate rejection to one read-only retry", async t => {
  const { client, state } = await setup(t, "backpressure-always");
  assert.equal((await observeWithAdmissionBackoff(client)).isError, true);
  assert.equal(state.observations, 2);
});
