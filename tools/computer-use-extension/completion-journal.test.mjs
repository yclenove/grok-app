import { test } from "node:test";
import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { CompletionJournal } from "./completion-journal.mjs";
import { CompletionScope } from "./completion-scope.mjs";
import { CompletionRecovery } from "./completion-recovery.mjs";
import { SharedTabs } from "./shared-tabs.mjs";
import { ExecutionClock } from "./execution-clock.mjs";

const endpoint = "http://127.0.0.1:32100";
function proof(tab = "101") {
  return { binding: { protocol: 2, requestId: randomUUID(), connection: { instanceId: randomUUID(), connectionNonce: randomUUID(), generation: 1 },
    session: "owner", runId: "run", tabId: tab, documentId: "0".repeat(32), documentGeneration: 7, grantGeneration: 1, snapshotId: randomUUID() },
  completionKey: "a".repeat(64) };
}
function store(initial = {}) {
  const data = structuredClone(initial); const events = [];
  const terminal = {};
  const receipts = { async setAccessLevel(value) { assert.deepEqual(value, { accessLevel: "TRUSTED_AND_UNTRUSTED_CONTEXTS" }); events.push("untrusted"); },
    async get(key) { return structuredClone(key === null ? terminal : { [key]: terminal[key] }); },
    async set(value) { Object.assign(terminal, structuredClone(value)); },
    async remove(keys) { for (const key of Array.isArray(keys) ? keys : [keys]) delete terminal[key]; } };
  const storage = {
    async setAccessLevel(value) { assert.deepEqual(value, { accessLevel: "TRUSTED_CONTEXTS" }); events.push("trusted"); },
    async get(key) { events.push("read"); return structuredClone({ [key]: data[key] }); },
    async set(value) { Object.assign(data, structuredClone(value)); events.push("write"); },
    async remove(key) { delete data[key]; events.push("remove"); },
  };
  const lifetime = { attest: async () => ({ incognito: false }), state: async () => "active" };
  const scripting = { executeScript: async request => {
    assert.equal(request.func.name, "armDocumentCompletion");
    return [{ frameId: 0, documentId: request.target.documentIds[0], result: { status: "armed" } }];
  } };
  return { data, events, storage, lifetime, scripting, receipts, terminal };
}
function barrier() {
  let enter; let release;
  return { reached: new Promise(resolve => { enter = resolve; }),
    wait: () => { enter(); return new Promise(resolve => { release = resolve; }); }, release: () => release?.() };
}
async function restored(phase = "prepared") {
  const saved = store(); const original = new CompletionJournal(saved); const receipt = proof();
  await original.reserve(endpoint, receipt);
  if (phase === "physicallySettled") await original.physicalFinished(receipt);
  return { ...saved, receipt, journal: new CompletionJournal(saved) };
}
function recover(f, scripting, fetch = async () => Response.json({ ok: true })) {
  const timers = new Map();
  const recovery = new CompletionRecovery({ journal: f.journal, scripting, fetch,
    schedule(fn, delay) { const id = randomUUID(); timers.set(id, { fn, delay }); return id; },
    cancel(id) { timers.delete(id); } });
  return { recovery, timers };
}

test("journal retains only strict cleanup authority and startup ownership excludes new operations", async () => {
  const f = store(); const journal = new CompletionJournal(f); const receipt = proof();
  await journal.reserve(endpoint, receipt);
  receipt.completionKey = "b".repeat(64);
  assert.equal(f.data.cuActionCleanup.entries[0].proof.completionKey, "a".repeat(64));
  assert.deepEqual(Object.keys(f.data.cuActionCleanup.entries[0]).sort(), ["documentContext", "endpoint", "hostLifetime", "phase", "proof", "terminalToken"]);
  assert.deepEqual(await journal.takeRestored(), []);
  const next = new CompletionJournal(f);
  await next.reserve(endpoint, proof("102"));
  const initial = await next.takeRestored();
  assert.equal(initial.length, 1); assert.equal(initial[0].proof.binding.tabId, "101");
  assert.deepEqual(await next.takeRestored(), []);
  assert(!JSON.stringify(journal).includes("a".repeat(64)));
  assert.deepEqual(f.events.slice(0, 2), ["untrusted", "trusted"]);
});

