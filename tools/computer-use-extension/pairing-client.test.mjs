import { test } from "node:test";
import assert from "node:assert/strict";
import { createHmac, webcrypto } from "node:crypto";
import { PairingClient, createProof, ExecutionSuspended, EXTENSION_ID, normalizeCode, parseEndpoint } from "./pairing-client.mjs";

const endpoint = "http://127.0.0.1:32100";
const code = "12345-67890-ABCDE-F0123";
const challenge = { protocol: 1, nonce: "challenge", instance: "00000000-0000-4000-8000-000000000002", ext: EXTENSION_ID, expiresAt: 310000 };
const connection = "00000000-0000-4000-8000-000000000001";
function fixture() {
  const saved = {};
  const calls = [];
  const storage = {
    async setAccessLevel(level) { assert.equal(level.accessLevel, "TRUSTED_CONTEXTS"); },
    async get(key) { return { [key]: saved[key] }; },
    async set(value) { Object.assign(saved, value); },
    async remove(key) { delete saved[key]; },
  };
  const fetch = async (url, init) => {
    const path = new URL(url).pathname;
    const body = JSON.parse(init.body);
    calls.push({ path, body, init });
    assert.equal(init.credentials, "omit");
    assert.equal(init.redirect, "error");
    assert(!url.includes(code));
    assert(!init.body.includes(code));
    if (path === "/cu/pairing-challenge") return Response.json(challenge);
    if (path === "/cu/pairing-confirm") return Response.json({ sessionKey: "a".repeat(64),
      instanceId: challenge.instance, connectionNonce: body.connectionNonce, generation: 2 });
    if (path === "/cu/extension-observation/negotiate") return Response.json({ ok: true, protocol: 2, completionProtocol: 2, purpose: "observation" });
    return Response.json({ ok: true });
  };
  return { saved, storage, fetch, calls, options: { storage, fetch, crypto: webcrypto, now: () => 10000 } };
}

test("Host retirement is bound to the current pairing and exact original witness, including late replies", async () => {
  const f = fixture(); const client = new PairingClient(f.options);
  const lifetime = { instanceId: "00000000-0000-4000-8000-000000000009", platform: "windows", scope: "0", pid: 123, birth: "abc" };
  await assert.rejects(client.retiredHost(lifetime), /cancelled/);
  await client.pair(endpoint, code);
  try {
    for (const state of ["live", "unavailable", "retired"]) {
      client.fetch = async (_url, init) => {
        assert.equal(init.headers.authorization, "Bearer " + f.saved.cuPairing.sessionKey);
        const body = JSON.parse(init.body); assert.deepEqual(body.lifetime, lifetime);
        assert.equal(body.connection.instanceId, challenge.instance);
        return Response.json({ ok: true, state, lifetime });
      };
      assert.equal(await client.retiredHost(lifetime), state === "retired");
    }
    for (const reply of [{ ok: true, state: "retired", lifetime: { ...lifetime, birth: "def" } },
      { ok: true, state: "retired", lifetime, extra: true }, { ok: true, state: "absent", lifetime }]) {
      client.fetch = async () => Response.json(reply);
      await assert.rejects(client.retiredHost(lifetime), /hostLifetimeUnavailable/);
    }
    let release; let reached;
    const entered = new Promise(resolve => { reached = resolve; });
    client.fetch = async url => {
      if (!url.endsWith("/host-retirement")) return Response.json({ ok: true });
      reached(); await new Promise(resolve => { release = resolve; });
      return Response.json({ ok: true, state: "retired", lifetime });
    };
    const pending = client.retiredHost(lifetime); await entered;
    await client.forget(); release(); await assert.rejects(pending, /cancelled/);
  } finally { await client.forget(); }
});

test("only exact loopback origins and complete codes are accepted", () => {
  assert.equal(parseEndpoint(endpoint), endpoint);
  for (const bad of ["http://localhost:123", "https://127.0.0.1:123", "http://127.0.0.1:65536",
    "http://127.0.0.1:80", endpoint + "/secret", endpoint + "?secret=x", "http://127.0.0.1:1@evil.test"]) {
    assert.throws(() => parseEndpoint(bad));
  }
  assert.equal(normalizeCode(code.toLowerCase()), "1234567890ABCDEF0123");
  assert.throws(() => normalizeCode(code + "oops"));
});

