// Real installed-source sw.js owns dispatch. Private pipe only supplies fixture consent/admission.
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { createInterface } from "node:readline";
import { readFile, realpath } from "node:fs/promises";
import { createServer } from "node:http";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { EXTENSION_ID } from "../computer-use-extension/pairing-client.mjs";
import { verifyWorkerRestarts } from "./existing-dispatch-restarts.mjs";
import { verifyExecutionClock } from "./existing-execution-clock.mjs";
import { verifyLiveController, verifyRetainedDocument } from "./existing-document-lifetime-live.mjs";
import { verifyTombstoneRetirement } from "./existing-retirement-live.mjs";
import { verifyAppRestart } from "./existing-app-restart-live.mjs";
import { verifyHostRetirement } from "./existing-host-retirement-live.mjs";
import { verifyBrowserExit } from "./existing-browser-exit-live.mjs";
import { OwnedCdpBrowser } from "./owned-cdp-browser.mjs";

const require = createRequire(new URL("../computer-use-browser/package.json", import.meta.url));
const { chromium } = require("playwright-core");
const args = process.argv.slice(2); const option = key => args[args.indexOf(key) + 1];
const bfcache = args.includes("--bfcache");
const appRestart = args.includes("--app-restart");
const hostRetirement = args.includes("--host-retirement");
const browserExit = args.includes("--browser-exit");
assert(args.includes("--app-host") && args.includes("--owned-profile"));
const lines = createInterface({ input: process.stdin }); let pending;
lines.on("line", line => { const resolve = pending; pending = null; resolve?.(JSON.parse(line)); });
async function rpc(command, extra = {}) {
  assert(!pending, "private fixture controls must be serial"); let timer;
  const reply = new Promise((resolve, reject) => {
    pending = resolve; timer = setTimeout(() => { pending = null; reject(new Error("fixture control timeout")); }, 15000);
  });
  process.stdout.write(JSON.stringify({ command, ...extra }) + "\n");
  try { return await reply; } finally { clearTimeout(timer); }
}
async function until(predicate, timeout = 12000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) { if (await predicate()) return; await delay(25); }
  throw new Error("fixture postcondition timeout");
}
const productionWorker = context => context.serviceWorkers().find(candidate => candidate.url() === `chrome-extension://${EXTENSION_ID}/sw.js`)
  || context.waitForEvent("serviceworker", { timeout: 10000,
    predicate: candidate => candidate.url() === `chrome-extension://${EXTENSION_ID}/sw.js` });
assert((await rpc("ready")).ok);
const profile = await realpath(option("--owned-profile"));
const owner = JSON.parse(await readFile(path.join(path.dirname(profile), "owner.json"), "utf8"));
assert.equal(owner.owner, option("--profile-owner")); assert.equal(owner.purpose, "cu-pairing-probe");
assert.equal(await realpath(owner.profile), profile);

