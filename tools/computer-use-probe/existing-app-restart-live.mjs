import assert from "node:assert/strict";
import { restart } from "./existing-dispatch-restarts.mjs";

const cases = [
  ["held-claim", "delayClaim", "releaseClaim"],
  ["before-claim", "delayCleanupWrite", "releaseCleanupWrite"],
  ["before-injection", "beforeInjection", "releaseInjection"],
  ["claimed-wait", "delayScripts", "releaseAction"],
  ["applied-click", "delayScripts", "releaseAction"],
];

export async function saved(popup) {
  return popup.evaluate(async () => (await chrome.storage.local.get("cuActionCleanup")).cuActionCleanup?.entries?.[0]);
}

export async function oldAuthorityRejected(popup, original, endpoint, originalStatus = "unreachable") {
  const denied = await popup.evaluate(async ({ original, endpoint }) => {
    const { instanceId, connectionNonce, generation, sessionKey } = original.connection;
    const send = (base, route, body, token) => fetch(base + route, {
      method: "POST", credentials: "omit", cache: "no-store", redirect: "error",
      headers: { "content-type": "application/json", ...(token ? { authorization: "Bearer " + token } : {}) },
      body: JSON.stringify(body), signal: AbortSignal.timeout(3000),
    }).then(response => response.status, () => "unreachable");
    return {
      oldEndpoint: await send(original.record.endpoint, "/cu/extension-completion/retirement", original.record.proof),
      newProof: await send(endpoint, "/cu/extension-completion/retirement", original.record.proof),
      newConnection: await send(endpoint, "/cu/extension-status", { instanceId, connectionNonce, generation }, sessionKey),
    };
  }, { original, endpoint });
  assert.deepEqual(denied, { oldEndpoint: originalStatus, newProof: 403, newConnection: 403 });
}

async function recordPhysicalTransitions(popup, requestId) {
  await popup.evaluate(requestId => {
    globalThis.__hostRestartPhysical = false;
    if (globalThis.__hostRestartListener) chrome.storage.onChanged.removeListener(globalThis.__hostRestartListener);
    globalThis.__hostRestartListener = (changes, area) => {
        if (area === "local" && changes.cuActionCleanup?.newValue?.entries?.some(record =>
        record.proof.binding.requestId === requestId && record.phase === "physicallySettled")) {
        globalThis.__hostRestartPhysical = true;
      }
    };
    chrome.storage.onChanged.addListener(globalThis.__hostRestartListener);
  }, requestId);
}