test("WebCrypto proof uses the bound canonical message", async () => {
  const proof = await createProof(challenge, code, connection, webcrypto, 10000);
  const data = JSON.stringify(["grok-cu-pairing-v1", challenge.nonce, challenge.instance,
    EXTENSION_ID, challenge.expiresAt, connection]);
  assert.equal(proof.response, createHmac("sha256", normalizeCode(code)).update(data).digest("hex"));
  await assert.rejects(createProof({ ...challenge, ext: "other" }, code, connection, webcrypto, 10000));
  await assert.rejects(createProof(challenge, code, connection, webcrypto, 310000));
});

test("pair stores only short-lived session data and never returns the key", async () => {
  const f = fixture(); const client = new PairingClient(f.options);
  const result = await client.pair(endpoint, code);
  assert.deepEqual(result, { paired: true, endpoint });
  assert.equal(f.saved.cuPairing.sessionKey.length, 64);
  assert(!JSON.stringify(f.saved).includes(code));
  assert.deepEqual(await client.status(), { paired: true, endpoint });
  assert(f.calls.at(-1).init.headers.authorization.startsWith("Bearer "));
  await client.forget();
  assert.deepEqual(f.saved, {});
  assert.equal(f.calls.at(-1).path, "/cu/extension-disconnect");
});

test("readiness is authenticated metadata and false does not retire the pairing", async () => {
  const f = fixture(); const client = new PairingClient(f.options);
  await client.pair(endpoint, code); const epoch = client.connectionEpoch;
  try {
    for (const ready of [false, true, false]) {
      client.fetch = async (url, init) => url.endsWith("/cu/extension-status")
        ? Response.json({ ok: true, executionReady: ready }) : f.fetch(url, init);
      assert.equal(await client.executionReady(epoch), ready);
      assert.equal(client.connectionEpoch, epoch);
      assert(f.saved.cuPairing);
    }
    assert.equal(await client.executionReady(epoch + 1), false);
    client.fetch = async () => Response.json({ ok: true, executionReady: "true" });
    assert.equal(await client.executionReady(epoch), false);
    assert.equal(client.connectionEpoch, null); assert.deepEqual(f.saved, {});
  } finally { await client.forget(); }
});

test("a late old readiness response cannot make a replacement connection executable", async () => {
  const f = fixture(); const client = new PairingClient(f.options);
  await client.pair(endpoint, code); const old = client.connectionEpoch;
  let enter; let release; const reached = new Promise(resolve => { enter = resolve; });
  client.fetch = async (url, init) => {
    if (url.endsWith("/cu/extension-status")) {
      enter(); await new Promise(resolve => { release = resolve; });
      return Response.json({ ok: true, executionReady: true });
    }
    return f.fetch(url, init);
  };
  const pending = client.executionReady(old); await reached;
  await client.pair(endpoint, code); const current = client.connectionEpoch;
  release(); assert.equal(await pending, false);
  client.fetch = async (url, init) => url.endsWith("/cu/extension-status")
    ? Response.json({ ok: true, executionReady: false }) : f.fetch(url, init);
  assert.equal(await client.executionReady(current), false);
  assert.equal(client.connectionEpoch, current);
  await client.forget();
});

test("worker restart drops the previous session and requests Host revocation", async () => {
  const f = fixture();
  await new PairingClient(f.options).pair(endpoint, code);
  const restarted = new PairingClient(f.options);
  assert.deepEqual(await restarted.status(), { paired: false });
  assert.deepEqual(f.saved, {});
  assert.equal(f.calls.at(-1).path, "/cu/extension-disconnect");
});

test("browser registration binds the current lifecycle and optionally retires the previous one", async () => {
  const f = fixture(); const client = new PairingClient(f.options);
  await client.pair(endpoint, code);
  let registration;
  client.fetch = async (url, init) => {
    if (url.endsWith("/cu/browser-restart")) {
      registration = JSON.parse(init.body);
      return Response.json({
        ok: true,
        abandoned: 2,
        results: [
          { requestId: "00000000-0000-4000-8000-0000000000a1", result: "unknown" },
          { requestId: "00000000-0000-4000-8000-0000000000a2", result: "cleanup" },
        ],
      });
    }
    return f.fetch(url, init);
  };
  const current = "00000000-0000-4000-8000-000000000010";
  const previous = "00000000-0000-4000-8000-000000000011";
  assert.equal((await client.registerBrowser(current)).abandoned, 2);
  assert.equal(registration.connection.instanceId, challenge.instance);
  assert.equal(registration.connection.connectionNonce, f.saved.cuPairing.connectionNonce);
  assert.equal(registration.connection.generation, 2);
  assert.equal(registration.currentId, current);
  assert.equal(registration.previousId, null);
  assert.equal((await client.registerBrowser(current, previous)).abandoned, 2);
  assert.equal(registration.previousId, previous);
  await assert.rejects(client.registerBrowser(current, current), /browserSessionUnavailable/);
  await assert.rejects(client.registerBrowser("invalid"), /browserSessionUnavailable/);
  await client.forget();
});

