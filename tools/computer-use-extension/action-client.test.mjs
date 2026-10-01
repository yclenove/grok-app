import { test } from "node:test";
import assert from "node:assert/strict";
import { webcrypto, randomUUID } from "node:crypto";
import { PairingClient, EXTENSION_ID } from "./pairing-client.mjs";

async function fixture() {
  const stored = {}; const calls = []; const handlers = new Map();
  const instance = randomUUID();
  const client = new PairingClient({ crypto: webcrypto, storage: {
    async setAccessLevel() {}, async get(key) { return { [key]: stored[key] }; },
    async set(value) { Object.assign(stored, value); }, async remove(key) { delete stored[key]; },
  }, fetch: async (url, init) => {
    const route = new URL(url).pathname; const body = JSON.parse(init.body);
    calls.push({ route, body, init });
    if (handlers.has(route)) return handlers.get(route)(body, init);
    if (route === "/cu/pairing-challenge") return Response.json({ protocol: 1, nonce: randomUUID(), instance,
      ext: EXTENSION_ID, expiresAt: Date.now() + 300000 });
    if (route === "/cu/pairing-confirm") return Response.json({ instanceId: instance,
      connectionNonce: body.connectionNonce, generation: 1, sessionKey: "a".repeat(64) });
    if (route === "/cu/extension-actions/negotiate") return Response.json({ ok: true, protocol: 2, completionProtocol: 2 });
    return Response.json({ ok: true });
  } });
  await client.pair("http://127.0.0.1:32100", "1234567890abcdef0123");
  const signal = new AbortController().signal;
  const packet = (text = "你好🙂") => {
    const { instanceId, connectionNonce, generation } = stored.cuPairing;
    const binding = { protocol: 2, requestId: randomUUID(), connection: { instanceId, connectionNonce, generation },
      session: "owner", runId: "run", tabId: "101", documentId: "0".repeat(32), documentGeneration: 7,
      grantGeneration: 2, snapshotId: randomUUID() };
    const { snapshotId, ...fields } = binding;
    return { proof: { binding, completionKey: "b".repeat(64) }, request: { ...fields, sequence: 1,
      deadlineMs: Date.now() + 10000, command: { kind: "act", snapshotId, elementRef: snapshotId + "-1",
        action: "set_value", parameters: { text } } } };
  };
  return { client, stored, calls, handlers, packet, signal,
    negotiate: () => client.negotiateActions(client.connectionEpoch, signal) };
}

test("v2 requires exact negotiation and submits full immutable command to the bound claim route", async () => {
  const f = await fixture();
  try {
    const epoch = f.client.connectionEpoch; const dispatch = f.packet();
    await assert.rejects(f.client.pollAction(epoch, f.signal), /actionUnavailable/);
    await assert.rejects(f.client.claimAction(epoch, dispatch, f.signal), /actionUnavailable/);
    assert(!f.calls.some(call => call.route.includes("/extension-actions/")));
    await f.negotiate();
    const handshake = f.calls.at(-1).body;
    assert.equal(handshake.protocol, 2); assert.equal(handshake.completionProtocol, 2);
    assert.deepEqual(handshake.actions, ["click", "set_value", "type_text", "scroll", "wait"]);
    const claiming = f.client.claimAction(epoch, dispatch, f.signal);
    dispatch.request.command.parameters.text = "changed after invocation";
    await claiming;
    const call = f.calls.at(-1);
    assert.equal(call.route, "/cu/extension-actions/claim");
    assert.equal(call.body.request.command.parameters.text, "你好🙂");
    assert.equal(call.body.proof.binding.requestId, call.body.request.requestId);
    assert(!f.calls.some(call => call.route === "/cu/extension-completion/claim"));
    assert.equal(call.init.credentials, "omit"); assert.equal(call.init.redirect, "error");
  } finally { await f.client.forget(); }
});

