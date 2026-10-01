import assert from "node:assert/strict";

// Full orderly browser exit, then a fresh browser using the same owned disk profile.
// The product Host survives. Test memory observes old proof but never settles on its behalf.
export async function verifyBrowserExit(env) {
  env.setStage("browser-exit-recovers-original-owner"); env.setStep("hold-claimed-action");
  await env.getWorker().evaluate(() => { globalThis.__cuDispatchFaults.delayClaim = true; });
  await env.enqueue("Count", "click", {});
  await env.until(() => env.getWorker().evaluate(() => typeof globalThis.__cuDispatchFaults.releaseClaim === "function"));
  const original = await env.popup.evaluate(async () => (await chrome.storage.local.get("cuActionCleanup")).cuActionCleanup.entries[0]);
  assert.equal(original.phase, "prepared");
  assert.equal((await env.stats()).claimsAccepted, 1);
  assert.equal(await env.fixture.evaluate(() => clicks), 0);
  assert.equal((await env.rpc("dispatch-state")).idle, false);
  const beforeBrowser = await env.popup.evaluate(async () => (await chrome.storage.local.get("cuBrowserSession")).cuBrowserSession);
  assert(beforeBrowser?.id);
  env.setStep("exit-entire-browser-and-reopen-owned-profile");
  await env.restartBrowser();
  const memory = await env.popup.evaluate(async () => {
    const { cuActionCleanup } = await chrome.storage.local.get("cuActionCleanup");
    const { cuPairing } = await chrome.storage.session.get("cuPairing");
    return { journalPresent: cuActionCleanup !== undefined, pairingAbsent: cuPairing === undefined };
  });
  assert.deepEqual(memory, { journalPresent: true, pairingAbsent: true });
  const afterBrowser = await env.popup.evaluate(async () => (await chrome.storage.local.get("cuBrowserSession")).cuBrowserSession);
  assert.notEqual(afterBrowser.id, beforeBrowser.id);
  process.stderr.write("browser lifecycle evidence: same profile fully restarted; new BrowserSession identity; native onStartup persisted\n");
  assert.equal(await env.fixture.evaluate(() => clicks), 0);
  env.setStep("explicit-pair-and-share-after-browser-restart");
  await env.pairAndShare();
  await env.until(async () => (await env.popup.evaluate(async () => !(await chrome.storage.local.get("cuActionCleanup")).cuActionCleanup)));
  const oldReply = await env.result(); assert.equal(oldReply.ok, false);
  const retirement = await env.popup.evaluate(async original => {
    const response = await fetch(original.endpoint + "/cu/extension-completion/retirement", {
      method: "POST", credentials: "omit", cache: "no-store", redirect: "error",
      headers: { "content-type": "application/json" }, body: JSON.stringify(original.proof), signal: AbortSignal.timeout(5000),
    });
    return response.ok ? (await response.json()).state : "unavailable";
  }, original);
  assert.equal(retirement, "absent", "the surviving Host must authenticate retirement of the original owner");
  process.stderr.write(`browser exit evidence: entire original browser closed; same owned profile reopened without reinstall; durable cleanup journal survived and pairing was absent; new pairing/share accepted; original retirement=${retirement}; old effect=0\n`);
  env.setStep("original-host-occupancy-must-recover");
  await env.idle();
  await env.enqueue("Count", "click", {});
  const reply = await env.result(); assert(reply.ok); assert.equal(reply.outcome.status, "applied");
  assert.equal(await env.fixture.evaluate(() => clicks), 1); await env.idle();
  env.pass();
}
