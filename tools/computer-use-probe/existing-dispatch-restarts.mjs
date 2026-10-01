import assert from "node:assert/strict";
import { attachNativeWorker } from "./extension-native-worker.mjs";

// Real lifecycle evidence is Chrome's native stopped -> running, not Playwright facade identity.
export async function restart({ context, popup, rpc, until, getWorker, setWorker, setStep }, afterStop, { hostIdle = false } = {}) {
  setStep("terminate-real-worker");
  const workerUrl = getWorker().url();
  await getWorker().dispose?.();
  const cdp = await context.newCDPSession(popup);
  let attached = false;
  try {
    const { targetInfos } = await cdp.send("Target.getTargets");
    assert(targetInfos.some(info => info.type === "service_worker" && info.url === workerUrl));
    let nativeVersion; let stopped = false;
    cdp.on("ServiceWorker.workerVersionUpdated", ({ versions }) => {
      for (const version of versions) {
        if (version.scriptURL !== workerUrl || (nativeVersion && version.versionId !== nativeVersion.versionId)) continue;
        nativeVersion = version;
        if (version.runningStatus === "stopped") stopped = true;
      }
    });
    await cdp.send("ServiceWorker.enable");
    await until(() => nativeVersion?.runningStatus === "running");
    stopped = false;
    await cdp.send("ServiceWorker.stopWorker", { versionId: nativeVersion.versionId });
    await until(() => stopped);
    assert.equal((await rpc("dispatch-state")).idle, hostIdle, "stopping a worker cannot change Host occupancy");
    const saved = await popup.evaluate(async () => {
      const value = (await chrome.storage.local.get("cuActionCleanup")).cuActionCleanup;
      return { count: value?.entries?.length, phase: value?.entries?.[0]?.phase };
    });
    assert.deepEqual(saved, { count: 1, phase: "prepared" });
    await afterStop?.();
    setStep("wake-new-worker");
    await popup.reload();
    const status = await popup.evaluate(() => chrome.runtime.sendMessage({ type: "cu-status" }));
    assert(status.ok && status.paired === false, "restart must not restore authority");
    await until(() => nativeVersion?.runningStatus === "running");
    const after = (await cdp.send("Target.getTargets")).targetInfos;
    const target = after.find(info => info.type === "service_worker" && info.url === workerUrl);
    assert(target);
    setStep("attach-new-worker-context");
    setWorker(await attachNativeWorker(cdp, target.targetId, workerUrl)); attached = true;
    assert(await getWorker().evaluate(() => !globalThis.__cuDispatchFaults), "old worker heap must be gone");
  } finally { if (!attached) await cdp.detach(); }
}

async function documentState(popup, tabId) {
  return popup.evaluate(async tabId => {
    const [row] = await chrome.scripting.executeScript({ target: { tabId: Number(tabId), frameIds: [0] }, world: "ISOLATED",
      func: () => ({ retired: globalThis.__grokComputerUseSnapshot?.retired === true,
        active: !!globalThis.__grokComputerUseSnapshot?.operation }) });
    return row.result;
  }, tabId);
}

async function replayObservation(popup, old) {
  return popup.evaluate(async old => {
    const { observeDocument } = await import(chrome.runtime.getURL("observe-document.mjs"));
    const rows = await chrome.scripting.executeScript({ target: old.target, world: "ISOLATED", func: observeDocument, args: old.args });
    const [state] = await chrome.scripting.executeScript({ target: old.target, world: "ISOLATED",
      func: () => ({ epoch: globalThis.__grokComputerUseExecution?.epoch, closed: globalThis.__grokComputerUseExecution?.closed === true,
        snapshotId: globalThis.__grokComputerUseSnapshot?.snapshotId, retired: globalThis.__grokComputerUseSnapshot?.retired === true }) });
    return { observationReturned: !!rows[0]?.result?.snapshotId, state: state.result };
  }, old);
}