test("revoked Host state clears extension storage", async () => {
  const f = fixture(); const client = new PairingClient(f.options);
  await client.pair(endpoint, code);
  client.fetch = async () => new Response("", { status: 403 });
  assert.deepEqual(await client.status(), { paired: false });
  assert.deepEqual(f.saved, {});
});

test("forget fences a late confirmation response and duplicate pair is rejected", async () => {
  const f = fixture(); let release; let reached;
  const barrier = new Promise(resolve => { reached = resolve; });
  const client = new PairingClient({ ...f.options, fetch: async (url, init) => {
    if (url.endsWith("/cu/pairing-confirm")) {
      reached(); await new Promise(resolve => { release = resolve; });
    }
    return f.fetch(url, init);
  } });
  const pending = client.pair(endpoint, code);
  await barrier;
  await assert.rejects(client.pair(endpoint, code), /busy/);
  await client.forget(); release();
  await assert.rejects(pending, /cancelled/);
  assert.deepEqual(f.saved, {});
  assert.deepEqual(await client.status(), { paired: false });
});

test("storage failure revokes an already minted session", async () => {
  const f = fixture(); f.storage.set = async () => { throw new Error("storage failed"); };
  await assert.rejects(new PairingClient(f.options).pair(endpoint, code), /storage failed/);
  assert.equal(f.calls.at(-1).path, "/cu/extension-disconnect");
});

test("storage removal failure cannot skip Host revocation on forget or restart", async () => {
  for (const restart of [false, true]) {
    const f = fixture(); const client = new PairingClient(f.options);
    await client.pair(endpoint, code);
    f.storage.remove = async () => { throw new Error("remove failed"); };
    await assert.rejects(restart ? new PairingClient(f.options).ready : client.forget(), /remove failed/);
    assert.equal(f.calls.at(-1).path, "/cu/extension-disconnect");
    if (!restart) assert.deepEqual(await client.status(), { paired: false });
  }
});

test("failed storage write and failed cleanup still revoke the newly issued key", async () => {
  const f = fixture();
  f.storage.set = async () => {
    f.storage.remove = async () => { throw new Error("remove failed"); };
    throw new Error("set failed");
  };
  await assert.rejects(new PairingClient(f.options).pair(endpoint, code));
  assert.equal(f.calls.at(-1).path, "/cu/extension-disconnect");
});

test("HTTP success with an invalid status payload is not a live pairing", async () => {
  const f = fixture(); const client = new PairingClient(f.options);
  await client.pair(endpoint, code);
  client.fetch = async (url, init) => url.endsWith("/cu/extension-status")
    ? Response.json({ ok: false }) : f.fetch(url, init);
  assert.deepEqual(await client.status(), { paired: false });
  assert.deepEqual(f.saved, {});
  assert.equal(f.calls.at(-1).path, "/cu/extension-disconnect");
});

test("oversized and cross-instance responses cannot publish a paired state", async () => {
  for (const response of [() => new Response("x".repeat(9000)), () => Response.json({ ...challenge, instance: null })]) {
    const f = fixture(); const client = new PairingClient({ ...f.options, fetch: async () => response() });
    await assert.rejects(client.pair(endpoint, code));
    assert.deepEqual(f.saved, {});
  }
});

test("pairing schedules a bounded heartbeat and unpair removes it", async () => {
  const f = fixture(); const scheduled = new Map(); let id = 0;
  const client = new PairingClient({ ...f.options,
    schedule: (fn, ms) => { scheduled.set(++id, { fn, ms }); return id; },
    cancel: key => scheduled.delete(key),
  });
  await client.pair(endpoint, code);
  assert.equal(scheduled.size, 1, "a paired worker must renew the Host lease");
  assert.equal([...scheduled.values()][0].ms, 10000);
  await client.forget();
  assert.equal(scheduled.size, 0);
});