// The actual product Host is killed while its separately owned browser/document survives.
// Private packets and journal credentials never enter the sanitized stage/evidence output.
export async function verifyAppRestart(env) {
  const { popup, fixture, rpc, until, getWorker, getTabId, shareCurrent, setStage, setStep, pass } = env;
  let expectedClicks = 0;
  for (const order of ["app", "app-then-worker", "worker-then-app"]) {
    for (const [name, flag, held] of cases) {
      setStage(`${order}-restart-recovers-${name}`); setStep("hold-original-action");
      const before = await env.stats();
      await getWorker().evaluate(flag => {
        const f = globalThis.__cuDispatchFaults;
        for (const key of ["releaseClaim", "releaseCleanupWrite", "releaseInjection", "releaseAction", "releaseCancel"]) delete f[key];
        f[flag] = true;
      }, flag);
      if (name === "claimed-wait") {
        await env.enqueue("Ready", "wait", { nameEquals: "Never", timeoutMs: 10000 });
        await until(() => popup.evaluate(async tabId => {
          const [row] = await chrome.scripting.executeScript({ target: { tabId: Number(tabId), frameIds: [0] }, world: "ISOLATED",
            func: () => !!globalThis.__grokComputerUseSnapshot?.operation });
          return row.result;
        }, getTabId()));
      } else {
        await env.enqueue("Count", "click", {});
        await until(() => getWorker().evaluate(held => typeof globalThis.__cuDispatchFaults[held] === "function", held));
      }
      if (name === "applied-click") expectedClicks++;
      assert.equal(await fixture.evaluate(() => clicks), expectedClicks);
      const original = await popup.evaluate(async () => {
        const { cuActionCleanup } = await chrome.storage.local.get("cuActionCleanup");
        const { cuPairing } = await chrome.storage.session.get("cuPairing");
        return { record: cuActionCleanup.entries[0], connection: cuPairing };
      });
      assert.equal(original.record.phase, "prepared"); assert(original.record.hostLifetime);
      assert.equal((await rpc("dispatch-state")).idle, false);
      const counts = await env.stats(); const claimed = name === "before-claim" ? 0 : 1;
      assert.equal(counts.claims, before.claims + claimed);
      assert.equal(counts.claimsAccepted, before.claimsAccepted + claimed);
      assert.equal(counts.results, before.results);

      const killHost = async () => {
        setStep("kill-original-host-process");
        const replaced = await rpc("host-restart");
        assert(replaced.ok && replaced.pid !== replaced.previousPid);
        assert.equal(original.record.hostLifetime.pid, replaced.previousPid);
        assert.equal((await rpc("dispatch-state")).idle, true, "new registry cannot prove old physical completion");
      };
      if (order === "worker-then-app") await restart(env, killHost);
      else {
        await killHost();
        if (order === "app-then-worker") await restart(env, undefined, { hostIdle: true });
      }
      assert(!fixture.isClosed()); assert.equal(await fixture.evaluate(() => clicks), expectedClicks);
      assert.equal((await rpc("status")).stored, false);
      if (order !== "app") await env.installFaults();

      setStep("old-endpoint-and-old-proof-rejected");
      const challenge = await rpc("begin");
      assert.notEqual(challenge.endpoint, original.record.endpoint);
      assert((await rpc("confirm", { nonce: challenge.nonce })).ok);
      await oldAuthorityRejected(popup, original, challenge.endpoint);
      // No new pairing means no current authority for the process-retirement query.
      const retained = await saved(popup);
      assert(retained); assert.deepEqual(retained.proof, original.record.proof);
      assert.deepEqual(retained.hostLifetime, original.record.hostLifetime);
      assert.equal((await rpc("shared")).length, 0);
      if (order === "app") {
        assert.equal(retained.phase, "prepared");
        await recordPhysicalTransitions(popup, original.record.proof.binding.requestId);
      }

      setStep("pair-new-host");
      const paired = await popup.evaluate(({ endpoint, code }) => chrome.runtime.sendMessage({ type: "cu-pair", endpoint, code }), challenge);
      assert(paired.ok && paired.paired);
      if (order === "app") {
        setStep("new-pairing-cannot-bypass-original-promises");
        const blocked = await popup.evaluate(tabId => chrome.runtime.sendMessage({ type: "cu-share-current", tabId: Number(tabId) }), getTabId());
        assert.equal(blocked.ok, false); assert.equal(blocked.error, "busy");
        assert.deepEqual(await saved(popup), original.record);
        assert.equal((await rpc("shared")).length, 0);
        setStep("join-original-native-and-cancel-replies");
        if (flag === "delayScripts") {
          await until(() => getWorker().evaluate(() => typeof globalThis.__cuDispatchFaults.releaseAction === "function"
            && typeof globalThis.__cuDispatchFaults.releaseCancel === "function"));
          await getWorker().evaluate(() => globalThis.__cuDispatchFaults.releaseCancel());
          assert.equal((await saved(popup)).phase, "prepared", "cancel reply alone cannot release the original act promise");
        }
        await getWorker().evaluate(({ held, flag }) => {
          globalThis.__cuDispatchFaults[flag] = false; globalThis.__cuDispatchFaults[held]();
        }, { held, flag });
        await until(() => popup.evaluate(() => globalThis.__hostRestartPhysical));
        assert.equal((await env.stats()).results, before.results, "old business result cannot reach the new connection");
      }
      setStep("physically-finished-old-instance-releases-journal");
      await until(async () => !await saved(popup));
      assert.equal(await fixture.evaluate(() => clicks), expectedClicks);
      await until(() => getWorker().evaluate(() => globalThis.__cuDispatchFaults.negotiated));
      await shareCurrent();
      await env.enqueue("Count", "click", {});
      const reply = await env.result(); assert(reply.ok); assert.equal(reply.outcome.status, "applied");
      expectedClicks++;
      assert.equal(await fixture.evaluate(() => clicks), expectedClicks); await env.idle();
      await until(async () => !await saved(popup));
      assert(!fixture.isClosed());
      process.stderr.write(`restart evidence: order=${order}; cut=${name}; oldEffect=${name === "applied-click" ? 1 : 0}; newEffect=1; oldAuthority=rejected; journal=cleared; tab=alive\n`);
      pass();
    }
  }
}