test("legacy, extra-field, wrong-version and failed negotiation never allow action polling", async () => {
  for (const reply of [{ ok: true }, { ok: true, protocol: 1, completionProtocol: 2 },
    { ok: true, protocol: 2, completionProtocol: 1 }, { ok: true, protocol: 2, completionProtocol: 2, extra: true }]) {
    const f = await fixture();
    try {
      f.handlers.set("/cu/extension-actions/negotiate", () => Response.json(reply));
      await assert.rejects(f.negotiate(), /actionUnavailable/);
      await assert.rejects(f.client.pollAction(f.client.connectionEpoch, f.signal), /actionUnavailable/);
      assert(!f.calls.some(call => call.route === "/cu/extension-actions/poll"));
    } finally { await f.client.forget(); }
  }
});

test("only action polls admit larger Unicode packets and every response is still bounded", async () => {
  const f = await fixture();
  try {
    await f.negotiate(); const epoch = f.client.connectionEpoch;
    f.handlers.set("/cu/extension-actions/poll", () => Response.json({ ok: true, dispatch: f.packet("🙂".repeat(4000)) }));
    const packet = await f.client.pollAction(epoch, f.signal);
    assert.equal([...packet.request.command.parameters.text].length, 4000);
    assert(Object.isFrozen(packet.request.command.parameters));
    f.handlers.set("/cu/extension-poll", () => Response.json({ ok: true, request: "x".repeat(9000) }));
    await assert.rejects(f.client.pollExtension(epoch, f.signal));
    for (const response of [() => new Response("x".repeat(65537)),
      () => new Response(new Uint8Array([0xff, 0xfe])), () => Response.json({ ok: true, dispatch: null, extra: true })]) {
      f.handlers.set("/cu/extension-actions/poll", response);
      await assert.rejects(f.client.pollAction(epoch, f.signal), /actionUnavailable/);
    }
  } finally { await f.client.forget(); }
});

test("malformed commands, lone surrogates and wrong full connection are rejected before claim", async () => {
  const f = await fixture();
  try {
    await f.negotiate(); const epoch = f.client.connectionEpoch;
    for (const mutate of [p => { p.request.command.parameters.text = "\ud800"; },
      p => { p.request.command.parameters = null; }, p => { p.request.command.extra = true; },
      p => { p.request.command.elementRef = p.request.command.snapshotId + "-"; },
      p => { p.request.command.parameters.extra = true; }, p => { p.extra = true; },
      p => { p.request.connection.generation++; },
      p => { p.request.connection = { ...p.request.connection, instanceId: randomUUID() };
        p.proof.binding.connection = { ...p.request.connection }; }]) {
      const packet = f.packet(); mutate(packet);
      await assert.rejects(f.client.claimAction(epoch, packet, f.signal), /actionUnavailable/);
    }
    assert(!f.calls.some(call => call.route.endsWith("/claim")));
  } finally { await f.client.forget(); }
});

test("late claim from a replaced pairing fails and neither claim nor result is retried", async () => {
  const f = await fixture(); let release; let reached;
  try {
    await f.negotiate(); const epoch = f.client.connectionEpoch;
    const entered = new Promise(resolve => { reached = resolve; });
    f.handlers.set("/cu/extension-actions/claim", async () => { reached(); await new Promise(resolve => { release = resolve; }); return Response.json({ ok: true }); });
    const claiming = f.client.claimAction(epoch, f.packet(), f.signal);
    const denied = assert.rejects(claiming, /actionUnavailable/);
    await entered; await f.client.forget(); release(); await denied;
    assert.equal(f.calls.filter(call => call.route.endsWith("/claim")).length, 1);
    await f.client.pair("http://127.0.0.1:32100", "1234567890abcdef0123"); await f.negotiate();
    f.handlers.set("/cu/extension-actions/result", () => { throw new Error("private payload must not escape"); });
    await assert.rejects(f.client.completeAction(f.client.connectionEpoch,
      { ...f.packet(), outcome: { status: "applied", detail: "clicked" } }, f.signal), { message: "actionUnavailable" });
    assert.equal(f.calls.filter(call => call.route.endsWith("/result")).length, 1);
  } finally { release?.(); await f.client.forget(); }
});