test("a failed durable write injects no document script", async () => {
  const f = store();
  let scripts = 0;
  const execute = f.scripting.executeScript;
  f.scripting.executeScript = async (request) => { scripts += 1; return execute(request); };
  f.storage.set = async () => { throw new Error("disk full"); };
  const journal = new CompletionJournal(f);
  await assert.rejects(journal.reserve(endpoint, proof()), /completionJournalUnavailable/);
  assert.equal(scripts, 0);
  assert.equal(f.data.cuActionCleanup, undefined);
});

test("ambiguous storage write stays occupied and never silently drops its recovery record", async () => {
  const f = store(); const journal = new CompletionJournal(f); const receipt = proof();
  const write = f.storage.set;
  f.storage.set = async value => { await write(value); throw new Error("reply lost"); };
  await assert.rejects(journal.reserve(endpoint, receipt), /completionJournalUnavailable/);
  assert(await journal.hasTab("101"));
  const afterRestart = new CompletionJournal(f);
  assert(await afterRestart.hasTab("101"));
  f.storage.set = write;
  await journal.physicalFinished(receipt); await journal.forget(receipt);
  assert.equal(await journal.hasTab("101"), false);
});

test("legacy records are explicitly unattested and a failed document admission cannot reserve or claim", async () => {
  const receipt = proof();
  const f = store({ cuActionCleanup: { version: 1, entries: [{ endpoint, proof: receipt, phase: "prepared" }] } });
  const journal = new CompletionJournal(f);
  assert.equal((await journal.takeRestored())[0].documentContext, null);
  f.lifetime.state = async () => { throw new Error("legacy must not infer destruction"); };
  assert.equal(await journal.provePhysical(receipt), false);
  await journal.physicalFinished(receipt);
  assert.equal(f.data.cuActionCleanup.version, 4); assert.equal(f.data.cuActionCleanup.entries[0].documentContext, null);
  const fresh = store(); fresh.lifetime.attest = async () => { throw new Error("untracked original document"); };
  const next = new CompletionJournal(fresh);
  await assert.rejects(next.reserve(endpoint, proof()), /completionJournalUnavailable/);
  assert.equal(await next.hasTab("101"), false); assert.equal(fresh.data.cuActionCleanup, undefined);
});

test("failed deletion retains local occupancy; acknowledged settlement is not sent again", async () => {
  const f = store(); const journal = new CompletionJournal(f); const receipt = proof(); let requests = 0;
  const scope = new CompletionScope({ endpoint, proof: receipt, journal, fetch: async (_url, init) => {
    requests++; assert.equal(f.data.cuActionCleanup.entries[0].phase, "physicallySettled");
    assert.deepEqual(init.headers, { "content-type": "application/json" }); return Response.json({ ok: true });
  } });
  await scope.prepare();
  const remove = f.storage.remove;
  f.storage.remove = async () => { throw new Error("unavailable"); };
  await assert.rejects(scope.settle(), /completionJournalUnavailable/);
  assert(await journal.hasTab("101")); assert(f.data.cuActionCleanup);
  f.storage.remove = remove;
  await scope.settle(); assert.equal(requests, 1);
  assert.equal(await journal.hasTab("101"), false); assert(!f.data.cuActionCleanup);
});

test("a physically finished owner can retire after a lost settle reply and bounded tombstone eviction", async () => {
  for (const terminal of ["settled", "absent"]) {
    const f = store(); const journal = new CompletionJournal(f); const receipt = proof(); const routes = [];
    const scope = new CompletionScope({ endpoint, proof: receipt, journal, fetch: async url => {
      const route = new URL(url).pathname.split("/").pop(); routes.push(route);
      if (route === "settle") return new Response("gone", { status: 403 });
      return Response.json({ ok: true, state: terminal });
    } });
    await scope.prepare(); await scope.settle();
    assert.deepEqual(routes, ["settle", "retirement"]); assert.equal(await journal.hasTab("101"), false);
  }
});

