import { test } from "node:test";
import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { ObservationCompletions, copyBoundObservation, copyObservationProof } from "./observation-completions.mjs";
import { ObservationJournal } from "./observation-journal.mjs";
import { CompletionJournal } from "./completion-journal.mjs";
import { CompletionScope } from "./completion-scope.mjs";
import { CompletionRecovery } from "./completion-recovery.mjs";
import { CleanupOwners } from "./cleanup-owners.mjs";

const endpoint = "http://127.0.0.1:32100";
const turn = () => new Promise(resolve => setImmediate(resolve));
function packet(tabId = "101") {
  const binding = { protocol: 2, requestId: randomUUID(), connection: { instanceId: randomUUID(), connectionNonce: randomUUID(), generation: 1 },
    session: "owner", runId: "run", tabId, documentId: "0".repeat(32), documentGeneration: 7, grantGeneration: 1, snapshotId: randomUUID() };
  const { snapshotId, ...fields } = binding;
  return { ...fields, sequence: 1, deadlineMs: Date.now() + 10000,
    command: { kind: "observe", snapshotId, screenshot: true, preview: false },
    completion: { purpose: "observation", proof: { binding, completionKey: "a".repeat(64) } } };
}
function barrier() {
  let enter; let release;
  const reached = new Promise(resolve => { enter = resolve; });
  const task = new Promise(resolve => { release = resolve; });
  return { reached, release, wait: () => { enter(); return task; } };
}
function store(initial = {}) {
  const data = structuredClone(initial); const events = [];
  const storage = {
    async setAccessLevel(value) { assert.deepEqual(value, { accessLevel: "TRUSTED_CONTEXTS" }); events.push("trusted"); },
    async get(key) { return structuredClone({ [key]: data[key] }); },
    async set(value) { Object.assign(data, structuredClone(value)); events.push("write"); },
    async remove(key) { delete data[key]; events.push("remove"); },
  };
  return { storage, data, events };
}
function fixture(observe = async () => ({ text: "fixture", pngBase64: "fixture-image" })) {
  const saved = store(); const calls = []; const timers = new Map(); const owners = new CleanupOwners();
  const journal = new ObservationJournal({ ...saved, owners });
  const f = { ...saved, calls, timers, owners, journal, cancelRequested: false, phase: "offered" };
  f.fetch = async (url, init) => {
    const route = new URL(url).pathname.split("/").pop(); calls.push(route);
    assert(new URL(url).pathname.startsWith("/cu/extension-observation/"));
    assert.deepEqual(init.headers, { "content-type": "application/json" });
    assert.equal(init.credentials, "omit"); assert.equal(init.redirect, "error");
    assert.equal(JSON.parse(init.body).purpose, "observation");
    if (route === "status") return Response.json({ ok: true, status: { phase: f.phase, cancelRequested: f.cancelRequested } });
    if (route === "retirement") return Response.json({ ok: true, state: f.phase === "settled" ? "settled" : "pending" });
    assert.equal(route, "settle");
    assert.equal(saved.data.cuObservationCleanup.entries[0].phase, "physicallySettled");
    f.phase = "settled"; return Response.json({ ok: true });
  };
  f.client = { connectionEpoch: 1,
    observationScope(_epoch, completion, ownedJournal) { return new CompletionScope({ endpoint, proof: completion.proof,
      purpose: "observation", journal: ownedJournal, fetch: (...args) => f.fetch(...args) }); },
    async claimObservation(_epoch, _completion, signal) {
      calls.push("claim"); assert.equal(saved.data.cuObservationCleanup.entries[0].phase, "prepared");
      if (signal.aborted) throw new Error("cancelled"); f.phase = "claimed";
    } };
  f.owner = new ObservationCompletions({ client: f.client, journal, observe: async (...args) => {
    calls.push("observe"); assert.equal(saved.data.cuObservationCleanup.entries[0].phase, "executing"); return observe(...args);
  }, schedule(fn, delay) { const id = randomUUID(); timers.set(id, { fn, delay }); return id; }, cancel(id) { timers.delete(id); } });
  return f;
}

