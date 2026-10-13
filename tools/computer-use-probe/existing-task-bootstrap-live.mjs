// Exact owned source registration/version; native stopped -> popup -> running.
// No Pair/Share, synthetic onStartup, authority, clock change or worker restart loop.
import assert from "node:assert/strict";
import { EXTENSION_ID } from "../computer-use-extension/pairing-client.mjs";
import { attachNativeWorker } from "./extension-native-worker.mjs";

const sourceRoot = `chrome-extension://${EXTENSION_ID}/`;
const sourceWorker = sourceRoot + "sw.js";

// A Playwright facade can outlive its stopped native worker. Open the real
// source popup first, then attach only the exact running registration/version.
export async function openSourceJointWorker({ context, until, publishBrowser,
  evidence, phase, onWorkerClosed = () => {} }) {
  let session, attached;
  let selectedVersion;
  const registrations = new Map();
  const versions = new Map();
  try {
    evidence.initialWorkerCount = context.serviceWorkers().filter(row => row.url() === sourceWorker).length;
    assert(evidence.initialWorkerCount <= 1, "source worker facades must be unique");
    phase("popup-create");
    const popup = await context.newPage();
    publishBrowser({ popup });
    phase("source-native-session");
    session = await context.newCDPSession(popup);
    session.on("ServiceWorker.workerRegistrationUpdated", ({ registrations: rows }) => {
      for (const row of rows) {
        if (row.scopeURL === sourceRoot) registrations.set(row.registrationId, row);
      }
    });
    session.on("ServiceWorker.workerVersionUpdated", ({ versions: rows }) => {
      for (const row of rows) {
        if (row.scriptURL !== sourceWorker) continue;
        versions.set(row.versionId, row);
        if (row.versionId === selectedVersion
          && (row.runningStatus === "stopped" || row.status === "redundant")) {
          attached?.markWorkerStopped();
          onWorkerClosed();
        }
      }
    });
    await session.send("ServiceWorker.enable");
    phase("popup-navigate");
    await popup.goto(sourceRoot + "popup.html");
    evidence.sourcePopupOpened = true;
    phase("version-query");
    const version = await session.send("Browser.getVersion");
    assert(Number(version.product.match(/\/(\d+)\./)?.[1]) >= 140);
    phase("native-source-worker-ready");
    await until(() => {
      const source = [...versions.values()].filter(row => row.status === "activated"
        && row.runningStatus === "running" && registrations.get(row.registrationId)?.isDeleted === false);
      assert(source.length <= 1, "running source registration/version must be unique");
      if (source.length !== 1) return false;
      selectedVersion = source[0].versionId;
      return true;
    });
    const native = versions.get(selectedVersion);
    phase("native-source-target-match");
    const sourceTargets = (await session.send("Target.getTargets")).targetInfos
      .filter(row => row.type === "service_worker" && row.url === sourceWorker);
    evidence.nativeSourceTargetCount = sourceTargets.length;
    assert.equal(sourceTargets.length, 1);
    assert.equal(sourceTargets[0].targetId, native.targetId);
    evidence.sourceRegistrationMatched = true;
    phase("native-source-worker-attach");
    attached = await attachNativeWorker(session, native.targetId, sourceWorker, { connectionEvidence: true });
    phase("trusted-source-api");
    const apis = await attached.evaluate(expected => ({
      installedIdMatches: chrome.runtime.id === expected,
      execute: typeof chrome.scripting?.executeScript === "function",
      capture: typeof chrome.tabs?.captureVisibleTab === "function",
      trustedStorage: typeof chrome.storage?.local?.setAccessLevel === "function",
    }), EXTENSION_ID);
    assert.deepEqual(apis, { installedIdMatches: true, execute: true, capture: true, trustedStorage: true });
    assert.equal(versions.get(selectedVersion).runningStatus, "running");
    evidence.sourceWorkerMatched = true;
    evidence.installedSourceMatched = true;
    phase("source-popup-unpaired");
    await until(async () => await popup.locator("#status").getAttribute("data-state") === "unpaired");
    publishBrowser({ worker: attached });
    return { popup, worker: attached };
  } catch (error) {
    if (attached) await attached.dispose();
    else await session?.detach();
    throw error;
  }
}