export async function verifyWorkerRestarts(env) {
  const { popup, fixture, rpc, until, result, idle, enqueue, stats, shareCurrent, pairAndShare,
    installFaults, getTabId, getWorker, setStep, setStage, pass } = env;
  const cases = [
    ["claimed-wait", null, null],
    ["before-claim", "delayCleanupWrite", "releaseCleanupWrite"],
    ["held-claim", "delayClaim", "releaseClaim"],
    ["before-injection", "beforeInjection", "releaseInjection"],
    ["applied-click", "delayScripts", "releaseAction"],
    ["reloaded-document", null, null, "reload"],
    ["navigated-document", null, null, "navigate"],
    ["closed-tab", null, null, "close"],
  ];
  let expectedClicks = await fixture.evaluate(() => clicks);
  let previousObservation;
  for (let index = 0; index < cases.length; index++) {
    const [name, flag, held, replacement] = cases[index];
    setStage("worker-restart-recovers-" + name); setStep("pair-share");
    if (index === 0) await shareCurrent();
    else { await installFaults(); await pairAndShare(); }
    if (previousObservation) {
      setStep("new-share-rejects-old-epoch");
      const late = await replayObservation(popup, previousObservation);
      assert.equal(late.observationReturned, false); assert.equal(late.state.closed, false);
      assert(late.state.epoch > previousObservation.args[2].epoch, "new worker epoch must increase across actual restart");
    }
    const before = await stats();
    assert.equal(await fixture.evaluate(() => clicks), expectedClicks, "prior action must not reappear after pairing");
    if (flag) await getWorker().evaluate(flag => { globalThis.__cuDispatchFaults[flag] = true; }, flag);
    setStep("enqueue-and-hold");
    if (!flag) {
      await enqueue("Ready", "wait", { nameEquals: "Never", timeoutMs: 10000 });
      await until(async () => (await documentState(popup, getTabId())).active);
    } else {
      await enqueue("Count", "click", {});
      await until(() => getWorker().evaluate(held => typeof globalThis.__cuDispatchFaults[held] === "function", held));
      assert.equal((await documentState(popup, getTabId())).active, false);
    }
    if (name === "applied-click") expectedClicks++;
    assert.equal(await fixture.evaluate(() => clicks), expectedClicks);
    const claimDelta = name === "before-claim" ? 0 : 1;
    assert.equal((await stats()).claims, before.claims + claimDelta);
    assert.equal((await stats()).claimsAccepted, before.claimsAccepted + claimDelta);
    assert.equal((await stats()).results, before.results);
    assert.equal((await rpc("dispatch-state")).idle, false);
    const oldObservation = await getWorker().evaluate(() => globalThis.__cuDispatchFaults.lastObservation);
    assert(oldObservation?.args[2]?.epoch > 0);
    await restart(env, replacement ? async () => {
      setStep("replace-original-document");
      if (replacement === "reload") await fixture.reload();
      else if (replacement === "navigate") await fixture.goto(fixture.url() + "?replacement");
      else await fixture.close();
    } : undefined);
    setStep("old-authority-retired");
    await until(async () => !(await rpc("status")).stored);
    const reply = await result(); assert.equal(reply.ok, false);
    setStep("recovered-physical-occupancy");
    await idle();
    setStep("cleanup-storage-acknowledged");
    // Host releases first; the subsequent session-storage deletion has its own acknowledgement.
    await until(() => popup.evaluate(async () => !(await chrome.storage.local.get("cuActionCleanup")).cuActionCleanup));
    if (replacement) {
      previousObservation = undefined; expectedClicks = 0;
      if (replacement === "close") assert(fixture.isClosed());
      else {
        assert(!fixture.isClosed()); assert.equal(await fixture.evaluate(() => clicks), 0);
        const newDocument = await popup.evaluate(async tabId => {
          const [row] = await chrome.scripting.executeScript({ target: { tabId: Number(tabId), frameIds: [0] },
            world: "ISOLATED", func: () => true }); return row.documentId;
        }, getTabId());
        assert.notEqual(newDocument, oldObservation.target.documentIds[0]);
      }
      pass(); continue;
    }
    setStep("original-document-retired");
    assert.deepEqual(await documentState(popup, getTabId()), { retired: true, active: false });
    setStep("late-old-observation-rejected");
    const late = await replayObservation(popup, oldObservation);
    assert.equal(late.observationReturned, false);
    assert.deepEqual(late.state, { epoch: oldObservation.args[2].epoch, closed: true, snapshotId: oldObservation.args[0], retired: true });
    previousObservation = oldObservation;
    setStep("page-effect-and-survival");
    assert(!fixture.isClosed()); assert.equal(await fixture.evaluate(() => clicks), expectedClicks);
    pass();
  }
}