function scheduledClient(options) {
  const tasks = new Map(); let next = 0;
  const client = new PairingClient({ ...options,
    schedule: (fn, ms) => { tasks.set(++next, { fn, ms }); return next; },
    cancel: id => tasks.delete(id),
  });
  return { client, tasks, async tick() {
    const [id, task] = tasks.entries().next().value;
    tasks.delete(id); await task.fn();
  } };
}

test("heartbeat renews once at a time and stops after network or protocol failure", async () => {
  for (const failure of [() => { throw new Error("offline"); }, () => Response.json({ ok: false })]) {
    const f = fixture(); const clock = scheduledClient(f.options);
    await clock.client.pair(endpoint, code);
    await clock.tick();
    assert.equal(f.calls.filter(call => call.path === "/cu/extension-status").length, 1);
    assert.equal(clock.tasks.size, 1);
    clock.client.fetch = async (url, init) => url.endsWith("/cu/extension-status") ? failure() : f.fetch(url, init);
    await clock.tick();
    assert.equal(clock.tasks.size, 0);
    assert.deepEqual(await clock.client.status(), { paired: false });
    assert.deepEqual(f.saved, {});
  }
});

test("status requests coalesce and a late heartbeat cannot erase a new connection", async () => {
  const f = fixture(); let release; let reached;
  const barrier = new Promise(resolve => { reached = resolve; });
  let statuses = 0;
  const clock = scheduledClient({ ...f.options, fetch: async (url, init) => {
    if (url.endsWith("/cu/extension-status") && ++statuses === 1) {
      reached(); await new Promise(resolve => { release = resolve; });
      return new Response("", { status: 403 });
    }
    return f.fetch(url, init);
  } });
  await clock.client.pair(endpoint, code);
  const timer = clock.tick(); await barrier;
  const status = clock.client.status();
  await Promise.resolve();
  assert.equal(statuses, 1, "UI poll and heartbeat must share one request");
  await clock.client.forget();
  await clock.client.pair(endpoint, code);
  const fresh = { ...f.saved.cuPairing };
  release(); await timer;
  assert.deepEqual(await status, { paired: true, endpoint });
  assert.deepEqual(f.saved.cuPairing, fresh);
  assert.equal(clock.tasks.size, 1, "old timer cannot reschedule itself");
  await clock.client.forget();
});

test("delayed retirement of A does not remove B's stored session", async () => {
  const f = fixture(); const clock = scheduledClient(f.options);
  await clock.client.pair(endpoint, code);
  let release; let reached;
  const barrier = new Promise(resolve => { reached = resolve; });
  const originalRemove = f.storage.remove;
  f.storage.remove = async key => {
    reached(); await new Promise(resolve => { release = resolve; });
    f.storage.remove = originalRemove; await originalRemove(key);
  };
  const retiring = clock.client.forget(); await barrier;
  const freshPairing = clock.client.pair(endpoint, code);
  release(); await retiring; await freshPairing;
  assert(f.saved.cuPairing, "new connection survives old cleanup");
  assert.deepEqual(await clock.client.status(), { paired: true, endpoint });
  await clock.client.forget();
});

test("scheduler failure cannot publish a session whose Host key was retired", async () => {
  const f = fixture();
  const client = new PairingClient({ ...f.options, schedule: () => { throw new TypeError("scheduler failed"); } });
  await assert.rejects(client.pair(endpoint, code), /scheduler failed/);
  assert.deepEqual(await client.status(), { paired: false });
  assert.deepEqual(f.saved, {});
  assert.equal(f.calls.at(-1).path, "/cu/extension-disconnect");
});