export async function verifyDormantBootstrap({ context, rpc, until, publishBrowser,
  evidence, pass, phase }) {
  let session, attached;
  const control = await context.newPage();
  const registrations = new Map();
  const versions = new Map();
  let originalVersion, originalRegistration;
  try {
    phase("native-source-registration-and-install-match");
    session = await context.newCDPSession(control);
    session.on("ServiceWorker.workerRegistrationUpdated", ({ registrations: rows }) => {
      for (const row of rows) {
        if (row.scopeURL === sourceRoot) registrations.set(row.registrationId, row);
      }
    });
    session.on("ServiceWorker.workerVersionUpdated", ({ versions: rows }) => {
      for (const row of rows) {
        if (row.scriptURL === sourceWorker) versions.set(row.versionId, row);
      }
    });
    await session.send("ServiceWorker.enable");
    await until(() => {
      const rows = [...versions.values()].filter(row => row.status === "activated"
        && registrations.get(row.registrationId)?.isDeleted === false);
      assert(rows.length <= 1, "source registration/version must be unique");
      if (rows.length !== 1) return false;
      originalVersion = rows[0].versionId;
      originalRegistration = rows[0].registrationId;
      return true;
    });
    evidence.registrationMatched = true;
    evidence.activatedSourceVersions = 1;
    pass("native-source-registration-and-install-match");

    phase("native-original-worker-physically-stopped");
    const original = () => versions.get(originalVersion);
    assert.equal(original().registrationId, originalRegistration);
    assert(["running", "stopped"].includes(original().runningStatus));
    evidence.initiallyStopped = original().runningStatus === "stopped";
    evidence.stopIssued = !evidence.initiallyStopped;
    if (evidence.stopIssued) await session.send("ServiceWorker.stopWorker", { versionId: originalVersion });
    await until(() => original()?.runningStatus === "stopped");
    evidence.nativeStopped = true;
    pass("native-original-worker-physically-stopped");

    phase("old-start-order-has-no-running-source-worker");
    // Target presence and a cached Playwright Worker are not native liveness.
    // Inspect only this owned source URL, never arbitrary targets or storage.
    const targets = (await session.send("Target.getTargets")).targetInfos;
    evidence.sourceTargetsWhileStopped = targets.filter(row => row.type === "service_worker"
      && row.url === sourceWorker).length;
    evidence.playwrightSourceFacadesWhileStopped = context.serviceWorkers()
      .filter(row => row.url() === sourceWorker).length;
    assert(evidence.sourceTargetsWhileStopped <= 1);
    assert(evidence.playwrightSourceFacadesWhileStopped <= 1);
    assert.equal(original().runningStatus, "stopped");
    assert.equal([...versions.values()].filter(row => row.registrationId === originalRegistration
      && row.runningStatus === "running").length, 0);
    const before = await rpc("before-request");
    assert.equal(before.state.featureEnabled, false);
    assert.equal(before.state.request, null);
    assert.equal(before.state.grants.length, 0);
    assert.equal(before.state.executions.length, 0);
    assert.equal(original().runningStatus, "stopped");
    evidence.noRunningWorkerBeforePopup = true;
    pass("old-start-order-has-no-running-source-worker");

    phase("real-source-popup-wakes-worker-without-authority");
    const opened = await openSourceJointWorker({ context, until, publishBrowser, evidence,
      phase: current => { evidence.popupPhase = current; } });
    const { popup } = opened;
    attached = opened.worker;
    // popup.js itself sends cu-status. Do not manufacture an extra wake message.
    await until(() => original()?.runningStatus === "running");
    assert.equal(original().registrationId, originalRegistration);
    const runningTargets = (await session.send("Target.getTargets")).targetInfos
      .filter(row => row.type === "service_worker" && row.url === sourceWorker);
    assert.equal(runningTargets.length, 1);
    const source = await attached.evaluate(async expected => {
      const marker = (await chrome.storage.local.get("cuBrowserSession")).cuBrowserSession;
      return {
        installedIdMatches: chrome.runtime.id === expected,
        trustedStorage: typeof chrome.storage.local.setAccessLevel === "function",
        markerPresent: typeof marker?.id === "string",
        started: marker?.started,
        noPreviousBrowser: marker?.previousId === null,
      };
    }, EXTENSION_ID);
    assert.deepEqual(source, { installedIdMatches: true, trustedStorage: true,
      markerPresent: true, started: false, noPreviousBrowser: true });
    await until(async () => await popup.locator("#status").getAttribute("data-state") === "unpaired");
    const after = await rpc("before-request");
    assert.deepEqual(after.state, before.state);
    assert.equal((await rpc("shared-count")).count, 0);
    evidence.nativeRunningAfterPopup = true;
    evidence.originalVersionAndRegistrationRetained = true;
    evidence.popupUnpaired = true;
    evidence.browserStartupNotInvented = true;
    evidence.noRequestOrAuthority = true;
    pass("real-source-popup-wakes-worker-without-authority");
  } finally {
    if (attached) {
      await attached.dispose();
      publishBrowser({ worker: undefined });
    }
    await session?.detach();
    await control.close();
  }
}