test("retirement pending, malformed or unauthenticated responses never erase local physical ownership", async () => {
  for (const retirement of [{ ok: true, state: "pending" }, { ok: true, state: "unknown" },
    { ok: true, state: "absent", extra: true }, null]) {
    const f = store(); const journal = new CompletionJournal(f); const receipt = proof();
    const scope = new CompletionScope({ endpoint, proof: receipt, journal, fetch: async url => {
      if (url.endsWith("/settle")) return new Response("unavailable", { status: 403 });
      if (retirement === null) return new Response("forbidden", { status: 403 });
      return Response.json(retirement);
    } });
    await scope.prepare(); await assert.rejects(scope.settle(), /completionUnavailable/);
    assert(await journal.hasTab("101")); assert.equal(f.data.cuActionCleanup.entries[0].phase, "physicallySettled");
  }
});

test("retirement fallback is unavailable without a persisted physical journal", async () => {
  const receipt = proof(); let requests = 0;
  const scope = new CompletionScope({ endpoint, proof: receipt, fetch: async () => {
    requests++; return new Response("gone", { status: 403 });
  } });
  await assert.rejects(scope.settle(), /completionUnavailable/); assert.equal(requests, 1);
});

test("eight pending receipts never get evicted and malformed startup records block new sharing", async () => {
  const f = store(); const journal = new CompletionJournal(f);
  for (let id = 0; id < 8; id++) await journal.reserve(endpoint, proof(String(id)));
  await assert.rejects(journal.reserve(endpoint, proof("8")), /completionJournalUnavailable/);
  assert.equal(f.data.cuActionCleanup.entries.length, 8);
  for (const value of [{ version: 5, entries: [] }, { version: 1, entries: [
    { endpoint, proof: proof(), phase: "prepared", command: { text: "must not be retained" } }] },
  { version: 1, entries: Array.from({ length: 9 }, (_, i) => ({ endpoint, proof: proof(String(i)), phase: "prepared" })) }]) {
    const bad = store({ cuActionCleanup: value }); const broken = new CompletionJournal(bad);
    let inspected = false;
    const sharing = new SharedTabs({ client: { connectionEpoch: 1 }, journal: broken, clock: new ExecutionClock(1),
      tabs: { query: () => { inspected = true; throw new Error("must not inspect"); } }, scripting: {} });
    await assert.rejects(sharing.share(101), /completionJournalUnavailable/); assert(!inspected);
    assert.deepEqual(bad.data.cuActionCleanup, value, "corrupt data must not be silently erased");
  }
});

test("cleanup must match the complete original binding and key, independent of JSON property order", async () => {
  const f = store(); const journal = new CompletionJournal(f); const receipt = proof();
  await journal.reserve(endpoint, receipt);
  for (const mutate of [p => { p.completionKey = "b".repeat(64); }, p => { p.binding.session = "other"; },
    p => { p.binding.grantGeneration++; }, p => { p.binding.documentId = "1".repeat(32); }]) {
    const changed = structuredClone(receipt); mutate(changed);
    await assert.rejects(journal.physicalFinished(changed), /completionJournalUnavailable/);
    await assert.rejects(journal.forget(changed), /completionJournalUnavailable/);
    assert.equal(f.data.cuActionCleanup.entries[0].phase, "prepared");
  }
  const reordered = { completionKey: receipt.completionKey,
    binding: Object.fromEntries(Object.entries(receipt.binding).reverse()) };
  await journal.physicalFinished(reordered); await journal.forget(reordered);
  assert.equal(await journal.hasTab("101"), false);
});

