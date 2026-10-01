import assert from "node:assert/strict";
import { restart } from "./existing-dispatch-restarts.mjs";

export async function verifyLiveController(env) {
  const { getWorker, fixture, rpc, until, enqueue, result, idle, shareCurrent, popup, setStage, pass } = env;
  setStage("unknown-action-reload-cleans-live-controller");
  await getWorker().evaluate(() => {
    const f = globalThis.__cuDispatchFaults; f.loseActionReply = true; f.loseCancelReply = true; f.holdLifetime = true;
  });
  await enqueue("Count", "click", {});
  const outcome = await result(); assert(outcome.ok); assert.equal(outcome.outcome.status, "unknown");
  await until(() => getWorker().evaluate(() => typeof globalThis.__cuDispatchFaults.releaseLifetime === "function"));
  assert.equal((await rpc("dispatch-state")).idle, false); assert.equal(await fixture.evaluate(() => clicks), 1);
  await fixture.reload();
  assert.equal((await rpc("dispatch-state")).idle, false, "pending native lifecycle query still owns cleanup");
  await getWorker().evaluate(() => globalThis.__cuDispatchFaults.releaseLifetime());
  await idle();
  await until(() => popup.evaluate(async () => !(await chrome.storage.local.get("cuActionCleanup")).cuActionCleanup));
  await shareCurrent();
  await enqueue("Count", "click", {}); assert((await result()).ok); await idle();
  assert.equal(await fixture.evaluate(() => clicks), 1, "new Share must not inherit the old busy slot or replay its action");
  // The remaining original restart matrix begins on a fresh fixture document with zero clicks.
  await fixture.reload(); await until(async () => (await rpc("shared")).length === 0); await shareCurrent(); pass();
}

export async function verifyRetainedDocument(env) {
  const { getWorker, fixture, rpc, until, enqueue, result, idle, pairAndShare, installFaults, popup, setStage, setStep, pass } = env;
  setStage("cached-original-remains-occupied"); setStep("hold-click-reply");
  await getWorker().evaluate(() => {
    const f = globalThis.__cuDispatchFaults; f.delayScripts = true; f.delayBeforeCancel = true;
  });
  await enqueue("Count", "click", {});
  await until(() => getWorker().evaluate(() => typeof globalThis.__cuDispatchFaults.releaseAction === "function"));
  const documentId = await getWorker().evaluate(() => globalThis.__cuDispatchFaults.lastObservation.target.documentIds[0]);
  assert.equal(await fixture.evaluate(() => clicks), 1);
  setStep("navigate-to-bfcache");
  await fixture.goto(fixture.url() + "?cached-away", { waitUntil: "commit" });
  setStep("query-original-document");
  const frameState = () => popup.evaluate(id => chrome.webNavigation.getFrame({ documentId: id }), documentId);
  // Chrome 148 drops webNavigation tracking for cached documents. Same-document back
  // restoration below is the independent cache proof; null is deliberately not destruction.
  try { await until(async () => { const value = await frameState(); return value === null || value.documentLifecycle === "cached"; }); }
  catch (error) {
    const session = await env.context.newCDPSession(fixture); let rejected;
    session.on("Page.backForwardCacheNotUsed", event => { rejected = event.notRestoredExplanations?.map(row => row.reason); });
    await session.send("Page.enable");
    const before = await frameState();
    await fixture.goBack({ waitUntil: "commit" }).catch(() => {});
    process.stderr.write(`cache diagnostics: ${JSON.stringify({ retainedBefore: before?.documentLifecycle ?? null,
      queryKind: before === null ? "null" : typeof before, keys: before ? Object.keys(before) : [],
      retainedAfter: (await frameState())?.documentLifecycle ?? null, reasons: rejected ?? [],
      clicks: await fixture.evaluate(() => clicks) })}\n`);
    await session.detach(); throw error;
  }
  await restart(env);
  assert.equal((await result()).ok, false);
  setStep("observe-real-cache-retry");
  assert.equal((await rpc("dispatch-state")).idle, false, "a null lifecycle query must not free a cached document");
  await getWorker().evaluate(id => {
    const original = chrome.webNavigation.getFrame.bind(chrome.webNavigation); globalThis.__cuCachedQueries = 0;
    chrome.webNavigation.getFrame = async details => {
      const value = await original(details);
      if (details.documentId === id && (value === null || value.documentLifecycle === "cached")) globalThis.__cuCachedQueries++;
      return value;
    };
  }, documentId);
  await until(() => getWorker().evaluate(() => globalThis.__cuCachedQueries > 0));
  assert.equal((await rpc("dispatch-state")).idle, false);
  assert.equal(await popup.evaluate(async () => (await chrome.storage.local.get("cuActionCleanup")).cuActionCleanup.entries.length), 1);
  const retained = await frameState(); assert(retained === null || retained.documentLifecycle === "cached"); pass();

  setStage("restored-document-cleanup-and-new-click"); setStep("restore-original-document");
  await fixture.goBack({ waitUntil: "commit" });
  await until(async () => (await frameState())?.documentLifecycle === "active");
  assert.equal((await frameState()).documentId, documentId);
  assert.equal(await fixture.evaluate(() => clicks), 1, "BFCache restoration must keep the original page counter");
  await idle();
  await until(() => popup.evaluate(async () => !(await chrome.storage.local.get("cuActionCleanup")).cuActionCleanup));
  await installFaults(); await pairAndShare();
  await enqueue("Count", "click", {}); assert((await result()).ok); await idle();
  assert.equal(await fixture.evaluate(() => clicks), 2); assert(!fixture.isClosed()); pass();
}
