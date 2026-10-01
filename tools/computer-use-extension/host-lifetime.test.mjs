import { test } from "node:test";
import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { HostLifetimeClient, copyHostLifetime } from "./host-lifetime.mjs";

const instanceId = randomUUID();
const lifetime = { instanceId, platform: "windows", scope: "0", pid: 1234, birth: "0123456789abcdef" };
const proof = { binding: { protocol: 2, requestId: randomUUID(), connection: { instanceId, connectionNonce: randomUUID(), generation: 1 },
  session: "owner", runId: "run", tabId: "101", documentId: "0".repeat(32), documentGeneration: 1, grantGeneration: 1,
  snapshotId: randomUUID() }, completionKey: "a".repeat(64) };

test("capture uses original proof without pairing credentials and binds its exact instance", async () => {
  let body;
  const client = new HostLifetimeClient({ client: {}, fetch: async (url, init) => {
    assert.equal(url, "http://127.0.0.1:32100/cu/extension-completion/host-lifetime");
    assert.deepEqual(init.headers, { "content-type": "application/json" }); assert.equal(init.credentials, "omit");
    assert.equal(init.redirect, "error"); body = JSON.parse(init.body); return Response.json({ ok: true, lifetime });
  } });
  const captured = await client.capture("http://127.0.0.1:32100", proof);
  assert.deepEqual(captured, lifetime); assert(Object.isFrozen(captured)); assert.deepEqual(body, proof);
});

test("invalid, oversized, wrong-instance and unauthenticated capture cannot provide a witness", async () => {
  for (const response of [() => new Response("", { status: 403 }), () => new Response("x".repeat(1025)),
    () => Response.json({ ok: true, lifetime, extra: true }), () => Response.json({ ok: true, lifetime: { ...lifetime, instanceId: randomUUID() } }),
    () => Response.json({ ok: true, lifetime: { ...lifetime, pid: 0 } }), () => new Response(new Uint8Array([0xff]))]) {
    const client = new HostLifetimeClient({ client: {}, fetch: async () => response() });
    await assert.rejects(client.capture("http://127.0.0.1:32100", proof), /hostLifetimeUnavailable/);
  }
  for (const invalid of [{ ...lifetime, platform: "other" }, { ...lifetime, pid: 2147483648 },
    { ...lifetime, birth: "" }, { ...lifetime, scope: "../" }, { ...lifetime, extra: true }]) {
    assert.throws(() => copyHostLifetime(invalid), /hostLifetimeUnavailable/);
  }
});