test("document terminal receipt authenticates native sender and persists only the exact owner's physical completion", async () => {
  const f = store(); const journal = new CompletionJournal(f); const receipt = proof();
  await journal.reserve(endpoint, receipt);
  const message = { type: "cu-document-finished", snapshotId: receipt.binding.snapshotId, requestId: receipt.binding.requestId };
  const sender = { frameId: 0, documentId: receipt.binding.documentId, tab: { id: 101, incognito: false } };
  for (const bad of [{ ...sender, frameId: 1 }, { ...sender, documentId: "1".repeat(32) },
    { ...sender, tab: { id: 102, incognito: false } }, { ...sender, tab: { id: 101, incognito: true } },
    { ...sender, tab: { id: 101 } }]) assert.equal(await journal.documentFinished(message, bad), false);
  for (const bad of [{ ...message, requestId: randomUUID() }, { ...message, snapshotId: randomUUID() },
    { ...message, extra: true }]) assert.equal(await journal.documentFinished(bad, sender), false);
  assert.equal(f.data.cuActionCleanup.entries[0].phase, "prepared");
  assert.equal(await journal.documentFinished(message, sender), true);
  const afterRestart = new CompletionJournal(f);
  f.lifetime.state = () => { throw new Error("terminal proof must not query a replacement document"); };
  assert.equal(await afterRestart.provePhysical(receipt), true);
  await journal.forget(receipt); await journal.reserve(endpoint, proof());
  assert.equal(await journal.documentFinished(message, sender), false);
  assert.equal(f.data.cuActionCleanup.entries[0].phase, "prepared", "late old receipt cannot finish a new owner");
});

test("guard installation is acknowledged after storage, and failed or mismatched native replies keep a prepared record", async () => {
  for (const failure of ["error", "mismatch", "wrong-status"]) {
    const f = store(); const journal = new CompletionJournal(f); const receipt = proof();
    f.scripting.executeScript = async request => {
      assert.equal(f.data.cuActionCleanup.entries[0].phase, "prepared");
      assert.equal(request.func.name, "armDocumentCompletion");
      assert.deepEqual(request.args, [receipt.binding.snapshotId, receipt.binding.requestId,
        f.data.cuActionCleanup.entries[0].terminalToken]);
      if (failure === "error") throw new Error("native error");
      return [{ frameId: 0, documentId: failure === "mismatch" ? "1".repeat(32) : receipt.binding.documentId,
        result: { status: failure === "wrong-status" ? "settled" : "armed" } }];
    };
    await assert.rejects(journal.reserve(endpoint, receipt)); assert(await journal.hasTab("101"));
    assert.equal(f.data.cuActionCleanup.entries[0].phase, "prepared");
  }
});

test("startup can consume a terminal nonce after the sender and its runtime message disappear", async () => {
  const f = store(); const first = new CompletionJournal(f); const receipt = proof();
  await first.reserve(endpoint, receipt);
  const key = "cuDocumentFinished." + receipt.binding.requestId;
  f.terminal[key] = { token: f.data.cuActionCleanup.entries[0].terminalToken };
  const journal = new CompletionJournal(f);
  f.lifetime.state = () => { throw new Error("destroyed document cannot be queried"); };
  assert.equal(await journal.provePhysical(receipt), true);
  assert.equal(f.data.cuActionCleanup.entries[0].phase, "physicallySettled"); assert.equal(f.terminal[key], undefined);
  assert(await journal.hasTab("101"), "Host settlement and storage deletion are still required");
  await journal.forget(receipt); assert.equal(await journal.hasTab("101"), false);
});