test("observation packets require exact purpose and every original binding, including the document and snapshot", () => {
  const request = packet(); const copied = copyBoundObservation(request);
  assert(Object.isFrozen(copied)); assert(Object.isFrozen(copied.completion.proof.binding.connection));
  for (const key of ["protocol", "requestId", "session", "runId", "tabId", "documentId", "documentGeneration", "grantGeneration"]) {
    assert.throws(() => copyBoundObservation({ ...request, [key]: "other" }), /invalidObservationScope/);
  }
  for (const completion of [{ ...request.completion, purpose: "action" }, request.completion.proof,
    { ...request.completion, extra: true }]) assert.throws(() => copyObservationProof(completion), /invalidObservationScope|invalidCompletion/);
  for (const bad of [{ ...request, protocol: 1 }, { ...request, extra: true }, { ...request, sequence: 0 },
    { ...request, deadlineMs: Date.now() + 12000 }, { ...request, deadlineMs: Date.now() - 1 },
    { ...request, connection: { ...request.connection, generation: 2 } },
    { ...request, command: { ...request.command, snapshotId: randomUUID() } },
    { ...request, command: { ...request.command, screenshot: "true" } },
    { ...request, command: { ...request.command, script: "eval" } }]) assert.throws(() => copyBoundObservation(bad));
});

test("natural observation acknowledges prepare, claim, executing and physical settlement before returning an image", async () => {
  const f = fixture(); const request = packet(); const result = await f.owner.run(1, request);
  assert.deepEqual(result, { kind: "observation", observation: { text: "fixture", pngBase64: "fixture-image" } });
  assert(f.owner.idle()); assert(!await f.journal.hasTab("101")); assert(!f.data.cuObservationCleanup);
  assert.deepEqual(f.calls, ["claim", "status", "observe", "settle"]);
  assert.equal(await f.owner.run(1, request), null); assert.equal(f.calls.filter(call => call === "observe").length, 1);
  assert.equal(f.timers.size, 0);
});

test("reset and connection replacement retain the original reader and discard its late image", async () => {
  for (const replace of [false, true]) {
    const read = barrier(); let signal; let finished = false;
    const f = fixture(async (_request, incoming) => { signal = incoming; await read.wait(); return { pngBase64: "late-image" }; });
    const running = f.owner.run(1, packet()).then(result => { finished = true; return result; });
    await read.reached;
    if (replace) f.client.connectionEpoch++;
    f.owner.reset(); await turn();
    assert(signal.aborted); assert(!finished); assert(!f.owner.idle()); assert(!f.calls.includes("settle"));
    assert(await f.journal.hasTab("101"));
    assert.equal(await f.owner.run(f.client.connectionEpoch, packet()), null, "same tab stays occupied until the original read joins");
    read.release(); assert.deepEqual(await running, { kind: "rejected", reason: "cancelled" });
    assert(f.owner.idle()); assert.equal(f.calls.filter(call => call === "observe").length, 1);
  }
});

test("Host cancellation, status loss and deadline abort the reader without racing its original promise", async () => {
  for (const mode of ["cancel", "status-loss", "deadline"]) {
    const read = barrier(); const aborted = barrier(); let finished = false;
    const f = fixture(async (_request, signal) => {
      signal.addEventListener("abort", () => { void aborted.wait(); }, { once: true });
      if (signal.aborted) void aborted.wait();
      await read.wait(); return { pngBase64: "late-image" };
    });
    f.cancelRequested = mode === "cancel";
    if (mode === "status-loss") {
      const fetch = f.fetch; f.fetch = async (...args) => {
        if (args[0].endsWith("/status")) { f.calls.push("status"); throw new Error("offline"); } return fetch(...args);
      };
    }
    const request = packet(); if (mode === "deadline") request.deadlineMs = Date.now() + 50;
    const running = f.owner.run(1, request).then(result => { finished = true; return result; });
    await read.reached; await aborted.reached; await turn();
    assert(!finished); assert(!f.calls.includes("settle")); assert(await f.journal.hasTab("101"));
    read.release(); assert.equal((await running).kind, "rejected"); assert(f.owner.idle()); aborted.release();
  }
});

test("settle waits for the original status reader cancellation even after the browser read returns", async () => {
  const read = barrier(); const body = barrier(); let finished = false;
  const f = fixture(async () => { await read.wait(); return { pngBase64: "late-image" }; });
  const fetch = f.fetch;
  f.fetch = async (...args) => {
    if (!args[0].endsWith("/status")) return fetch(...args);
    f.calls.push("status");
    return { ok: true, body: { getReader: () => ({ async read() { return { done: true }; }, async cancel() { await body.wait(); } }) } };
  };
  const running = f.owner.run(1, packet()).then(result => { finished = true; return result; });
  await read.reached; await body.reached;
  f.owner.reset(); read.release(); await turn();
  assert(!finished); assert(!f.calls.includes("settle")); assert(!f.owner.idle());
  body.release(); assert.equal((await running).kind, "rejected"); assert(f.owner.idle());
});

