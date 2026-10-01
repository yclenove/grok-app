import assert from "node:assert/strict";
import { saved, oldAuthorityRejected } from "./existing-app-restart-live.mjs";

// Two separate product Host processes; faults wrap native APIs in the production worker.
// No mock Host answers, manufactured process witness, journal mutation, or replacement SW.
export async function verifyHostRetirement(env) {
  const { popup, fixture, rpc, until, getWorker, getTabId, setStage, setStep, pass } = env;
  const counters = () => getWorker().evaluate(() => {
    const f = globalThis.__cuDispatchFaults;
    return { queries: f.hostRetirements, states: f.hostStates, lost: f.lostHostReplies, deletes: f.failedCleanupRemoves };
  });
  const busy = async () => {
    const reply = await popup.evaluate(tabId => chrome.runtime.sendMessage({ type: "cu-share-current", tabId: Number(tabId) }), getTabId());
    assert.equal(reply.ok, false); assert.equal(reply.error, "busy");
    assert.equal((await rpc("shared")).length, 0);
    assert.equal(await fixture.evaluate(() => clicks), 0);
  };
  setStage("live-predecessor-keeps-original-owner"); setStep("hold-accepted-claim");
  await getWorker().evaluate(() => { globalThis.__cuDispatchFaults.delayClaim = true; });
  await env.enqueue("Count", "click", {});
  await until(() => getWorker().evaluate(() => typeof globalThis.__cuDispatchFaults.releaseClaim === "function"));
  const original = await popup.evaluate(async () => {
    const { cuActionCleanup } = await chrome.storage.local.get("cuActionCleanup");
    const { cuPairing } = await chrome.storage.session.get("cuPairing");
    return { record: cuActionCleanup.entries[0], connection: cuPairing };
  });
  assert.equal(original.record.phase, "prepared"); assert(original.record.hostLifetime);
  assert.equal((await rpc("dispatch-state")).idle, false);
  await getWorker().evaluate(endpoint => { globalThis.__cuDispatchFaults.unreachableCompletion = endpoint; }, original.record.endpoint);
  setStep("start-successor-with-original-host-alive");
  const successor = await rpc("host-successor");
  assert(successor.ok && successor.pid !== successor.previousPid);
  assert.equal(successor.previousPid, original.record.hostLifetime.pid);
  assert.equal((await rpc("dispatch-state")).idle, true);
  const challenge = await rpc("begin"); assert((await rpc("confirm", { nonce: challenge.nonce })).ok);
  await oldAuthorityRejected(popup, original, challenge.endpoint, 200);
  const paired = await popup.evaluate(({ endpoint, code }) => chrome.runtime.sendMessage({ type: "cu-pair", endpoint, code }), challenge);
  assert(paired.ok && paired.paired); await busy();
  await getWorker().evaluate(() => globalThis.__cuDispatchFaults.releaseClaim());
  setStep("physical-completion-does-not-retire-live-host");
  await until(async () => (await saved(popup))?.phase === "physicallySettled");
  await until(async () => (await counters()).states.includes("live"));
  const retained = await saved(popup);
  assert.deepEqual(retained.proof, original.record.proof);
  assert.deepEqual(retained.hostLifetime, original.record.hostLifetime);
  assert.deepEqual(await rpc("host-predecessor-state"), { alive: true, pid: successor.previousPid, idle: false });
  assert.equal((await rpc("dispatch-state")).idle, true); await busy();
  assert.equal((await env.stats()).claims, 1); assert.equal((await env.stats()).results, 0);
  pass();

  setStage("lost-host-retirement-reply-keeps-owner"); setStep("terminate-only-original-host");
  await getWorker().evaluate(() => {
    const f = globalThis.__cuDispatchFaults; f.loseHostRetirement = true; f.holdHostRetirement = true;
  });
  const terminated = await rpc("host-predecessor-stop");
  assert(terminated.ok); assert.equal(terminated.previousPid, successor.previousPid);
  assert.equal((await rpc("status")).stored, true);
  await until(async () => (await counters()).lost === 1, 40000);
  assert.equal((await saved(popup)).phase, "physicallySettled"); await busy();
  assert.equal((await env.stats()).claims, 1); assert.equal((await env.stats()).results, 0);
  pass();

  setStage("failed-local-deletion-retries-cleanup-only"); setStep("hold-authenticated-retirement-response");
  await until(() => getWorker().evaluate(() => typeof globalThis.__cuDispatchFaults.releaseHostRetirement === "function"), 40000);
  assert.equal((await saved(popup)).phase, "physicallySettled");
  const acceptedQueries = (await counters()).queries;
  await getWorker().evaluate(() => {
    const f = globalThis.__cuDispatchFaults; f.failCleanupRemove = true; f.releaseHostRetirement();
  });
  await until(async () => (await counters()).deletes === 1);
  assert.equal((await saved(popup)).phase, "physicallySettled"); await busy();
  setStep("retry-storage-without-another-claim-or-kernel-query");
  // Production cleanup backs off to at most 30 seconds; no action/IPC timeout changes here.
  await until(async () => !await saved(popup), 40000);
  assert.equal((await counters()).queries, acceptedQueries);
  assert.equal((await counters()).lost, 1); assert.equal((await counters()).deletes, 1);
  assert.equal((await env.stats()).claims, 1); assert.equal((await env.stats()).results, 0);
  assert.equal(await fixture.evaluate(() => clicks), 0);
  await env.shareCurrent(); await env.enqueue("Count", "click", {});
  const reply = await env.result(); assert(reply.ok); assert.equal(reply.outcome.status, "applied");
  assert.equal(await fixture.evaluate(() => clicks), 1); await env.idle();
  await until(async () => !await saved(popup));
  assert.equal((await env.stats()).claims, 2); assert.equal((await env.stats()).results, 1);
  assert.equal((await counters()).queries, acceptedQueries); assert(!fixture.isClosed());
  process.stderr.write("Host retirement evidence: two Hosts alive before cut; original retained pending; live query retained journal; lost reply/deletion failure retained owner; old click=0; new click=1; tab survives\n");
  pass();
}