const sharedTab = { tabId: 101, title: "fixture", url: "https://fixture.invalid/", documentGeneration: 7, focused: true };
test("Share carries the exact document proof, rejects malformed proofs before HTTP and supports legacy omission", async () => {
  const f = fixture(); const client = new PairingClient(f.options);
  await client.pair(endpoint, code);
  try {
    for (const documentId of [null, "", "bad", "z".repeat(32), 12]) {
      const before = f.calls.length;
      await assert.rejects(client.shareCurrentTab({ ...sharedTab, documentId }), /shareUnavailable/);
      assert.equal(f.calls.length, before, "invalid proof must send no request");
    }
    const documentId = "0123456789abcdef0123456789abcdef";
    await client.shareCurrentTab({ ...sharedTab, documentId });
    assert.equal(f.calls.at(-1).path, "/cu/tab-offer");
    assert.equal(f.calls.at(-1).body.documentId, documentId);
    await client.shareCurrentTab(sharedTab);
    assert(!Object.hasOwn(f.calls.at(-1).body, "documentId"));
  } finally { await client.forget(); }
});
test("share requires a real document generation and unshare binds it with an increasing sequence", async () => {
  const f = fixture(); const client = new PairingClient(f.options);
  await client.pair(endpoint, code);
  await assert.rejects(client.shareCurrentTab({ ...sharedTab, documentGeneration: undefined }), /shareUnavailable/);
  await assert.rejects(client.shareCurrentTab({ ...sharedTab, url: "https://user:secret@fixture.invalid/" }), /shareUnavailable/);
  assert(!f.calls.some(call => call.path === "/cu/tab-offer"));
  await client.shareCurrentTab(sharedTab);
  assert.equal(f.calls.at(-1).body.sequence, 1);
  assert.equal(f.calls.at(-1).body.documentGeneration, 7);
  assert.equal(f.calls.at(-1).body.profileId, f.saved.cuPairing.connectionNonce);
  await client.unshareTab(101, 7);
  assert.equal(f.calls.at(-1).body.sequence, 2);
  assert.equal(f.calls.at(-1).body.documentGeneration, 7);
  await client.forget();
});

test("failed unshare retires the pairing so a grant cannot remain renewed", async () => {
  const f = fixture(); const clock = scheduledClient(f.options);
  await clock.client.pair(endpoint, code);
  clock.client.fetch = async (url, init) => url.endsWith("/cu/tab-unoffer")
    ? new Response("", { status: 403 }) : f.fetch(url, init);
  await assert.rejects(clock.client.unshareTab(101, 7));
  assert.deepEqual(await clock.client.status(), { paired: false });
  assert.deepEqual(f.saved, {});
  assert.equal(clock.tasks.size, 0);
  assert.equal(f.calls.at(-1).path, "/cu/extension-disconnect");
});

test("late share cannot report success or retire a replacement pairing", async () => {
  const f = fixture(); let reached; let release;
  const barrier = new Promise(resolve => { reached = resolve; });
  const client = new PairingClient({ ...f.options, fetch: async (url, init) => {
    if (url.endsWith("/cu/tab-offer")) { reached(); await new Promise(resolve => { release = resolve; }); }
    return f.fetch(url, init);
  } });
  await client.pair(endpoint, code);
  const sharing = client.shareCurrentTab(sharedTab);
  await barrier; await client.forget(); await client.pair(endpoint, code);
  const fresh = { ...f.saved.cuPairing };
  release(); await assert.rejects(sharing, /cancelled/);
  assert.deepEqual(f.saved.cuPairing, fresh);
  assert.deepEqual(await client.status(), { paired: true, endpoint });
  await client.forget();
});

test("a stale share or unshare intent cannot acquire a replacement connection", async () => {
  const f = fixture(); const client = new PairingClient(f.options);
  await client.pair(endpoint, code);
  const oldEpoch = client.connectionEpoch;
  await client.forget(); await client.pair(endpoint, code);
  const before = f.calls.length;
  await assert.rejects(client.shareCurrentTab(sharedTab, oldEpoch), /cancelled/);
  await assert.rejects(client.unshareTab(101, 7, oldEpoch), /cancelled/);
  assert.equal(f.calls.length, before, "stale intents must send no HTTP requests");
  assert.deepEqual(await client.status(), { paired: true, endpoint });
  await client.forget();
});

test("connection replacement while waiting for ready cannot redirect share or unshare", async () => {
  for (const operation of ["share", "unshare"]) {
    const f = fixture(); const client = new PairingClient(f.options);
    await client.pair(endpoint, code);
    const ready = client.ready; let release;
    client.ready = new Promise(resolve => { release = resolve; });
    const work = operation === "share" ? client.shareCurrentTab(sharedTab) : client.unshareTab(101, 7);
    client.ready = ready;
    await client.forget(); await client.pair(endpoint, code);
    const before = f.calls.length;
    release(); await assert.rejects(work, /cancelled/);
    assert.equal(f.calls.length, before);
    await client.forget();
  }
});