test("wrong terminal nonces and old request keys cannot finish another owner's work", async () => {
  const f = store(); const journal = new CompletionJournal(f); const receipt = proof();
  await journal.reserve(endpoint, receipt); const key = "cuDocumentFinished." + receipt.binding.requestId;
  f.terminal[key] = { token: "f".repeat(64) };
  assert.equal(await journal.storedReceipt(key), null); assert.equal(f.data.cuActionCleanup.entries[0].phase, "prepared");
  f.terminal[key] = { token: f.data.cuActionCleanup.entries[0].terminalToken, extra: true };
  assert.equal(await journal.storedReceipt(key), null);
  const old = "cuDocumentFinished." + randomUUID(); f.terminal[old] = { token: "e".repeat(64) };
  assert.equal(await journal.storedReceipt(old), null); assert.equal(f.terminal[old], undefined);
  f.terminal.unrelated = { keep: true }; assert.equal(await journal.storedReceipt("unrelated"), null);
  assert.deepEqual(f.terminal.unrelated, { keep: true });
  f.terminal[key] = { token: f.data.cuActionCleanup.entries[0].terminalToken };
  const wrong = structuredClone(receipt); wrong.completionKey = "f".repeat(64);
  await assert.rejects(journal.forget(wrong)); assert(f.terminal[key], "a wrong proof cannot erase physical evidence");
  assert.equal(await journal.storedReceipt(key), "101"); assert.equal(f.terminal[key], undefined);
});

test("startup reaps orphan terminal keys, preserves unrelated data and migrates v2 without inventing a guard nonce", async () => {
  const receipt = proof(); const f = store({ cuActionCleanup: { version: 2, entries: [
    { endpoint, proof: receipt, phase: "prepared", documentContext: { incognito: false } }] } });
  const key = "cuDocumentFinished." + receipt.binding.requestId; const orphan = "cuDocumentFinished." + randomUUID();
  f.terminal[key] = { token: "d".repeat(64) }; f.terminal[orphan] = { token: "e".repeat(64) }; f.terminal.unrelated = true;
  const journal = new CompletionJournal(f); await journal.ready;
  assert.equal((await journal.takeRestored())[0].terminalToken, null);
  assert.equal(f.terminal[orphan], undefined); assert.equal(f.terminal.unrelated, true);
  assert.equal(await journal.storedReceipt(key), null); assert.equal(f.data.cuActionCleanup.entries[0].phase, "prepared");
});

test("receipt removal failure retains the original session owner and can be retried without action replay", async () => {
  const f = store(); const journal = new CompletionJournal(f); const receipt = proof(); await journal.reserve(endpoint, receipt);
  const key = "cuDocumentFinished." + receipt.binding.requestId;
  f.terminal[key] = { token: f.data.cuActionCleanup.entries[0].terminalToken };
  const remove = f.receipts.remove; f.receipts.remove = async () => { throw new Error("disk unavailable"); };
  await assert.rejects(journal.storedReceipt(key));
  assert.equal(f.data.cuActionCleanup.entries[0].phase, "physicallySettled"); assert(await journal.hasTab("101"));
  f.receipts.remove = remove; assert.equal(await journal.storedReceipt(key), "101");
  await journal.forget(receipt); assert.equal(await journal.hasTab("101"), false); assert.equal(f.terminal[key], undefined);
});

test("startup recovery waits for pairing retirement and the original cancellation script before settling", async () => {
  const f = await restored(); const priorPairing = barrier(); const script = barrier(); let requests = 0;
  const { recovery } = recover(f, { executeScript: async request => {
    assert.equal(request.func.name, "cancelDocumentOperation"); assert.equal(request.world, "ISOLATED");
    assert.deepEqual(request.target, { tabId: 101, documentIds: [f.receipt.binding.documentId] });
    assert.deepEqual(request.args, [f.receipt.binding.snapshotId, f.receipt.binding.requestId, true]);
    await script.wait(); return [{ frameId: 0, documentId: f.receipt.binding.documentId, result: { status: "settled" } }];
  } }, async url => { requests++; assert(url.endsWith("/settle")); return Response.json({ ok: true }); });
  const retiring = priorPairing.wait(); const task = recovery.start(retiring);
  await new Promise(resolve => setImmediate(resolve)); assert.equal(requests, 0);
  await f.journal.reserve(endpoint, proof("102"));
  priorPairing.release(); await script.reached;
  assert(await f.journal.hasTab("101")); assert.equal(requests, 0);
  script.release(); await task;
  assert.equal(requests, 1); assert.equal(await f.journal.hasTab("101"), false);
  assert(await f.journal.hasTab("102"), "new-worker work is not owned by startup recovery");
});