test("unacknowledged prepare or executing writes never start the browser reader", async () => {
  for (const phase of ["prepare", "executing"]) {
    const f = fixture(); const write = f.storage.set; const held = barrier(); let blocked = false;
    f.storage.set = async value => {
      if (value.cuObservationCleanup.entries[0].phase === (phase === "prepare" ? "prepared" : "executing") && !blocked) {
        blocked = true; await held.wait();
      }
      return write(value);
    };
    const controller = new AbortController(); const running = f.owner.run(1, packet(), controller.signal);
    await held.reached; assert(!f.calls.includes("observe"));
    if (phase === "prepare") assert(!f.calls.includes("claim"));
    controller.abort(); held.release(); await running;
    assert(!f.calls.includes("observe")); assert(f.owner.idle());
  }
});

test("lost claim and storage replies never replay a read; cleanup failures retain shared admission", async () => {
  for (const phase of ["claim", "prepared", "executing"]) {
    const f = fixture(); const request = packet();
    if (phase === "claim") f.client.claimObservation = async () => { f.calls.push("claim"); throw new Error("lost reply"); };
    else {
      const write = f.storage.set;
      f.storage.set = async value => {
        await write(value);
        if (value.cuObservationCleanup.entries[0].phase === phase) throw new Error("lost reply");
      };
    }
    const remove = f.storage.remove; f.storage.remove = async () => { throw new Error("deletion failed"); };
    assert.equal(await f.owner.run(1, request), null); assert(!f.owner.idle()); assert(await f.journal.hasTab("101"));
    assert(!f.calls.includes("observe")); assert.equal(await f.owner.run(1, request), null);
    const replacement = packet(); assert.throws(() => f.owners.claim("action", replacement.completion.proof), /cleanupOwnerUnavailable/);
    const settleCount = f.calls.filter(call => call === "settle").length;
    f.storage.remove = remove; await f.owner.retryCleanup(); assert(f.owner.idle()); assert.equal(f.timers.size, 0);
    assert.equal(f.calls.filter(call => call === "settle").length, settleCount, "acknowledged Host settlement is not resent");
    f.owners.claim("action", replacement.completion.proof);
  }
});

test("unconfirmed Host settlement retries only cleanup and holds capacity across reset", async () => {
  const f = fixture(); const fetch = f.fetch; let offline = true;
  f.fetch = async (...args) => {
    if (offline && !args[0].endsWith("/status")) { f.calls.push(new URL(args[0]).pathname.split("/").pop()); throw new Error("offline"); }
    return fetch(...args);
  };
  assert.equal(await f.owner.run(1, packet()), null); f.owner.reset(); await f.owner.retryCleanup();
  assert(!f.owner.idle()); assert(await f.journal.hasTab("101")); assert.equal(f.timers.size, 1);
  assert.equal(f.calls.filter(call => call === "observe").length, 1);
  offline = false; await f.owner.retryCleanup(); assert(f.owner.idle()); assert.equal(f.timers.size, 0);
  assert.equal(f.calls.filter(call => call === "claim").length, 1); assert.equal(f.calls.filter(call => call === "observe").length, 1);
});

test("restart recovery settles only prepared or physically finished reads, never executing reads or DOM receipts", async () => {
  for (const phase of ["prepared", "executing", "physicallySettled"]) {
    const saved = store(); const original = new ObservationJournal(saved); const proof = packet().completion.proof;
    await original.reserve(endpoint, proof);
    if (phase === "executing") await original.begin(proof);
    if (phase === "physicallySettled") await original.physicalFinished(proof);
    const restored = new ObservationJournal(saved); const routes = []; const timers = new Map(); let scripts = 0;
    const recovery = new CompletionRecovery({ journal: restored, purpose: "observation", scripting: {
      async executeScript() { scripts++; throw new Error("a DOM action nonce cannot attest a browser read"); } },
      fetch: async (url, init) => { routes.push(url); assert.equal(JSON.parse(init.body).purpose, "observation"); return Response.json({ ok: true }); },
      schedule(fn, delay) { timers.set(1, { fn, delay }); return 1; }, cancel(id) { timers.delete(id); } });
    await recovery.start(Promise.resolve()); assert.equal(scripts, 0);
    assert.equal(await restored.hasTab("101"), phase === "executing");
    assert.equal(routes.length, phase === "executing" ? 0 : 1); assert.equal(timers.size, phase === "executing" ? 1 : 0);
    if (phase === "executing") {
      await restored.forgetBrowserExit([randomUUID()]); await recovery.retry(); assert(await restored.hasTab("101")); assert.equal(routes.length, 0);
      await restored.forgetBrowserExit([proof.binding.requestId]); await recovery.retry();
      assert(!await restored.hasTab("101")); assert.equal(timers.size, 0); assert.equal(routes.length, 0);
    }
  }
});