test("only forbidden execution polls with exact authenticated disabled status suspend the original Pair", async () => {
  for (const operation of ["observation", "action", "negotiate", "observation-negotiate"]) {
    const f = fixture(); const client = new PairingClient(f.options);
    await client.pair(endpoint, code);
    const epoch = client.connectionEpoch;
    const signal = new AbortController().signal;
    const route = operation === "observation" ? "/cu/extension-poll"
      : operation === "action" ? "/cu/extension-actions/poll"
        : operation === "observation-negotiate" ? "/cu/extension-observation/negotiate" : "/cu/extension-actions/negotiate";
    let statuses = 0;
    client.fetch = async (url, init) => {
      if (url.endsWith("/cu/extension-actions/negotiate") && operation === "action")
        return Response.json({ ok: true, protocol: 2, completionProtocol: 2 });
      if (url.endsWith(route)) return new Response(null, { status: 403 });
      if (url.endsWith("/cu/extension-status")) {
        statuses++;
        assert.equal(init.headers.authorization, "Bearer " + f.saved.cuPairing.sessionKey);
        const saved = f.saved.cuPairing;
        assert.deepEqual(JSON.parse(init.body), { instanceId: saved.instanceId,
          connectionNonce: saved.connectionNonce, generation: saved.generation });
        return Response.json({ ok: true, executionReady: false });
      }
      return f.fetch(url, init);
    };
    try {
      if (operation === "action") await client.negotiateActions(epoch, signal);
      if (operation === "observation") await client.negotiateObservations(epoch, signal);
      const work = operation === "observation" ? client.pollExtension(epoch, signal)
        : operation === "action" ? client.pollAction(epoch, signal)
          : operation === "observation-negotiate" ? client.negotiateObservations(epoch, signal) : client.negotiateActions(epoch, signal);
      await assert.rejects(work, ExecutionSuspended);
      assert.equal(statuses, 1);
      assert.equal(client.connectionEpoch, epoch);
      assert(f.saved.cuPairing);
      assert(!f.calls.some(call => call.path === "/cu/extension-disconnect"));
    } finally { await client.forget(); }
  }
});

test("forbidden polling with enabled, invalid or lost status and other failures cannot suspend", async () => {
  for (const failure of ["enabled", "invalid", "expired", "network", "rate", "protocol"]) {
    const f = fixture(); const client = new PairingClient(f.options);
    await client.pair(endpoint, code);
    const epoch = client.connectionEpoch;
    let statuses = 0;
    client.fetch = async (url, init) => {
      if (url.endsWith("/cu/extension-poll")) {
        if (failure === "network") throw new Error("offline");
        if (failure === "protocol") return Response.json({ ok: false });
        return new Response(null, { status: failure === "rate" ? 429 : 403 });
      }
      if (url.endsWith("/cu/extension-status")) {
        statuses++;
        if (failure === "expired") return new Response(null, { status: 403 });
        return Response.json({ ok: true, executionReady: failure === "invalid" ? "false" : true });
      }
      return f.fetch(url, init);
    };
    try {
      await client.negotiateObservations(epoch, new AbortController().signal);
      await assert.rejects(client.pollExtension(epoch, new AbortController().signal), error =>
        error instanceof Error && !(error instanceof ExecutionSuspended));
      assert.equal(statuses, ["enabled", "invalid", "expired"].includes(failure) ? 1 : 0);
      if (["invalid", "expired"].includes(failure)) assert.equal(client.connectionEpoch, null);
    } finally { await client.forget(); }
  }
});

test("cancellation or replacement during authenticated suspension check cannot follow the new Pair", async () => {
  for (const replacement of [false, true]) {
    const f = fixture(); const client = new PairingClient(f.options);
    await client.pair(endpoint, code);
    const epoch = client.connectionEpoch;
    const controller = new AbortController();
    let reached; let release;
    const entered = new Promise(resolve => { reached = resolve; });
    client.fetch = async (url, init) => {
      if (url.endsWith("/cu/extension-poll")) return new Response(null, { status: 403 });
      if (url.endsWith("/cu/extension-status")) {
        reached(); await new Promise(resolve => { release = resolve; });
        return Response.json({ ok: true, executionReady: false });
      }
      return f.fetch(url, init);
    };
    await client.negotiateObservations(epoch, controller.signal);
    const work = client.pollExtension(epoch, controller.signal);
    const rejected = assert.rejects(work, error => error instanceof Error && !(error instanceof ExecutionSuspended));
    await entered;
    if (replacement) await client.pair(endpoint, code); else controller.abort();
    const current = client.connectionEpoch;
    release(); await rejected;
    assert.equal(client.connectionEpoch, current);
    if (replacement) assert.notEqual(current, epoch);
    await client.forget();
  }
});