const server = createServer((_request, response) => {
  response.writeHead(200, { "content-type": "text/html; charset=utf-8" });
  response.end(`<!doctype html><title>Dispatch owned fixture</title><style>button,input{display:block;margin:20px}</style>
<button id="clicker">Count</button><input id="field" aria-label="Field"><button id="waiting">Ready</button>
<script>window.clicks=0;document.getElementById('clicker').onclick=()=>window.clicks++;</script>`);
});
let context; let popup; let fixture; let worker; let nativeBrowser; let attachedBrowser;
let extensionInstalled = false; let stage = "launch"; let step = "start"; const passed = [];
const pass = () => { passed.push(stage); process.stderr.write(`dispatch live: ${stage} PASS\n`); };
const setStep = name => {
  step = name;
  if (browserExit) process.stderr.write(`browser-exit phase: ${stage}/${name}\n`);
};
try {
  await new Promise((resolve, reject) => { server.once("error", reject); server.listen(0, "127.0.0.1", resolve); });
  const url = `http://127.0.0.1:${server.address().port}/fixture`;
  const extension = fileURLToPath(new URL("../computer-use-extension", import.meta.url));
  const launchContext = async () => {
    if (browserExit) {
      setStep("native-browser-launch");
      nativeBrowser = new OwnedCdpBrowser(option("--browser"), profile);
      await nativeBrowser.send("Browser.getVersion");
      if (!extensionInstalled) {
        setStep("native-install-once");
        const installed = await nativeBrowser.send("Extensions.loadUnpacked", { path: extension });
        assert.equal(installed.id, EXTENSION_ID); extensionInstalled = true;
      }
      setStep("attach-owned-browser-context");
      attachedBrowser = await chromium.connectOverCDP(await nativeBrowser.playwrightEndpoint(), { timeout: 10000 });
      const [ownedContext] = attachedBrowser.contexts();
      assert(ownedContext, "owned native browser context missing");
      return ownedContext;
    }
    return chromium.launchPersistentContext(profile, {
    executablePath: option("--browser"), headless: true, viewport: { width: 1000, height: 700 }, locale: "en-US",
    ignoreDefaultArgs: bfcache ? ["--disable-back-forward-cache"] : [],
    args: [...(browserExit ? [] : ["--headless=new"]), "--disable-background-networking", "--disable-features=DisableLoadExtensionCommandLineSwitch",
      "--disable-extensions-except=" + extension, "--load-extension=" + extension],
    });
  };
  context = await launchContext();
  context.setDefaultTimeout(10000);
  step = "find-production-extension-worker";
  worker = await productionWorker(context);
  step = "verify-production-worker-identity";
  assert(worker.url().endsWith(`://${EXTENSION_ID}/sw.js`));
  popup = await context.newPage(); await popup.goto(`chrome-extension://${EXTENSION_ID}/popup.html`);
  stage = "execution-clock-real-transactions";
  await verifyExecutionClock(popup); pass();
  // Faults wrap native operations on the actual worker. No replacement transport or injected capability.
  async function installFaults() { await worker.evaluate(() => {
    const faults = { loseClaim: false, loseSettle: false, loseResult: false, losePoll: false, delayClaim: false, delayScripts: false,
      failCleanupWrite: false, failedCleanupWrites: 0, delayCleanupWrite: false, beforeInjection: false,
      replayNext: false, negotiated: false, claims: 0, claimsAccepted: 0, results: 0, settlements: 0,
      updates: 0, unoffers: 0, unoffered: 0, retirements: 0, last: null };
    faults.hostRetirements = 0; faults.hostStates = []; faults.lostHostReplies = 0; faults.failedCleanupRemoves = 0;
    faults.preparation = [];
    chrome.tabs.onUpdated.addListener((_id, change) => {
      if (change.status === "loading" || typeof change.url === "string") faults.updates++;
    });
    const originalFetch = globalThis.fetch.bind(globalThis);
    globalThis.fetch = async (url, init) => {
      const route = new URL(url).pathname;
      if (new URL(url).origin === faults.unreachableCompletion
        && ["/cu/extension-completion/settle", "/cu/extension-completion/retirement"].includes(route)) {
        throw new Error("owned fixture original completion endpoint unavailable");
      }
      if (route === "/cu/extension-actions/claim") faults.claims++;
      if (route === "/cu/extension-actions/result") faults.results++;
      if (route === "/cu/extension-completion/settle") faults.settlements++;
      if (route === "/cu/extension-completion/retirement") {
        faults.retirements++;
        if (faults.delayRetirement) {
          faults.delayRetirement = false;
          await new Promise(resolve => { faults.releaseRetirement = resolve; });
        }
      }
      if (route === "/cu/tab-unoffer") faults.unoffers++;
      const response = await originalFetch(url, init);
      if (route === "/cu/extension-completion/host-lifetime") {
        faults.preparation.push({ phase: "host-lifetime", status: response.status });
        if (faults.preparation.length > 16) faults.preparation.shift();
      }
      if (route === "/cu/extension-completion/host-retirement" && response.ok) {
        const { state } = await response.clone().json();
        faults.hostRetirements++; faults.hostStates.push(state);
        if (faults.hostStates.length > 16) faults.hostStates.shift();
        if (state === "retired" && faults.loseHostRetirement) {
          faults.loseHostRetirement = false; faults.lostHostReplies++;
          await response.arrayBuffer(); throw new Error("owned fixture Host retirement reply lost");
        }
        if (state === "retired" && faults.holdHostRetirement) {
          faults.holdHostRetirement = false;
          await new Promise(resolve => { faults.releaseHostRetirement = resolve; });
        }
      }
      if (route === "/cu/extension-actions/claim" && response.ok) faults.claimsAccepted++;
      if (route === "/cu/tab-unoffer" && response.ok) faults.unoffered++;
      if (route === "/cu/extension-actions/negotiate" && response.ok) faults.negotiated = true;
      if (route === "/cu/extension-actions/poll" && response.ok) {
        const body = await response.clone().json();
        if (body.dispatch) {
          faults.last = body.dispatch;
          if (faults.losePoll) {
            faults.losePoll = false; await response.arrayBuffer(); throw new Error("owned fixture poll loss");
          }
        }
        else if (faults.replayNext && faults.last) {
          faults.replayNext = false; await response.arrayBuffer();
          return Response.json({ ok: true, dispatch: faults.last });
        }
      }
      if (route === "/cu/extension-actions/claim" && faults.delayClaim) {
        faults.delayClaim = false; await new Promise(resolve => { faults.releaseClaim = resolve; });
      }
      for (const [path, name] of [["/cu/extension-actions/claim", "loseClaim"],
        ["/cu/extension-completion/settle", "loseSettle"], ["/cu/extension-actions/result", "loseResult"]]) {
        if (route === path && faults[name]) { faults[name] = false; await response.arrayBuffer(); throw new Error("owned fixture reply loss"); }
      }
      return response;
    };
    const execute = chrome.scripting.executeScript.bind(chrome.scripting);
    const getFrame = chrome.webNavigation.getFrame.bind(chrome.webNavigation);
    chrome.webNavigation.getFrame = async details => {
      if (faults.holdLifetime && details.tabId === undefined) {
        faults.holdLifetime = false; await new Promise(resolve => { faults.releaseLifetime = resolve; });
      }
      const result = await getFrame(details);
      if (details.tabId !== undefined) {
        faults.preparation.push({ phase: "attest-document", exists: !!result,
          documentMatches: result?.documentId === details.documentId,
          outermost: result?.parentFrameId === -1 && result?.frameType === "outermost_frame",
          active: result?.documentLifecycle === "active", errorFree: result?.errorOccurred === false });
        if (faults.preparation.length > 16) faults.preparation.shift();
      }
      return result;
    };
    const storeLocal = chrome.storage.local.set.bind(chrome.storage.local);
    const removeLocal = chrome.storage.local.remove.bind(chrome.storage.local);
    chrome.storage.local.remove = async keys => {
      if (faults.failCleanupRemove && (Array.isArray(keys) ? keys : [keys]).includes("cuActionCleanup")) {
        faults.failCleanupRemove = false; faults.failedCleanupRemoves++;
        throw new Error("owned fixture cleanup deletion failed");
      }
      return removeLocal(keys);
    };
    chrome.storage.local.set = async value => {
      if (faults.failCleanupWrite && Object.hasOwn(value, "cuActionCleanup")) {
        faults.failCleanupWrite = false; faults.failedCleanupWrites++; throw new Error("owned fixture storage failure");
      }
      await storeLocal(value);
      if (Object.hasOwn(value, "cuActionCleanup")) {
        faults.preparation.push({ phase: "store-cleanup", success: true });
        if (faults.preparation.length > 16) faults.preparation.shift();
      }
      if (faults.delayCleanupWrite && Object.hasOwn(value, "cuActionCleanup")) {
        faults.delayCleanupWrite = false; await new Promise(resolve => { faults.releaseCleanupWrite = resolve; });
      }
    };
    chrome.scripting.executeScript = async request => {
      if (request.func.name === "observeDocument") faults.lastObservation = { target: request.target, args: request.args };
      if (faults.delayBeforeCancel && request.func.name === "cancelDocumentOperation") {
        await new Promise(resolve => { faults.releaseBeforeCancel = resolve; });
      }
      if (faults.beforeInjection && request.func.name === "actDocument") {
        await new Promise(resolve => { faults.releaseInjection = resolve; });
      }
      let rows;
      try { rows = await execute(request); }
      catch (error) {
        if (request.func.name === "armDocumentCompletion") faults.preparation.push({ phase: "arm-document", failed: true });
        throw error;
      }
      if (request.func.name === "armDocumentCompletion") {
        faults.preparation.push({ phase: "arm-document", armed: rows?.length === 1 && rows[0].result?.status === "armed",
          rowCount: rows?.length, guardUnavailable: rows?.some(row => String(row.error?.message ?? row.error ?? "").includes("completionGuardUnavailable")) });
        if (faults.preparation.length > 16) faults.preparation.shift();
      }
      if (faults.loseActionReply && request.func.name === "actDocument") {
        faults.loseActionReply = false; throw new Error("owned fixture native reply loss");
      }
      if (faults.loseCancelReply && request.func.name === "cancelDocumentOperation") {
        faults.loseCancelReply = false; throw new Error("owned fixture cancellation reply loss");
      }
      if (faults.delayScripts && request.func.name === "actDocument") await new Promise(resolve => { faults.releaseAction = resolve; });
      if (faults.delayScripts && request.func.name === "cancelDocumentOperation") await new Promise(resolve => { faults.releaseCancel = resolve; });
      return rows;
    };
    globalThis.__cuDispatchFaults = faults;
  }); }
  await installFaults();
  async function openStableFixture() {
    fixture = await context.newPage();
    await fixture.setViewportSize({ width: 1000, height: 700 });
    await fixture.goto(url); await fixture.bringToFront();
    // CDP attachment can otherwise return before Chrome delivers its initial
    // native resize events. Drain rendering before creating any model snapshot;
    // never suppress product lifecycle listeners or replay a rejected action.
    await fixture.waitForFunction(() => document.readyState === "complete" && document.hasFocus()
      && innerWidth === 1000 && innerHeight === 700, undefined, { timeout: 10000 });
    await fixture.evaluate(() => new Promise(resolve => {
      requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
    }));
  }
  await openStableFixture();
  let selector; let tabId;
  async function pairAndShare() {
    await worker.evaluate(() => { globalThis.__cuDispatchFaults.negotiated = false; });
    const challenge = await rpc("begin"); assert((await rpc("confirm", { nonce: challenge.nonce })).ok);
    const pairing = await popup.evaluate(({ endpoint, code }) => chrome.runtime.sendMessage({ type: "cu-pair", endpoint, code }), challenge);
    assert(pairing.ok && pairing.paired);
    await until(() => worker.evaluate(() => globalThis.__cuDispatchFaults.negotiated));
    await shareCurrent();
  }
  async function shareCurrent() {
    const state = await popup.evaluate(() => chrome.runtime.sendMessage({ type: "cu-tab-state" }));
    assert(state.ok && state.paired); tabId = String(state.tabId);
    if (!state.shared) {
      const shared = await popup.evaluate(tabId => chrome.runtime.sendMessage({ type: "cu-share-current", tabId }), state.tabId);
      assert(shared.ok && shared.shared);
    }
    const candidates = await rpc("shared"); assert.equal(candidates.length, 1); selector = candidates[0].id;
  }
  const stats = () => worker.evaluate(() => {
    const f = globalThis.__cuDispatchFaults;
    return { claims: f.claims, claimsAccepted: f.claimsAccepted, results: f.results,
      settlements: f.settlements, retirements: f.retirements };
  });
  async function enqueue(name, action, parameters) {
    const { observation } = await rpc("dispatch-observe", { selector });
    const elementRef = observation.nodes.find(node => node.name === name)?.elementRef;
    assert(elementRef, "fixture ref missing");
    const reply = await rpc("dispatch-enqueue", { tabId,
      action: { kind: "act", snapshotId: observation.snapshotId, elementRef, action, parameters } });
    assert(reply.queued);
  }
  async function result() {
    let value;
    await until(async () => { value = await rpc("dispatch-result"); return !value.pending; });
    return value;
  }
  const idle = () => until(async () => (await rpc("dispatch-state")).idle);
  const environment = { context, popup, fixture, rpc, until, result, idle, enqueue, stats,
    shareCurrent, pairAndShare, installFaults, getTabId: () => tabId, getWorker: () => worker,
    setWorker: value => { worker = value; }, setStep, setStage: value => { stage = value; }, pass };
  environment.restartBrowser = async () => {
    setStep("dispose-old-worker-handle");
    await worker?.dispose?.();
    if (nativeBrowser) {
      setStep("detach-old-playwright-connection");
      await attachedBrowser?.close(); attachedBrowser = null;
      setStep("close-owned-browser-process");
      await nativeBrowser.close(); nativeBrowser = null;
    } else await context.close();
    assert(fixture.isClosed() && popup.isClosed(), "the whole original browser must exit");
    context = await launchContext(); context.setDefaultTimeout(10000);
    setStep("find-restarted-production-worker");
    worker = await productionWorker(context);
    assert(worker.url().endsWith(`://${EXTENSION_ID}/sw.js`));
    setStep("open-restarted-popup-and-fixture");
    popup = await context.newPage(); await popup.goto(`chrome-extension://${EXTENSION_ID}/popup.html`);
    await openStableFixture();
    Object.assign(environment, { context, popup, fixture });
    setStep("attach-restarted-native-operation-counters");
    await installFaults();
  };
  stage = "production-sw-negotiates";
  await pairAndShare(); pass();
  if (browserExit) await verifyBrowserExit(environment);
  else if (hostRetirement) await verifyHostRetirement(environment);
  else if (appRestart) await verifyAppRestart(environment);
  else if (bfcache) await verifyRetainedDocument(environment);
  else {
    stage = "queued-click-once";
    await enqueue("Count", "click", {});
    let reply = await result(); assert(reply.ok); assert.equal(reply.outcome.status, "applied");
    assert.equal(await fixture.evaluate(() => clicks), 1); await idle();
    assert.equal((await stats()).claims, 1); pass();
    stage = "maximum-unicode-packet";
    const text = "中文🙂🚀".repeat(1000);
    await enqueue("Field", "set_value", { text });
    reply = await result(); assert(reply.ok); assert.equal(reply.outcome.status, "verified");
    assert.equal(await fixture.locator("#field").inputValue(), text); await idle(); pass();
    stage = "lost-claim-zero-effect";
    let before = await stats();
    await worker.evaluate(() => { globalThis.__cuDispatchFaults.loseClaim = true; });
    await enqueue("Count", "click", {});
    reply = await result(); assert(reply.ok); assert.equal(reply.outcome.status, "rejected");
    assert.equal(await fixture.evaluate(() => clicks), 1); assert.equal((await stats()).claims, before.claims + 1);
    await idle(); pass();
    stage = "lost-settle-cleanup-only";
    before = await stats();
    await worker.evaluate(() => { globalThis.__cuDispatchFaults.loseSettle = true; });
    await enqueue("Count", "click", {});
    reply = await result(); assert(reply.ok); assert.equal(reply.outcome.status, "applied");
    await until(async () => (await stats()).retirements >= before.retirements + 1);
    assert.equal((await stats()).settlements, before.settlements + 1);
    assert.equal((await stats()).retirements, before.retirements + 1);
    await until(() => popup.evaluate(async () => !(await chrome.storage.local.get("cuActionCleanup")).cuActionCleanup));
    assert.equal((await stats()).claims, before.claims + 1);
    assert.equal(await fixture.evaluate(() => clicks), 2); await idle(); pass();
    stage = "lost-business-result-no-replay";
    before = await stats();
    await worker.evaluate(() => { globalThis.__cuDispatchFaults.loseResult = true; });
    await enqueue("Count", "click", {});
    reply = await result(); assert(reply.ok); assert.equal(reply.outcome.status, "applied");
    await delay(600);
    assert.equal((await stats()).results, before.results + 1); assert.equal((await stats()).claims, before.claims + 1);
    assert.equal(await fixture.evaluate(() => clicks), 3); await idle(); pass();
    stage = "host-cancel-real-wait";
    await enqueue("Ready", "wait", { nameEquals: "Never", timeoutMs: 10000 });
    await until(() => worker.evaluate(async tabId => {
      const [row] = await chrome.scripting.executeScript({ target: { tabId: Number(tabId), frameIds: [0] }, world: "ISOLATED",
        func: () => !!globalThis.__grokComputerUseSnapshot?.operation }); return row.result;
    }, tabId));
    assert((await rpc("dispatch-cancel")).ok);
    reply = await result(); assert.equal(reply.ok, false); await idle(); assert(!fixture.isClosed()); pass();
    stage = "unpair-joins-both-scripts";
    await worker.evaluate(() => { globalThis.__cuDispatchFaults.delayScripts = true; });
    await enqueue("Count", "click", {});
    await until(() => worker.evaluate(() => typeof globalThis.__cuDispatchFaults.releaseAction === "function"));
    assert.equal(await fixture.evaluate(() => clicks), 4);
    const forgot = await popup.evaluate(() => chrome.runtime.sendMessage({ type: "cu-forget" })); assert(forgot.ok);
    await until(() => worker.evaluate(() => typeof globalThis.__cuDispatchFaults.releaseCancel === "function"));
    assert.equal((await rpc("status")).stored, false);
    reply = await result(); assert.equal(reply.ok, false);
    assert.equal((await rpc("dispatch-state")).idle, false);
    await worker.evaluate(() => globalThis.__cuDispatchFaults.releaseCancel());
    assert.equal((await rpc("dispatch-state")).idle, false, "cancel reply alone is not completion");
    await worker.evaluate(() => globalThis.__cuDispatchFaults.releaseAction());
    await idle(); assert(!fixture.isClosed()); pass();
    stage = "duplicate-poll-retires-without-effect";
    await worker.evaluate(() => { const f = globalThis.__cuDispatchFaults; f.delayScripts = false; f.negotiated = false; f.last = null; });
    await pairAndShare();
    await enqueue("Count", "click", {}); reply = await result(); assert(reply.ok);
    assert.equal(await fixture.evaluate(() => clicks), 5); before = await stats();
    await worker.evaluate(() => { globalThis.__cuDispatchFaults.replayNext = true; });
    await until(async () => !(await rpc("status")).stored);
    assert.equal((await stats()).claims, before.claims); assert.equal(await fixture.evaluate(() => clicks), 5);
    assert(!fixture.isClosed()); pass();
    stage = "lost-poll-retires-without-effect";
    await pairAndShare(); before = await stats();
    await worker.evaluate(() => { globalThis.__cuDispatchFaults.losePoll = true; });
    await enqueue("Count", "click", {});
    reply = await result(); assert.equal(reply.ok, false);
    await until(async () => !(await rpc("status")).stored); await idle();
    assert.equal((await stats()).claims, before.claims); assert.equal(await fixture.evaluate(() => clicks), 5);
    assert(!fixture.isClosed()); pass();
    stage = "navigation-during-claim-has-zero-effect";
    step = "pair-share";
    await pairAndShare(); before = await stats();
    await worker.evaluate(() => { globalThis.__cuDispatchFaults.delayClaim = true; });
    step = "enqueue";
    await enqueue("Count", "click", {});
    step = "claim-reply-held";
    await until(() => worker.evaluate(() => typeof globalThis.__cuDispatchFaults.releaseClaim === "function"));
    assert.equal(await fixture.evaluate(() => clicks), 5);
    step = "navigate";
    await fixture.goto(url + "?new-document");
    step = "candidate-retired";
    await until(async () => (await rpc("shared")).length === 0);
    step = "business-cancelled";
    reply = await result(); assert.equal(reply.ok, false);
    assert.equal((await rpc("dispatch-state")).idle, false, "accepted claim remains occupied before its reply");
    await worker.evaluate(() => globalThis.__cuDispatchFaults.releaseClaim());
    step = "physically-settled";
    await idle();
    assert.equal((await stats()).claims, before.claims + 1); assert.equal(await fixture.evaluate(() => clicks), 0);
    assert(!fixture.isClosed()); pass();
    stage = "cleanup-storage-failure-is-zero-claim";
    await shareCurrent();
    before = await stats();
    await worker.evaluate(() => { globalThis.__cuDispatchFaults.failCleanupWrite = true; });
    await enqueue("Count", "click", {}); reply = await result();
    assert(reply.ok); assert.equal(reply.outcome.status, "rejected"); await idle();
    assert.equal((await stats()).claims, before.claims);
    assert.equal(await worker.evaluate(() => globalThis.__cuDispatchFaults.failedCleanupWrites), 1);
    assert.equal(await fixture.evaluate(() => clicks), 0); pass();
    await verifyLiveController(environment);
    await verifyTombstoneRetirement(environment);
    await verifyWorkerRestarts(environment);
  }
  process.stdout.write(JSON.stringify({ event: "passed", checks: passed }) + "\n");
} catch (error) {
  process.stderr.write(`dispatch live failed at ${stage}/${step}; assertion=${error?.code === "ERR_ASSERTION"}, timeout=${/timeout/i.test(String(error?.message))}, ownedExitPending=${error?.message === "owned browser did not exit"}\n`);
  const diagnostic = await worker?.evaluate(() => {
    const f = globalThis.__cuDispatchFaults;
    return f ? { claims: f.claims, updates: f.updates, unoffers: f.unoffers, unoffered: f.unoffered,
      heldClaim: typeof f.releaseClaim === "function", settlements: f.settlements,
      preparation: f.preparation?.slice(-16) } : { unavailable: true };
  }).catch(() => ({ unavailable: true }));
  process.stderr.write(`dispatch diagnostic: ${JSON.stringify(diagnostic)}\n`);
  if (stage.startsWith("worker-restart-") && popup && context) {
    const diagnosticSession = await context.newCDPSession(popup).catch(() => null);
    const targets = await diagnosticSession?.send("Target.getTargets").catch(() => null);
    process.stderr.write(`restart targets: ${JSON.stringify({ nativeWorkers: targets?.targetInfos.filter(info => info.type === "service_worker").length,
      playwrightWorkers: context.serviceWorkers().length })}\n`);
    await diagnosticSession?.detach().catch(() => {});
  }
  process.stdout.write(JSON.stringify({ event: "failed", check: stage }) + "\n"); process.exitCode = 1;
} finally {
  await worker?.evaluate(() => {
    const f = globalThis.__cuDispatchFaults; f?.releaseAction?.(); f?.releaseCancel?.(); f?.releaseClaim?.(); f?.releaseInjection?.();
    f?.releaseCleanupWrite?.(); f?.releaseLifetime?.(); f?.releaseBeforeCancel?.();
    f?.releaseRetirement?.();
    f?.releaseHostRetirement?.();
    if (f) f.last = null;
  }).catch(() => {});
  await popup?.evaluate(() => chrome.runtime.sendMessage({ type: "cu-forget" })).catch(() => {});
  await worker?.dispose?.().catch(() => {});
  if (nativeBrowser) {
    await attachedBrowser?.close().catch(() => {});
    await nativeBrowser.close();
  } else await context?.close().catch(() => {});
  await new Promise(resolve => server.close(resolve)); lines.close();
}