test("missing or wrong original document keeps its receipt and only cleanup can retry", async () => {
  for (const mode of ["missing", "wrong-document", "native-error"]) {
    const f = await restored(); let ready = false; let requests = 0;
    const { recovery, timers } = recover(f, { executeScript: async () => {
      if (!ready && mode === "native-error") throw new Error("permission unavailable");
      return [{ frameId: 0, documentId: !ready && mode === "wrong-document" ? "1".repeat(32) : f.receipt.binding.documentId,
        result: { status: ready ? "settled" : "snapshot_missing" } }];
    } }, async () => { requests++; return Response.json({ ok: true }); });
    await recovery.start(Promise.resolve());
    assert(await f.journal.hasTab("101")); assert.equal(requests, 0); assert.equal(timers.size, 1);
    ready = true; await recovery.retry();
    assert.equal(requests, 1); assert.equal(await f.journal.hasTab("101"), false); assert.equal(timers.size, 0);
  }
});

test("physically finished records survive lost Host replies and recovery never reinjects an action", async () => {
  const f = await restored("physicallySettled"); let requests = 0;
  const { recovery, timers } = recover(f, { executeScript: () => { throw new Error("must not inject any browser script"); } },
    async (url, init) => {
      assert(url.endsWith("/settle")); assert(!Object.hasOwn(init.headers, "authorization"));
      if (++requests === 1) throw new Error("lost reply"); return Response.json({ ok: true });
    });
  await recovery.start(Promise.resolve()); assert(await f.journal.hasTab("101"));
  await recovery.retry(); assert.equal(await f.journal.hasTab("101"), false);
  assert.equal(requests, 2); assert.equal(timers.size, 0);
});

test("confirmed browser-exit retirement also retires its in-memory recovery task", async () => {
  const f = await restored(); let scripts = 0; let requests = 0;
  const { recovery, timers } = recover(f, { executeScript: async () => {
    scripts++; throw new Error("original browser is gone");
  } }, async () => { requests++; throw new Error("must not resettle retired owner"); });
  await recovery.start(Promise.resolve());
  assert.equal(timers.size, 1); assert.equal(scripts, 1);
  await f.journal.forgetBrowserExit([f.receipt.binding.requestId]);
  await recovery.retry();
  assert.equal(timers.size, 0);
  assert.equal(scripts, 1); assert.equal(requests, 0);
});

test("one unresolved document cannot prevent retrying cleanup for another tab", async () => {
  const saved = store(); const original = new CompletionJournal(saved); const first = proof("101"); const second = proof("102");
  await original.reserve(endpoint, first); await original.reserve(endpoint, second);
  const f = { ...saved, journal: new CompletionJournal(saved) }; const held = barrier();
  let otherReady = false; let otherAttempts = 0;
  const { recovery, timers } = recover(f, { executeScript: async request => {
    if (request.target.tabId === 101) await held.wait();
    else { otherAttempts++; if (!otherReady) throw new Error("temporarily unavailable"); }
    return [{ frameId: 0, documentId: request.target.documentIds[0], result: { status: "settled" } }];
  } });
  const running = recovery.start(Promise.resolve());
  try {
    await held.reached; await new Promise(resolve => setImmediate(resolve));
    assert.equal(otherAttempts, 1); assert.equal(timers.size, 1, "retry must not wait for the unrelated native Promise");
    otherReady = true; void recovery.retry();
    for (let i = 0; i < 20 && await f.journal.hasTab("102"); i++) await new Promise(resolve => setImmediate(resolve));
    assert.equal(await f.journal.hasTab("102"), false);
    assert(await f.journal.hasTab("101")); assert.equal(otherAttempts, 2);
  } finally {
    otherReady = true; held.release(); await running; await recovery.retry();
  }
});