test("transport rejects cross-connection requests and stale result writes without exposing keys", async () => {
  const f = fixture(); const client = new PairingClient(f.options);
  await client.pair(endpoint, code);
  const epoch = client.connectionEpoch;
  const signal = new AbortController().signal;
  client.fetch = async (url, init) => url.endsWith("/cu/extension-poll")
    ? Response.json({ ok: true, request: { connection: { instanceId: "other", connectionNonce: connection, generation: 2 } } })
    : f.fetch(url, init);
  await client.negotiateObservations(epoch, signal);
  await assert.rejects(client.pollExtension(epoch, signal), /connection/);
  const requestIdentity = { instanceId: f.saved.cuPairing.instanceId, connectionNonce: f.saved.cuPairing.connectionNonce,
    generation: f.saved.cuPairing.generation };
  await client.forget(); await client.pair(endpoint, code);
  const before = f.calls.length;
  await assert.rejects(client.completeExtension(epoch, { request: { connection: requestIdentity } }, signal), /cancelled/);
  assert.equal(f.calls.length, before);
  await client.retireTransport(epoch);
  assert(client.connectionEpoch !== null, "old transport failure must not revoke a new pairing");
  await client.forget();
});

function completionProof(session) {
  return { binding: { protocol: 2, requestId: webcrypto.randomUUID(),
    connection: { instanceId: session.instanceId, connectionNonce: session.connectionNonce, generation: session.generation },
    session: "owner", runId: "run", tabId: "101", documentId: "0".repeat(32), documentGeneration: 7,
    grantGeneration: 1, snapshotId: webcrypto.randomUUID() }, completionKey: "b".repeat(64) };
}

test("observation negotiation rejects legacy or incomplete capabilities and cannot grant polling or claim", async () => {
  const f = fixture(); const client = new PairingClient(f.options); await client.pair(endpoint, code);
  const epoch = client.connectionEpoch; const signal = new AbortController().signal;
  const completion = { purpose: "observation", proof: completionProof(f.saved.cuPairing) };
  try {
    const before = f.calls.length;
    await assert.rejects(client.pollExtension(epoch, signal), /invalidObservationScope/);
    await assert.rejects(client.claimObservation(epoch, completion, signal), /invalidObservationScope/);
    assert.equal(f.calls.length, before);
    for (const reply of [{ ok: true, protocol: 1, completionProtocol: 2, purpose: "observation" },
      { ok: true, protocol: 2, completionProtocol: 2 }, { ok: true, protocol: 2, completionProtocol: 2, purpose: "action" },
      { ok: true, protocol: 2, completionProtocol: 2, purpose: "observation", extra: true }]) {
      client.fetch = async () => Response.json(reply);
      await assert.rejects(client.negotiateObservations(epoch, signal), /invalidObservationScope/);
      await assert.rejects(client.pollExtension(epoch, signal), /invalidObservationScope/);
      await assert.rejects(client.claimObservation(epoch, completion, signal), /invalidObservationScope/);
    }
  } finally { await client.forget(); }
});