test("trusted observation journal excludes images, commands and action nonces; proof matching ignores property order", async () => {
  const saved = store(); const proof = packet().completion.proof;
  const lifetime = { instanceId: proof.binding.connection.instanceId, platform: "windows", scope: "a", pid: 123, birth: "b" };
  let retired = false;
  const journal = new ObservationJournal({ ...saved, hostLifetime: { async capture() { return lifetime; }, async retired() { return retired; } } });
  await journal.reserve(endpoint, proof);
  const record = saved.data.cuObservationCleanup.entries[0];
  assert.deepEqual(Object.keys(record).sort(), ["endpoint", "hostLifetime", "phase", "proof", "purpose"]);
  assert.equal(record.purpose, "observation"); assert(!JSON.stringify(journal).includes(proof.completionKey));
  const reordered = { completionKey: proof.completionKey, binding: Object.fromEntries(Object.entries(proof.binding).reverse()) };
  assert(await journal.retains(reordered)); assert.equal(await journal.retiredHost(proof), false);
  await journal.begin(reordered); assert.equal(await journal.provePhysical(reordered), false);
  assert.equal(await journal.retiredHost(proof), false); await journal.physicalFinished(reordered);
  assert.equal(await journal.retiredHost(proof), false); retired = true; assert.equal(await journal.retiredHost(proof), true);
  await journal.forget(reordered); assert(!await journal.hasTab("101"));
});

test("action and observation journals share eight slots and exclude each other's pending tab across write failures", async () => {
  const saved = store(); const owners = new CleanupOwners(); const observation = new ObservationJournal({ ...saved, owners });
  const action = new CompletionJournal({ ...saved, owners,
    lifetime: { async attest() { return { incognito: false }; } },
    scripting: { async executeScript(request) { return [{ frameId: 0, documentId: request.target.documentIds[0], result: { status: "armed" } }]; } },
    receipts: { async setAccessLevel() {}, async get() { return {}; }, async remove() {} } });
  const receipts = [];
  for (let index = 0; index < 8; index++) {
    const proof = packet(String(index)).completion.proof; const journal = index % 2 ? action : observation;
    await journal.reserve(endpoint, proof); receipts.push({ proof, journal });
  }
  await assert.rejects(observation.reserve(endpoint, packet("9").completion.proof));
  await assert.rejects(action.reserve(endpoint, packet("0").completion.proof));
  assert(await observation.hasTab("0")); assert(await action.hasTab("1"));
  const original = receipts[0]; await original.journal.physicalFinished(original.proof);
  const write = saved.storage.set; saved.storage.set = async () => { throw new Error("disk full"); };
  await assert.rejects(original.journal.forget(original.proof));
  await assert.rejects(action.reserve(endpoint, packet("0").completion.proof));
  saved.storage.set = write; await original.journal.forget(original.proof);
  await action.reserve(endpoint, packet("0").completion.proof);
  await assert.rejects(observation.reserve(endpoint, packet("0").completion.proof));
});

test("restored observation admission blocks action reuse and exact browser cleanup releases it only after persistence", async () => {
  const saved = store(); const proof = packet().completion.proof; const original = new ObservationJournal(saved);
  await original.reserve(endpoint, proof); await original.begin(proof);
  const owners = new CleanupOwners(); const restored = new ObservationJournal({ ...saved, owners }); await restored.ready;
  const replacement = packet().completion.proof; assert.throws(() => owners.claim("action", replacement));
  const remove = saved.storage.remove; saved.storage.remove = async () => { throw new Error("reply loss"); };
  await assert.rejects(restored.forgetBrowserExit([proof.binding.requestId])); assert.throws(() => owners.claim("action", replacement));
  saved.storage.remove = remove; await restored.forgetBrowserExit([proof.binding.requestId]); owners.claim("action", replacement);
});

test("malformed or mismatched restored cleanup records fail closed", async () => {
  const proof = packet().completion.proof;
  const record = { endpoint, purpose: "observation", proof, phase: "prepared", hostLifetime: null };
  for (const bad of [{ ...record, purpose: "action" }, { ...record, terminalToken: "nonce" },
    { ...record, phase: "finished" }, { ...record, proof: { ...proof, binding: { ...proof.binding, documentId: "invalid" } } }]) {
    const journal = new ObservationJournal(store({ cuObservationCleanup: { version: 1, entries: [bad] } }));
    await assert.rejects(journal.ready, /observationJournalUnavailable/);
  }
  const saved = store(); const journal = new ObservationJournal(saved); await journal.reserve(endpoint, proof);
  for (const wrong of [{ ...proof, completionKey: "b".repeat(64) },
    { ...proof, binding: { ...proof.binding, documentGeneration: 8 } }]) await assert.rejects(journal.retains(wrong), /observationJournalUnavailable/);
  assert(await journal.hasTab("101")); assert.equal(saved.data.cuObservationCleanup.entries[0].phase, "prepared");
});