test("cross-Host cleanup requires saved kernel witness and physical completion; live Host and delete failure retain ownership", async () => {
  const f = store(); const receipt = proof(); let retired = false; let checks = 0;
  const lifetime = { instanceId: receipt.binding.connection.instanceId, platform: "windows", scope: "0", pid: 100, birth: "abc" };
  const hostLifetime = { capture: async () => lifetime, retired: async value => { checks++; assert.deepEqual(value, lifetime); return retired; } };
  const journal = new CompletionJournal({ ...f, hostLifetime });
  await journal.reserve(endpoint, receipt);
  assert.equal(await journal.retiredHost(receipt), false); assert.equal(checks, 0);
  const next = new CompletionJournal({ ...f, hostLifetime });
  const [restored] = await next.takeRestored(); assert.deepEqual(restored.hostLifetime, lifetime);
  const scope = new CompletionScope({ endpoint, proof: receipt, journal: next, fetch: async () => { throw new Error("old Host gone"); } });
  await assert.rejects(scope.settle(), /completionUnavailable/); assert(await next.hasTab("101")); assert.equal(checks, 1);
  retired = true;
  const remove = f.storage.remove; f.storage.remove = async () => { throw new Error("ambiguous deletion"); };
  await assert.rejects(scope.settle(), /completionJournalUnavailable/); assert(await next.hasTab("101"));
  f.storage.remove = remove; await scope.settle(); assert.equal(await next.hasTab("101"), false); assert.equal(checks, 2);
});

test("capture failure is zero reservation; v3 never fabricates a Host witness and wrong proof cannot query it", async () => {
  const f = store(); const receipt = proof();
  const journal = new CompletionJournal({ ...f, hostLifetime: { capture: async () => { throw new Error("gone"); } } });
  await assert.rejects(journal.reserve(endpoint, receipt), /completionJournalUnavailable/);
  assert.equal(f.data.cuActionCleanup, undefined);
  const legacy = store({ cuActionCleanup: { version: 3, entries: [{ endpoint, proof: receipt, phase: "physicallySettled",
    documentContext: { incognito: false }, terminalToken: null }] } });
  let calls = 0;
  const old = new CompletionJournal({ ...legacy, hostLifetime: { retired: async () => { calls++; return true; } } });
  assert.equal((await old.takeRestored())[0].hostLifetime, null);
  assert.equal(await old.retiredHost(receipt), false); assert.equal(calls, 0);
  const wrong = structuredClone(receipt); wrong.completionKey = "b".repeat(64);
  await assert.rejects(old.retiredHost(wrong), /completionJournalUnavailable/); assert.equal(calls, 0);
});

test("a delayed process-retirement reply cannot release a replacement owner on the same tab", async () => {
  const f = store(); const old = proof(); const next = proof(); const held = barrier();
  const hostLifetime = {
    capture: async (_endpoint, receipt) => ({ instanceId: receipt.binding.connection.instanceId,
      platform: "windows", scope: "0", pid: 100, birth: "abc" }),
    retired: async () => { await held.wait(); return true; },
  };
  const journal = new CompletionJournal({ ...f, hostLifetime });
  await journal.reserve(endpoint, old); await journal.physicalFinished(old);
  const pending = journal.retiredHost(old); await held.reached;
  try {
    // Another authenticated completion can finish this owner before the slow kernel query.
    await journal.forget(old); await journal.reserve(endpoint, next);
  } finally { held.release(); }
  assert.equal(await pending, false);
  assert(await journal.hasTab(next.binding.tabId));
  assert.equal(f.data.cuActionCleanup.entries[0].phase, "prepared");
  assert.deepEqual(f.data.cuActionCleanup.entries[0].proof, next);
});