test("observation claims use exactly the negotiated Pair and acknowledgements; cleanup carries no Pair Bearer", async () => {
  const f = fixture(); const client = new PairingClient(f.options); await client.pair(endpoint, code);
  const epoch = client.connectionEpoch; const signal = new AbortController().signal;
  const completion = { purpose: "observation", proof: completionProof(f.saved.cuPairing) };
  try {
    await client.negotiateObservations(epoch, signal);
    const negotiation = f.calls.at(-1);
    assert.deepEqual(negotiation.body, { protocol: 2, completionProtocol: 2, purpose: "observation", connection: completion.proof.binding.connection });
    const scope = client.observationScope(epoch, completion);
    const wrong = structuredClone(completion); wrong.proof.binding.connection.generation++;
    const before = f.calls.length;
    assert.throws(() => client.observationScope(epoch, wrong), /cancelled/);
    await assert.rejects(client.claimObservation(epoch, wrong, signal), /cancelled/); assert.equal(f.calls.length, before);
    await client.claimObservation(epoch, completion, signal);
    assert.deepEqual(f.calls.at(-1).body, completion); assert.equal(f.calls.at(-1).init.headers.authorization, "Bearer " + f.saved.cuPairing.sessionKey);
    client.fetch = async (url, init) => url.endsWith("/claim") ? Response.json({ ok: true, extra: true }) : f.fetch(url, init);
    await assert.rejects(client.claimObservation(epoch, completion, signal), /invalidObservationScope/);
    await client.forget(); await client.pair(endpoint, code);
    const replacement = structuredClone(f.saved.cuPairing);
    await assert.rejects(client.claimObservation(client.connectionEpoch, completion, signal), /invalidObservationScope/);
    await scope.settle(); const cleanup = f.calls.at(-1);
    assert.equal(cleanup.path, "/cu/extension-observation/settle"); assert.deepEqual(cleanup.body, completion);
    assert.equal(cleanup.init.headers.authorization, undefined); assert.deepEqual(f.saved.cuPairing, replacement);
  } finally { await client.forget(); }
});

test("late observation negotiation cannot authorize a replacement Pair", async () => {
  const f = fixture(); const client = new PairingClient(f.options); await client.pair(endpoint, code);
  let enter; let release; const reached = new Promise(resolve => { enter = resolve; });
  const epoch = client.connectionEpoch; const signal = new AbortController().signal;
  client.fetch = async (url, init) => {
    if (url.endsWith("/cu/extension-observation/negotiate")) { enter(); await new Promise(resolve => { release = resolve; }); }
    return f.fetch(url, init);
  };
  const work = client.negotiateObservations(epoch, signal); const rejected = assert.rejects(work, /cancelled/);
  try {
    await reached; await client.forget(); await client.pair(endpoint, code); release(); await rejected;
    await assert.rejects(client.pollExtension(client.connectionEpoch, signal), /invalidObservationScope/);
    assert.notEqual(client.connectionEpoch, epoch);
  } finally { release?.(); await rejected; await client.forget(); }
});

test("late claim cannot survive replacement pairing but its captured cleanup scope can settle", async () => {
  for (const lost of [false, true]) {
    const f = fixture(); let entered; let release;
    const reached = new Promise(resolve => { entered = resolve; });
    const client = new PairingClient({ ...f.options, fetch: async (url, init) => {
      if (url.endsWith("/claim")) { entered(); await new Promise(resolve => { release = resolve; }); if (lost) throw new Error("lost reply"); }
      return f.fetch(url, init);
    } });
    await client.pair(endpoint, code);
    const epoch = client.connectionEpoch; const proof = completionProof(f.saved.cuPairing);
    const scope = client.completionScope(epoch, proof);
    const claiming = client.claimCompletion(epoch, proof, new AbortController().signal);
    const rejected = assert.rejects(claiming);
    await reached; await client.forget(); await client.pair(endpoint, code);
    const replacement = { ...f.saved.cuPairing }; release(); await rejected;
    assert.throws(() => client.completionScope(epoch, proof), /cancelled/);
    await scope.settle();
    const cleanup = f.calls.at(-1);
    assert.equal(cleanup.path, "/cu/extension-completion/settle");
    assert.deepEqual(cleanup.body, proof); assert.equal(cleanup.init.headers.authorization, undefined);
    assert.deepEqual(f.saved.cuPairing, replacement);
    await client.forget();
  }
});

test("completion claims require the exact current epoch and a strict acknowledgement", async () => {
  const f = fixture(); const client = new PairingClient(f.options);
  await client.pair(endpoint, code);
  const epoch = client.connectionEpoch; const proof = completionProof(f.saved.cuPairing);
  const altered = structuredClone(proof); altered.binding.connection.generation++;
  const before = f.calls.length;
  assert.throws(() => client.completionScope(epoch, altered), /cancelled/);
  await assert.rejects(client.claimCompletion(epoch, altered, new AbortController().signal), /cancelled/);
  assert.equal(f.calls.length, before);
  client.fetch = async (url, init) => url.endsWith("/claim") ? Response.json({ ok: true, extra: "secret" }) : f.fetch(url, init);
  await assert.rejects(client.claimCompletion(epoch, proof, new AbortController().signal), /invalidCompletion/);
  await client.forget();
});
