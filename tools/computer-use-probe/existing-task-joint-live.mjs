// Real source MV3 + authenticated HTTP + original App picker + NEW TaskGrant/MCP.
// Popup/Host decisions and ACP peer are synthetic. Late capture uses owned CDP action only.
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { createInterface } from "node:readline";
import { readFile, realpath, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { createServer } from "node:http";
import { pinnedChromeLaunchPath } from "../computer-use-browser/immutable-chrome.mjs";
import { EXTENSION_ID } from "../computer-use-extension/pairing-client.mjs";
import { sessionClient } from "./existing-mcp-live.mjs";
import { pauseOwnedWorker } from "./existing-task-worker-pause.mjs";
import { verifyOriginalHostRecovery } from "./existing-task-recovery-live.mjs";
import { recoveryTerminalScenarios } from "./existing-task-recovery-terminal-live.mjs";
import { recoveryOwnerScenarios } from "./existing-task-recovery-owner-live.mjs";
import { launchOwnedJointBrowser, verifyBrowserReplacement } from "./existing-task-browser-live.mjs";
import { instrumentJointWorker } from "./existing-task-worker-diagnostics.mjs";
import { preparePreapprovalLease } from "./existing-task-preapproval-lease.mjs";
import { ownerReplacements, replaceOriginalOwner, verifyRejectedOwner, verifyDeniedOwnerInput } from "./existing-task-owner-replacement.mjs";
import { openSourceJointWorker, verifyDormantBootstrap } from "./existing-task-bootstrap-live.mjs";
import { JointPhaseEvidence } from "./existing-task-phase-evidence.mjs";
import { terminalScenarios, verifyOriginalTerminal } from "./existing-task-terminals-live.mjs";
import { reservationTerminalScenarios, verifyReservationTerminal } from "./existing-task-reservation-terminals-live.mjs";
import { publicationTerminalScenarios, verifyPublicationTerminal } from "./existing-task-publication-terminals-live.mjs";
import { verifyOriginalChatStop } from "./existing-task-chat-stop-live.mjs";
import { nativeEvaluationFailure } from "./extension-native-worker.mjs";
import { lateActionScenarios, verifyOriginalLateAction } from "./existing-task-late-actions-live.mjs";
import { captureScenario, verifyOriginalCaptureDenial } from "./existing-task-capture-live.mjs";
import { lateCaptureScenarios, verifyOriginalLateCapture } from "./existing-task-late-capture-live.mjs";
import { captureSuccessScenario, verifyOriginalCaptureSuccess } from "./existing-task-capture-success-live.mjs";

const require = createRequire(new URL("../computer-use-browser/package.json", import.meta.url));
const { chromium } = require("playwright-core");
const root = fileURLToPath(new URL("../../", import.meta.url));
const args = process.argv.slice(2);
const option = key => {
  assert(args.includes(key), "explicit private fixture argument required");
  return args[args.indexOf(key) + 1];
};
const scenario = option("--scenario");
const browserScenarios = ["active-browser-reopen", "active-profile-replacement", "active-profile-tab-id-collision"];
const browserLifecycle = browserScenarios.includes(scenario);
const dormantBootstrap = scenario === "startup-dormant";
const recoveryConcurrent = scenario === "active-host-resume-concurrent";
const recoveryTerminal = recoveryTerminalScenarios.includes(scenario);
const recoveryOwner = recoveryOwnerScenarios.includes(scenario);
const lateAction = lateActionScenarios.includes(scenario);
const captureDenial = scenario === captureScenario;
const lateCapture = lateCaptureScenarios.includes(scenario);
const captureSuccess = scenario === captureSuccessScenario;
const preapprovalReplacements = ["choose-connection-repair", "reservation-connection-repair", "publication-connection-repair"];
const initiallyEnabled = preapprovalReplacements.includes(scenario);
const preapprovalLeases = ["choose-lease-expiry", "reservation-lease-expiry", "publication-lease-expiry"];
assert(captureSuccess || lateCapture || captureDenial || lateAction || recoveryOwner || recoveryConcurrent || recoveryTerminal || ["success", "startup-dormant", "active-connection-repair", "active-lease-expiry", "active-host-resume", "active-chat-stop-queued", ...browserScenarios, ...preapprovalReplacements, ...preapprovalLeases, ...ownerReplacements, ...terminalScenarios, ...reservationTerminalScenarios, ...publicationTerminalScenarios, "choose-unshare", "choose-reshare", "choose-reload", "choose-forget", "publication-forget", "reservation-forget", "publication-active-unshare", "publication-inactive-unshare", "reservation-active-unshare", "reservation-inactive-unshare", "discovery-active-unshare", "discovery-inactive-unshare"].includes(scenario), "fixed private joint scenario required");
const activeLifecycle = ["active-connection-repair", "active-lease-expiry", "active-host-resume", "active-chat-stop-queued"].includes(scenario) || browserLifecycle || recoveryConcurrent || recoveryTerminal || recoveryOwner || lateAction || captureDenial || lateCapture || captureSuccess;
let pending;
const lines = createInterface({ input: process.stdin });
lines.on("line", line => {
  if (!pending) return;
  const resolve = pending; pending = null; resolve(JSON.parse(line));
});
async function rpc(command, extra = {}) {
  assert(!pending, "private Host commands must be serial");
  let timer;
  const reply = new Promise((resolve, reject) => {
    pending = resolve;
    timer = setTimeout(() => { pending = null; reject(new Error("private fixture deadline")); }, 18000);
  });
  process.stdout.write(JSON.stringify({ command, ...extra }) + "\n");
  try { return await reply; } finally { clearTimeout(timer); }
}
const until = async predicate => {
  const deadline = performance.now() + 15000;
  while (performance.now() < deadline) { if (await predicate()) return; await delay(50); }
  throw new Error("native fixture postcondition deadline");
};
const decode = reply => {
  assert.equal(reply.isError, false, "actual new-grant MCP tool must succeed");
  return JSON.parse(reply.content.find(item => item.type === "text").text);
};
let context, client, other, popup, worker, pausedWorker, nativeBrowser, attachedBrowser;
const publishBrowser = current => {
  if ("context" in current) context = current.context;
  if ("popup" in current) popup = current.popup;
  if ("worker" in current) worker = current.worker;
  if ("nativeBrowser" in current) nativeBrowser = current.nativeBrowser;
  if ("attachedBrowser" in current) attachedBrowser = current.attachedBrowser;
};
let stage = "ownership-ready";
let detail = "start";
const checks = [];
const memberActionEvidence = [];
const memberDomEvidence = [];
const startupEvidence = { phase: "not-started", contextClosed: false, initialWorkerCount: 0, workerEvents: 0, sourceWorkerMatched: false, originalWorkerClosed: false, workerClosedAtPhase: null, elapsedMs: 0 };
const replacementEvidence = { phase: "not-started" };
const bootstrapEvidence = { phase: "not-started" };
let preapprovalLeaseEvidence;
let ownerReplacementEvidence;
let ownerInputEvidence;
let originalObservationEvidence;
let phaseEvidence;
let terminalEvidence;
let chatStopEvidence;
let recoveryRaceEvidence;
let recoveryTerminalEvidence;
let recoveryOwnerEvidence;
let lateActionEvidence;
let captureEvidence;
const startupStarted = performance.now();
const startupPhase = phase => {
  detail = `startup-${phase}`;
  startupEvidence.phase = phase;
  startupEvidence.elapsedMs = Math.round(performance.now() - startupStarted);
  phaseEvidence?.note(phase);
};
let memberActionPhase = null;
const outcomeKind = value => ["applied", "verified", "rejected", "unknown"].includes(value) ? value : "other";
let mcpRequests = 0;
const pass = name => { checks.push(name); stage = name; };
const reportPassed = () => process.stdout.write(JSON.stringify({ event: "passed", scenario, checks,
  bootstrapEvidence,
  preapprovalLeaseEvidence,
  ownerReplacementEvidence,
  ownerInputEvidence,
  originalObservationEvidence,
  terminalEvidence,
  chatStopEvidence,
  recoveryRaceEvidence,
  recoveryTerminalEvidence,
  recoveryOwnerEvidence,
  lateActionEvidence,
  captureEvidence,
  connectionEvidence: worker?.connectionEvidence(),
  syntheticHostDecisions: true, syntheticAcpPeer: true, syntheticRequest: false, mcpRequests, legacyGrant: false }) + "\n");
const fixtureServer = createServer((request, response) => {
  const member = request.url === "/member-b" ? "B" : request.url === "/member-c" ? "C" : "A";
  const suffix = member === "A" ? "" : ` ${member}`;
  response.writeHead(200, { "content-type": "text/html; charset=utf-8" });
  response.end(`<title>CU New TaskGrant Fixture${suffix}</title><main><p>Owned visible fixture 你好; member ${member}</p>
    <button id="action">Fixture action</button><input id="input" aria-label="Fixture input">
    <textarea id="scroll" aria-label="Fixture scroll" style="height:100px;width:150px;overflow-y:scroll">${"Scroll content\n".repeat(100)}</textarea>
    <script>globalThis.clicks=0;document.querySelector('#action').onclick=()=>{globalThis.clicks++};
    globalThis.textEvents={beforeinput:0,input:0,change:0,inputType:null};
    for(const type of ['beforeinput','input','change'])document.querySelector('#input').addEventListener(type,event=>{
      globalThis.textEvents[type]++;if(type==='beforeinput')globalThis.textEvents.inputType=event.inputType;
    });</script></main>`);
});
let profile;
async function runJoint() {
try {
  assert.equal((await rpc("ready")).ok, true);
  stage = "profile-owner-validation";
  profile = await realpath(option("--owned-profile"));
  const owner = JSON.parse(await readFile(path.join(path.dirname(profile), "owner.json"), "utf8"));
  assert.equal(owner.owner, option("--profile-owner"));
  assert.equal(owner.purpose, "cu-pairing-probe");
  assert.equal(await realpath(owner.profile), profile);
  phaseEvidence = new JointPhaseEvidence(profile);
  await new Promise((resolve, reject) => {
    fixtureServer.once("error", reject); fixtureServer.listen(0, "127.0.0.1", resolve);
  });
  const extension = path.join(root, "tools/computer-use-extension");
  stage = "owned-chrome-launch";
  startupPhase("context-launch");
  context = browserLifecycle || dormantBootstrap || lateCapture || captureSuccess
    ? await launchOwnedJointBrowser({ browser: option("--browser"), profile, chromium, extension,
      install: true, publishBrowser, phase: startupPhase })
    : await chromium.launchPersistentContext(profile, {
    executablePath: pinnedChromeLaunchPath(option("--browser"), path.join(profile, ".chrome-exec")),
    headless: true, locale: "en-US",
    args: ["--headless=new", "--lang=en-US", "--disable-background-networking",
      "--disable-features=DisableLoadExtensionCommandLineSwitch", "--disable-extensions-except=" + extension, "--load-extension=" + extension],
  });
  context.on("close", () => { startupEvidence.contextClosed = true; });
  context.on("serviceworker", () => { startupEvidence.workerEvents = Math.min(8, startupEvidence.workerEvents + 1); });
  if (dormantBootstrap) {
    stage = "source-dormant-bootstrap";
    await verifyDormantBootstrap({ context, rpc, until, publishBrowser, evidence: bootstrapEvidence,
      pass, phase: current => { detail = current; bootstrapEvidence.phase = current;
        phaseEvidence.note(current); } });
    startupPhase("complete");
    reportPassed(); return;
  }
  stage = "source-mv3-start";
  ({ worker, popup } = await openSourceJointWorker({ context, until, publishBrowser,
    evidence: startupEvidence, phase: startupPhase, onWorkerClosed: () => {
      startupEvidence.originalWorkerClosed = true;
      startupEvidence.workerClosedAtPhase = startupEvidence.phase;
    } }));
  startupPhase("complete");
  stage = "source-mv3-instrument";
  phaseEvidence.note("source-instrument");
  await instrumentJointWorker(worker);
  stage = "owned-fixture-open";
  phaseEvidence.note("owned-fixture-open");
  const openMember = async (member, route) => {
    const note = step => { detail = `owned-fixture-${member.toLowerCase()}-${step}`; phaseEvidence.note(detail); };
    note("create"); const page = await context.newPage();
    note("navigate"); await page.goto(`http://127.0.0.1:${fixtureServer.address().port}/${route}`);
    note("focus"); await page.bringToFront();
    note("tab-query");
    const id = await worker.evaluate(async () => (await chrome.tabs.query({ active: true, lastFocusedWindow: true }))[0].id);
    return { member, page, id };
  };
  const { page: fixture, id: fixtureId } = await openMember("A", "fixture");
  const siblings = [];
  for (const member of ["B", "C"]) {
    siblings.push(await openMember(member, `member-${member.toLowerCase()}`));
  }
  const [memberB, memberC] = siblings;
  detail = "owned-fixture-a-focus"; phaseEvidence.note(detail);
  await fixture.bringToFront();
  stage = "original-mcp-binding";
  client = sessionClient(await rpc("mcp-binding"));
  await client.rpc("initialize");
  const catalog = await client.rpc("tools/list");
  assert(catalog.tools.some(tool => tool.name === "computer_request_access"));
  assert(!catalog.tools.some(tool => /approve|authorize|reconnect|resume/.test(tool.name)));
  const call = async (name, args = {}) => {
    if (name === "computer_request_access" && args.action === "request") mcpRequests++;
    await delay(120); return client.call(name, args);
  };
  stage = initiallyEnabled ? "request-enabled-no-model-authority" : "request-off-no-model-read";
  const before = await rpc("before-request");
  assert.equal(before.state.request, null);
  assert.equal(before.state.featureEnabled, initiallyEnabled);
  assert.equal(before.state.grants.length, 0);
  assert.equal(before.state.executions.length, 0);
  assert.equal((await call("computer_observe", { screenshot: false })).isError, true);
  assert.equal((await call("computer_list_targets")).isError, true);
  // Page DOM runs independently while the original MV3 worker is paused.
  const zeroPageEffects = async () => {
    for (const page of [fixture, memberB.page, memberC.page]) {
      assert.equal(await page.evaluate(() => globalThis.clicks), 0);
      assert.equal(await page.locator("#input").inputValue(), "");
      assert.equal(await page.locator("#scroll").evaluate(element => element.scrollTop), 0);
    }
  };
  const zeroPageReads = async () => {
    await zeroPageEffects();
    const calls = await worker.evaluate(() => ({ calls: globalThis.__jointCalls, captures: globalThis.__jointCaptures }));
    assert.equal(calls.captures, 0);
    assert(!calls.calls.some(row => ["observeDocument", "actDocument"].includes(row.function)), "no model content/input before App publication");
  };
  await zeroPageReads(); pass(stage);

  stage = "real-mcp-first-request-no-authority";
  const requested = decode(await call("computer_request_access", {
    action: "request", purpose: "Read and edit the owned fixture",
    surfaces: ["existing-tabs"], capabilities: ["observe", "input"], scopeIntent: "owned fixture",
  }));
  assert.equal(requested.state, "pending");
  assert.equal(requested.authority, "none");
  assert.equal(mcpRequests, 1);
  // Pending ends model dispatch. Only the synthetic Host runs preparation below.
  const initial = await rpc("begin");
  assert.equal(initial.state.featureEnabled, initiallyEnabled);
  assert.equal(initial.state.grants.length, 0);
  assert.equal(initial.state.executions.length, 0);
  assert.equal(initial.state.request.key.requestId, requested.requestId);
  assert.equal(initial.state.request.key.revision, requested.revision);
  const preparation = initial.preparation;
  assert.equal(preparation.ready, false);
  const challenge = preparation.pairing;
  assert.equal(challenge.installedExtensionId, EXTENSION_ID);
  await zeroPageReads(); pass(stage);

  stage = "original-app-confirm-required";
  const pair = async (current = challenge) => {
    phaseEvidence.note("pair-fill");
    await popup.locator("#endpoint").fill(current.endpoint);
    await popup.locator("#code").fill(current.verificationCode);
    phaseEvidence.note("pair-click");
    await popup.locator("#pair").click();
    phaseEvidence.note("pair-ui-ready");
    await until(async () => await popup.locator("#pair").isEnabled());
  };
  await pair();
  detail = "source-popup-unconfirmed-pair-rejected";
  phaseEvidence.note(detail);
  await until(async () => await popup.locator("#status").getAttribute("data-state") === "pairingRejected");
  assert.equal((await rpc("preparation")).pairingConfirmed, false);
  assert.equal((await rpc("confirm", { nonce: "foreign-nonce" })).ok, false);
  assert.equal((await rpc("confirm", { nonce: challenge.nonce })).ok, true);
  await pair();
  detail = "source-popup-original-pair-confirmed";
  phaseEvidence.note(detail);
  await until(async () => await popup.locator("#status").getAttribute("data-state") === "paired");
  await zeroPageReads(); pass(stage);

  const share = async (page = fixture, title = "CU New TaskGrant Fixture", count = 1, preparing = true) => {
    detail = "share-focus";
    phaseEvidence.note(detail, false, worker?.connectionEvidence());
    await page.bringToFront();
    detail = "share-ui-ready";
    phaseEvidence.note(detail, false, worker?.connectionEvidence());
    await until(async () => await popup.locator("#tab-title").textContent() === title);
    await until(async () => await popup.locator("#share-current").isEnabled());
    detail = "share-click";
    phaseEvidence.note(detail, false, worker?.connectionEvidence());
    await popup.locator("#share-current").click();
    detail = "share-host-ready";
    phaseEvidence.note(detail, false, worker?.connectionEvidence());
    if (preparing) await until(async () => (await rpc("preparation")).ready);
    await until(async () => (await rpc("shared-count")).count === count);
    phaseEvidence.note("share-confirmed", false, worker?.connectionEvidence());
  };
  stage = "real-unshare-reshare-original-picker";
  await share();
  detail = "unshare-ui-ready";
  phaseEvidence.note(detail);
  await until(async () => await popup.locator("#unshare-current").isEnabled());
  detail = "unshare-click";
  phaseEvidence.note(detail);
  await popup.locator("#unshare-current").click();
  detail = "unshare-host-ready";
  phaseEvidence.note(detail);
  await until(async () => !(await rpc("preparation")).ready);
  phaseEvidence.note("unshare-confirmed");
  assert.equal((await rpc("load")).ok, false);
  await share();
  await share(memberB.page, "CU New TaskGrant Fixture B", 2);
  await share(memberC.page, "CU New TaskGrant Fixture C", 3);
  if (publicationTerminalScenarios.includes(scenario)) {
    terminalEvidence = await verifyPublicationTerminal({ scenario, rpc, call, worker, initial, until,
      members: [{ member: "A", page: fixture, id: fixtureId }, memberB, memberC], zeroPageReads, pass,
      phase: (next, nextDetail = next) => { stage = next; detail = nextDetail;
        phaseEvidence.note(nextDetail, false, worker?.connectionEvidence()); } });
    reportPassed(); return;
  }
  if (reservationTerminalScenarios.includes(scenario)) {
    terminalEvidence = await verifyReservationTerminal({ scenario, rpc, call, worker, initial, until,
      members: [{ member: "A", page: fixture, id: fixtureId }, memberB, memberC], zeroPageReads, pass,
      phase: (next, nextDetail = next) => { stage = next; detail = nextDetail; phaseEvidence.note(nextDetail); } });
    reportPassed(); return;
  }
  if (terminalScenarios.includes(scenario)) {
    terminalEvidence = await verifyOriginalTerminal({ scenario, rpc, call, worker, initial, until,
      members: [{ member: "A", page: fixture, id: fixtureId }, memberB, memberC], zeroPageReads, pass,
      phase: (next, nextDetail = next) => { stage = next; detail = nextDetail; phaseEvidence.note(nextDetail); } });
    reportPassed(); return;
  }
  if (scenario.startsWith("discovery-")) {
    await zeroPageReads(); pass(stage);
    stage = "original-owned-discovery-unshare-cannot-return-stale-receipt";
    const inactive = scenario.endsWith("inactive-unshare");
    const revokedPage = inactive ? memberB.page : fixture;
    const revokedTitle = inactive ? "CU New TaskGrant Fixture B" : "CU New TaskGrant Fixture";
    // Prepare focus/UI before the five-second production discovery budget.
    await revokedPage.bringToFront();
    await until(async () => await popup.locator("#tab-title").textContent() === revokedTitle);
    await until(async () => await popup.locator("#unshare-current").isEnabled());
    assert.equal((await rpc("start-discovery")).started, true);
    await until(async () => (await rpc("discovery-ready")).ready);
    const pendingDiscovery = await rpc("discovery-ready");
    assert.equal(pendingDiscovery.originalCandidates, 3);
    assert.equal(pendingDiscovery.discoveryWorkers, 1);
    assert.deepEqual(pendingDiscovery.state.request.key, initial.state.request.key);
    assert.equal(pendingDiscovery.state.featureEnabled, false);
    assert.equal(pendingDiscovery.state.executions.length, 0);
    assert.equal(pendingDiscovery.state.grants.length, 0);
    await popup.locator("#unshare-current").click();
    await until(async () => (await rpc("shared-count")).count === 2);
    const rejected = await rpc("finish-discovery");
    assert.equal(rejected.rejected, true);
    assert.equal(rejected.at, "original-owned-discovery");
    assert.deepEqual(rejected.state.request.key, initial.state.request.key);
    assert.equal(rejected.state.featureEnabled, false);
    assert.equal(rejected.state.grants.length, 0);
    assert.equal(rejected.state.executions.length, 0);
    assert.equal(rejected.discoveryWorkers, 0);
    assert.equal(rejected.ownedWorkers, 0);
    assert.equal(rejected.continuations, 0);
    assert.equal(rejected.stored, false);
    await zeroPageReads(); pass(stage);
    stage = "original-task-zero-authority-unselected-share-retained";
    for (const screenshot of [false, true]) assert.equal((await call("computer_observe", { screenshot })).isError, true);
    assert.equal((await call("computer_list_targets")).isError, true);
    assert.equal((await rpc("shared-count")).count, 2);
    for (const page of [fixture, memberB.page, memberC.page]) assert.equal(page.isClosed(), false);
    await zeroPageReads(); pass(stage);
    reportPassed(); return;
  }
  const selected = await rpc("load");
  assert.equal(selected.ok, true);
  assert.equal(selected.view.pickerId, preparation.pickerId);
  assert.equal(selected.view.candidates.length, 3);
  await zeroPageReads(); pass(stage);

  stage = "new-task-grant-original-app-publication";
  // Opaque references come from the original Host picker, never model tab IDs.
  const candidates = ["CU New TaskGrant Fixture", "CU New TaskGrant Fixture B"].map(title => {
    const rows = selected.view.candidates.filter(row => row.title === title);
    assert.equal(rows.length, 1);
    return rows[0].candidateId;
  });
  if (scenario !== "success" && !activeLifecycle) {
    const publication = scenario.startsWith("publication-");
    const reservation = scenario.startsWith("reservation-");
    const connectionForget = scenario.endsWith("-forget");
    const connectionRepair = preapprovalReplacements.includes(scenario);
    const leaseExpiry = preapprovalLeases.includes(scenario);
    const ownerReplacement = ownerReplacements.includes(scenario);
    const ownerRejectionStage = publication ? "publication-owner-replacement-rejects-original-pin" : reservation ? "reservation-owner-replacement-rejects-original-worker" : "choose-owner-replacement-rejects-original-picker";
    const rejectionStage = ownerReplacement ? ownerRejectionStage : leaseExpiry ? "original-lease-expiry-rejects-old-authority" : connectionRepair ? "original-connection-repair-rejects-old-authority" : connectionForget ? "original-connection-forget-rejects-old-authority" : publication ? "prepared-publication-unshare-cannot-publish" : reservation ? "original-owned-reservation-unshare-cannot-prepare" : "old-picker-revoked-share-cannot-approve";
    stage = rejectionStage;
    const inactive = scenario.endsWith("inactive-unshare");
    const revokedPage = inactive ? memberB.page : fixture;
    const revokedTab = inactive ? memberB.id : fixtureId;
    const revokedTitle = inactive ? "CU New TaskGrant Fixture B" : "CU New TaskGrant Fixture";
    await revokedPage.bringToFront();
    const documentId = () => worker.evaluate(async tabId => (await chrome.webNavigation.getFrame({ tabId, frameId: 0 }))?.documentId, revokedTab);
    const originalDocument = await documentId();
    assert.equal(typeof originalDocument, "string");
    let lease;
    let publicationRun;
    if (leaseExpiry) {
      await zeroPageReads();
      lease = await preparePreapprovalLease({ context, popup, worker, rpc,
        registerPause: current => { pausedWorker = current; },
        phase: current => { detail = current; },
      });
      preapprovalLeaseEvidence = lease.evidence;
    }
    if (publication) {
      detail = "start-original-owned-publication";
      assert.equal((await rpc("start-publication", { candidates })).started, true);
      await until(async () => (await rpc("publication-ready")).ready);
      const prepared = await rpc("publication-ready");
      publicationRun = prepared.runId;
      assert.deepEqual(prepared.state.request.key, initial.state.request.key);
      assert.equal(prepared.state.featureEnabled, true);
      assert.equal(prepared.state.grants.length, 0);
      assert.equal(prepared.state.executions.length, 1);
      assert.equal(prepared.ownedWorkers, 1);
      assert.equal(prepared.stored, true);
      if (leaseExpiry) { await lease.entered(prepared); await zeroPageEffects(); }
      else await zeroPageReads();
    }
    if (reservation) {
      detail = "original-reservation-worker-entered";
      assert.equal((await rpc("start-reservation", { candidates })).started, true);
      await until(async () => (await rpc("reservation-ready")).ready);
      const prepared = await rpc("reservation-ready");
      assert.deepEqual(prepared.state.request.key, initial.state.request.key);
      assert.equal(prepared.state.featureEnabled, initiallyEnabled);
      assert.equal(prepared.state.grants.length, 0);
      assert.equal(prepared.state.executions.length, 0);
      assert.equal(prepared.reservationWorkers, 1);
      assert.equal(prepared.stored, initiallyEnabled);
      if (leaseExpiry) { await lease.entered(prepared); await zeroPageEffects(); }
      else await zeroPageReads();
    }
    if (ownerReplacement) {
      stage = "original-owner-replaced-with-zero-authority";
      detail = "replace-only-original-request-owner";
      ownerReplacementEvidence = await replaceOriginalOwner({ scenario, rpc, worker,
        initialKey: initial.state.request.key, publicationRun, documentId,
        originalDocument, zeroPageReads });
      pass(stage);
      stage = rejectionStage;
    } else if (leaseExpiry) {
      if (!publication && !reservation) await lease.entered();
      await lease.retire();
      await zeroPageEffects();
      pass("original-production-lease-retired-before-release");
      stage = rejectionStage;
    } else if (connectionRepair) {
      stage = "replacement-real-pair-and-shares-no-authority";
      detail = "pending-original-new-host-challenge";
      const confirmations = await worker.evaluate(() => globalThis.__jointPairingConfirmed);
      const replacement = await rpc("repair-challenge");
      assert.equal((await rpc("shared-count")).count, 0);
      await pair(replacement);
      await until(async () => await popup.locator("#status").getAttribute("data-state") === "paired");
      assert.equal(await worker.evaluate(() => globalThis.__jointPairingConfirmed), confirmations + 1);
      assert.equal(await documentId(), originalDocument);
      const noReplacementAuthority = async () => {
        const current = await rpc("lifecycle-authority");
        assert.deepEqual(current.state.request.key, initial.state.request.key);
        assert.equal(current.grants, 0);
        assert.equal(current.state.grants.length, 0);
        assert.equal(current.continuations, 0);
        assert.equal(current.ownedWorkers, publication ? 1 : 0);
        await zeroPageReads();
      };
      await noReplacementAuthority();
      await share(fixture, "CU New TaskGrant Fixture", 1, false);
      await share(memberB.page, "CU New TaskGrant Fixture B", 2, false);
      await share(memberC.page, "CU New TaskGrant Fixture C", 3, false);
      assert.equal(await documentId(), originalDocument);
      await noReplacementAuthority(); pass(stage);
      stage = rejectionStage;
    } else if (connectionForget) {
      detail = "source-popup-forget-original-connection";
      await popup.locator("#forget").click();
      await until(async () => await popup.locator("#status").getAttribute("data-state") === "unpaired");
      await until(async () => (await rpc("shared-count")).count === 0);
      const disconnects = await worker.evaluate(() => globalThis.__jointHttp.filter(row => row.route === "/cu/extension-disconnect" && row.status === 200).length);
      assert.equal(disconnects, 1, "original authenticated disconnect must complete exactly once");
      assert.equal(await documentId(), originalDocument);
    } else if (scenario === "choose-reload") {
      detail = "same-url-reload";
      const originalUrl = fixture.url();
      await fixture.reload();
      assert.equal(fixture.url(), originalUrl);
      await until(async () => (await rpc("shared-count")).count === 2);
      const replacedDocument = await documentId();
      assert.equal(typeof replacedDocument, "string");
      assert.notEqual(replacedDocument, originalDocument);
    } else {
      detail = "original-share-unshare";
      await until(async () => await popup.locator("#tab-title").textContent() === revokedTitle);
      await until(async () => await popup.locator("#unshare-current").isEnabled());
      await popup.locator("#unshare-current").click();
      await until(async () => (await rpc("shared-count")).count === 2);
      assert.equal(await documentId(), originalDocument);
    }
    if (["choose-reshare", "choose-reload"].includes(scenario)) {
      detail = "replacement-share";
      await share(fixture, "CU New TaskGrant Fixture", 3);
      if (scenario === "choose-reshare") assert.equal(await documentId(), originalDocument);
    }
    detail = "old-host-candidate-approval";
    const rejected = publication ? await rpc("finish-publication") : reservation ? await rpc("finish-reservation") : ownerReplacement ? await rpc("reject-original-picker-owner", { candidates }) : await rpc("reject-approval", { candidates });
    assert.equal(rejected.rejected, true);
    assert.equal(rejected.at, publication ? "original-publication" : reservation ? "original-owned-reservation" : ownerReplacement ? "original-app-picker" : "original-reservation");
    assert.deepEqual(rejected.state.request.key, initial.state.request.key);
    assert.equal(rejected.state.featureEnabled, initiallyEnabled);
    assert.equal(rejected.state.grants.length, 0);
    if (!publication && !ownerReplacement) assert.equal(rejected.state.executions.length, 0);
    if (publication) {
      assert.equal(rejected.stored, initiallyEnabled);
      await until(async () => (await rpc("stopped", { runId: rejected.runId })).ok);
    }
    if (reservation) {
      assert.equal(rejected.reservationWorkers, 0);
      assert.equal(rejected.stored, initiallyEnabled);
    }
    assert.equal(rejected.ownedWorkers, 0);
    assert.equal(rejected.continuations, 0);
    if (ownerReplacement) {
      await verifyRejectedOwner({ scenario, rejected, rpc, until, documentId, originalDocument, publicationRun });
      process.stderr.write(`original owner witness: scenario=${scenario}, evidence=${JSON.stringify(ownerReplacementEvidence)}\n`);
    }
    if (leaseExpiry) {
      await zeroPageEffects();
      await lease.resume();
      assert.equal(await documentId(), originalDocument);
      process.stderr.write(`preapproval natural lease witness: scenario=${scenario}, evidence=${JSON.stringify(preapprovalLeaseEvidence)}\n`);
    }
    await zeroPageReads(); pass(stage);

    stage = ownerReplacement ? "original-task-zero-authority-user-shares-retained" : connectionRepair ? "original-task-zero-authority-new-shares-retained" : connectionForget || leaseExpiry ? "original-task-zero-authority-tabs-retained" : "original-task-zero-authority-unselected-share-retained";
    assert.equal((await call("computer_observe", { screenshot: false })).isError, true);
    assert.equal((await call("computer_observe", { screenshot: true })).isError, true);
    assert.equal((await call("computer_list_targets")).isError, true);
    if (ownerReplacement) {
      detail = "original-owner-direct-input-authority-rejection";
      ownerInputEvidence = await verifyDeniedOwnerInput({ scenario, rejected, call, rpc,
        members: [{ member: "A", id: fixtureId }, memberB, memberC], zeroPageReads });
      process.stderr.write(`original owner direct input witness: scenario=${scenario}, evidence=${JSON.stringify(ownerInputEvidence)}\n`);
    }
    assert.equal((await rpc("shared-count")).count, connectionForget || leaseExpiry ? 0 : connectionRepair || ownerReplacement || ["choose-reshare", "choose-reload"].includes(scenario) ? 3 : 2);
    for (const page of [fixture, memberB.page, memberC.page]) assert.equal(page.isClosed(), false);
    await zeroPageReads(); pass(stage);
    reportPassed();
    return;
  }
  const approved = await rpc("approve", { candidates });
  assert.equal(approved.ok, true);
  assert.equal(approved.state.grants.length, 1);
  assert.equal(approved.state.executions.length, 1);
  assert.deepEqual(approved.state.grants[0].members.map(row => row.targetId).sort(),
    [`tab:${fixtureId}`, `tab:${memberB.id}`].sort());
  assert.equal(approved.outcome.binding.targetId, `tab:${fixtureId}`);
  const selectedRun = { runId: approved.outcome.runId, targetId: approved.outcome.binding.targetId };
  if (scenario === "active-chat-stop-queued") {
    chatStopEvidence = await verifyOriginalChatStop({ rpc, call, worker,
      members: [{ member: "A", id: fixtureId, page: fixture }, memberB, memberC],
      initial, approved, until, zeroPageReads, pass,
      phase: (nextStage, nextDetail = nextStage) => { stage = nextStage; detail = nextDetail;
        phaseEvidence.note(nextDetail, false, worker.connectionEvidence()); } });
    reportPassed(); return;
  }
  assert.equal((await rpc("delivery")).ok, true);
  await zeroPageReads(); pass(stage);

  stage = "new-grant-unselected-member-c-has-no-access";
  const targetC = { runId: selectedRun.runId, targetId: `tab:${memberC.id}` };
  const targets = decode(await call("computer_list_targets", { runId: selectedRun.runId }));
  assert.deepEqual(targets.map(row => row.targetId).sort(), [`tab:${fixtureId}`, `tab:${memberB.id}`].sort());
  assert.equal((await call("computer_open_target", targetC)).isError, true);
  for (const screenshot of [false, true]) {
    assert.equal((await call("computer_observe", { ...targetC, screenshot })).isError, true);
  }
  const noMemberCReadOrInput = async () => {
    assert.equal(await memberC.page.evaluate(() => globalThis.clicks), 0);
    assert.equal(await memberC.page.locator("#input").inputValue(), "");
    assert.equal(await memberC.page.locator("#scroll").evaluate(element => element.scrollTop), 0);
    const cCalls = await worker.evaluate(tabId => ({
      scripts: globalThis.__jointCalls.filter(row => row.tab === tabId && ["observeDocument", "actDocument"].includes(row.function)),
      captures: globalThis.__jointCaptures,
    }), memberC.id);
    assert.equal(cCalls.scripts.length, 0);
    assert.equal(cCalls.captures, 0);
  };
  await noMemberCReadOrInput(); pass(stage);

  stage = "new-grant-mcp-click-and-dedupe";
  await fixture.bringToFront();
  const observe = async () => decode(await call("computer_observe", { ...selectedRun, screenshot: false }));
  const identity = (observation, actionId, name) => ({ version: 1, actionId, ...selectedRun,
    targetGeneration: observation.targetGeneration, snapshotId: observation.snapshotId,
    geometryRevision: observation.geometryRevision, target: { elementRef: observation.nodes.find(row => row.name === name)?.ref } });
  if (lateCapture || captureSuccess) {
    captureEvidence = await (captureSuccess ? verifyOriginalCaptureSuccess : verifyOriginalLateCapture)({ scenario, rpc, call, decode, until, observe, identity,
      worker, popup, nativeBrowser, profile, fixture, fixtureId, memberB, memberC, selectedRun, initial, pass,
      phase: (nextStage, nextDetail = nextStage) => { stage = nextStage; detail = nextDetail;
        phaseEvidence.note(nextDetail, false, worker.connectionEvidence()); } });
    reportPassed(); return;
  }
  if (captureDenial) {
    captureEvidence = await verifyOriginalCaptureDenial({ rpc, call, decode, until, observe, identity,
      worker, fixture, fixtureId, memberB, memberC, selectedRun, initial, pass,
      phase: (nextStage, nextDetail = nextStage) => { stage = nextStage; detail = nextDetail;
        phaseEvidence.note(nextDetail, false, worker.connectionEvidence()); } });
    reportPassed(); return;
  }
  if (scenario === "active-host-resume" || recoveryConcurrent || recoveryTerminal || recoveryOwner) {
    await verifyOriginalHostRecovery({ rpc, call, decode, until, observe, identity,
      fixture, fixtureId, memberB, memberC, selectedRun, worker, initial,
      noMemberCReadOrInput, pass,
      concurrent: recoveryConcurrent, onRaceEvidence: evidence => { recoveryRaceEvidence = evidence; },
      interruption: recoveryTerminal ? (scenario === "active-host-resume-pause" ? "pause" : "chat-stop") : undefined,
      onTerminalEvidence: evidence => { recoveryTerminalEvidence = evidence; },
      ownerScenario: recoveryOwner ? scenario : undefined,
      onOwnerEvidence: evidence => { recoveryOwnerEvidence = evidence; },
      phase: (nextStage, nextDetail = nextStage) => { stage = nextStage; detail = nextDetail;
        phaseEvidence.note(nextDetail, false, worker.connectionEvidence()); } });
    reportPassed(); return;
  }
  if (lateAction) {
    lateActionEvidence = await verifyOriginalLateAction({ scenario, rpc, call, decode, until, observe, identity,
      fixture, fixtureId, memberB, memberC, selectedRun, worker, popup, initial, noMemberCReadOrInput, pass,
      phase: (nextStage, nextDetail = nextStage) => { stage = nextStage; detail = nextDetail;
        phaseEvidence.note(nextDetail, false, worker.connectionEvidence()); } });
    reportPassed(); return;
  }
  if (browserLifecycle) {
    await verifyBrowserReplacement({ scenario, rpc, call, decode, until, observe, identity,
      context, popup, worker, nativeBrowser, attachedBrowser,
      fixture, fixtureId, memberB, memberC, selectedRun, initial,
      profile, browser: option("--browser"), chromium, extension, mcpPid: client.pid,
      fixtureUrl: `http://127.0.0.1:${fixtureServer.address().port}`,
      publishBrowser,
      evidence: replacementEvidence, pass,
      phase: (nextStage, nextDetail) => { stage = nextStage; detail = nextDetail;
        phaseEvidence.note(nextDetail.replace(/^replacement-/, ""), nextDetail.startsWith("replacement-"),
          worker?.connectionEvidence()); } });
    reportPassed(); return;
  }
  if (activeLifecycle) {
    stage = "original-grant-fresh-observation-no-input";
    detail = "original-observe-request";
    const originalReply = await call("computer_observe", { ...selectedRun, screenshot: false });
    detail = "original-observe-response";
    originalObservationEvidence = {
      isError: originalReply.isError === true,
      code: ["feature_disabled", "run_not_found", "target_unauthorized", "surface_unavailable", "identity_mismatch", "dead_target", "stop_requested", "lease_held", "duplicate_action", "schema", "adapter", "timeout", "rate_limited"].includes(originalReply.code) ? originalReply.code : "other",
      textContent: originalReply.content?.some(item => item.type === "text") === true,
      imageContent: originalReply.content?.some(item => item.type === "image") === true,
    };
    const original = decode(originalReply);
    detail = "original-observe-expected-text";
    originalObservationEvidence.expectedText = typeof original.text === "string" && original.text.includes("Owned visible fixture 你好");
    assert(original.text.includes("Owned visible fixture 你好"));
    const oldClick = { ...identity(original, "old-connection-never-clicked", "Fixture action"), action: "click", parameters: {} };
    detail = "original-observe-document-identity";
    const originalDocument = await worker.evaluate(async tabId => (await chrome.webNavigation.getFrame({ tabId, frameId: 0 }))?.documentId, fixtureId);
    originalObservationEvidence.documentIdPresent = typeof originalDocument === "string";
    assert.equal(typeof originalDocument, "string");
    const noInput = async () => {
      for (const page of [fixture, memberB.page, memberC.page]) {
        assert.equal(page.isClosed(), false);
        assert.equal(await page.evaluate(() => globalThis.clicks), 0);
        assert.equal(await page.locator("#input").inputValue(), "");
        assert.equal(await page.locator("#scroll").evaluate(element => element.scrollTop), 0);
      }
      await noMemberCReadOrInput();
    };
    const readCount = () => worker.evaluate(() => globalThis.__jointCalls.filter(row => ["observeDocument", "actDocument"].includes(row.function)).length);
    const readsBefore = await readCount();
    await noInput();
    process.stderr.write(`original first observation witness: scenario=${scenario}, evidence=${JSON.stringify(originalObservationEvidence)}\n`);
    pass(stage);

    if (scenario === "active-lease-expiry") {
      stage = "original-thirty-second-lease-expires-without-disconnect";
      const lease = await rpc("lease-witness");
      assert.equal(lease.stored, true);
      assert.equal(lease.leaseMs, 30000);
      assert.equal(await worker.evaluate(() => globalThis.__jointDisconnectRequests), 0);
      detail = "pause-original-owned-worker";
      pausedWorker = await pauseOwnedWorker(context, popup, worker.url());
      const start = performance.now();
      const deadline = start + lease.leaseMs + 2000;
      detail = "original-http-maintenance-lease-expiry";
      // Poll a stored-state witness that cannot itself expire/renew the lease.
      // Each original pipe reply settles within its unchanged deadline.
      for (;;) {
        const current = await rpc("lease-witness");
        assert.equal(current.leaseMs, lease.leaseMs);
        if (!current.stored) break;
        assert(performance.now() < deadline, "original real lease maintenance did not retire connection");
        await delay(100);
      }
      const elapsed = performance.now() - start;
      assert(elapsed >= lease.leaseMs - 11000, "connection retired before its real heartbeat lease");
      detail = "resume-original-owned-worker-after-expiry";
      await pausedWorker.resume(); pausedWorker = undefined;
      assert.equal((await rpc("shared-count")).count, 0);
      process.stderr.write(`real lease witness: original worker paused; stored connection retired after ${Math.round(elapsed)}ms; product lease=${lease.leaseMs}ms; no clock mutation or disconnect before pause\n`);
      pass(stage);
    } else {
      stage = "real-source-repair-retires-original-connection";
    }
    detail = "new-host-challenge-original-source-pairing";
    const confirmations = await worker.evaluate(() => globalThis.__jointPairingConfirmed);
    const replacement = await rpc("repair-challenge");
    assert.equal((await rpc("shared-count")).count, 0);
    await pair(replacement);
    await until(async () => await popup.locator("#status").getAttribute("data-state") === "paired");
    assert.equal(await worker.evaluate(() => globalThis.__jointPairingConfirmed), confirmations + 1);
    assert.equal(await worker.evaluate(async tabId => (await chrome.webNavigation.getFrame({ tabId, frameId: 0 }))?.documentId, fixtureId), originalDocument);
    if (scenario === "active-connection-repair") { await noInput(); pass(stage); }

    stage = "old-grant-denied-before-and-after-fresh-share";
    const rejectedOldGrant = async suffix => {
      detail = `old-grant-${suffix}-denied`;
      for (const screenshot of [false, true]) {
        assert.equal((await call("computer_observe", { ...selectedRun, screenshot })).isError, true);
      }
      assert.equal((await call("computer_act", { ...oldClick, actionId: `retired-${suffix}-click` })).isError, true);
      assert.equal((await call("computer_open_target", { runId: selectedRun.runId, targetId: `tab:${memberB.id}` })).isError, true);
      assert.equal(await readCount(), readsBefore, "retired grant must not dispatch page read/input");
      await noInput();
    };
    await rejectedOldGrant("before-share");
    await share(fixture, "CU New TaskGrant Fixture", 1, false);
    await share(memberB.page, "CU New TaskGrant Fixture B", 2, false);
    await share(memberC.page, "CU New TaskGrant Fixture C", 3, false);
    await rejectedOldGrant("after-share");
    const authority = await rpc("lifecycle-authority");
    assert.equal(authority.grants, 1);
    assert.equal(authority.continuations, 1);
    assert.equal(authority.ownedWorkers, 0);
    assert.equal(authority.state.grants.length, 1);
    assert.equal(authority.state.executions.length, 1);
    pass(stage);

    stage = "old-run-stop-retains-new-connection-shares";
    decode(await call("computer_stop", selectedRun));
    await until(async () => (await rpc("stopped", { runId: selectedRun.runId })).ok);
    assert.equal((await rpc("shared-count")).count, 3);
    assert.equal((await popup.locator("#status").getAttribute("data-state")), "paired");
    await noInput(); pass(stage);
    reportPassed(); return;
  }
  const observation = await observe();
  assert(observation.text.includes("Owned visible fixture 你好"));
  const clickedArgs = { ...identity(observation, "new-task-click", "Fixture action"), action: "click", parameters: {} };
  const clicked = decode(await call("computer_act", clickedArgs));
  assert.equal(clicked.kind, "applied");
  assert.equal(await fixture.evaluate(() => globalThis.clicks), 1);
  assert.deepEqual(decode(await call("computer_act", clickedArgs)), clicked);
  assert.equal(await fixture.evaluate(() => globalThis.clicks), 1); pass(stage);
  assert.equal((await call("computer_act", { ...clickedArgs, ...targetC, actionId: "unselected-member-c-click" })).isError, true);
  await noMemberCReadOrInput();

  stage = "new-grant-mcp-cjk-set-and-type";
  const set = decode(await call("computer_act", { ...identity(await observe(), "new-task-set", "Fixture input"), action: "set_value", parameters: { text: "你好" } }));
  assert.equal(set.kind, "verified");
  assert.equal(await fixture.locator("#input").inputValue(), "你好");
  const typed = decode(await call("computer_act", { ...identity(await observe(), "new-task-type", "Fixture input"), action: "type_text", parameters: { text: "，世界🙂" } }));
  assert.equal(typed.kind, "verified");
  assert.equal(await fixture.locator("#input").inputValue(), "你好，世界🙂"); pass(stage);

  stage = "new-grant-mcp-wait-and-scroll";
  detail = "wait-action";
  const waiting = decode(await call("computer_act", { ...identity(await observe(), "new-task-wait", "Fixture action"), action: "wait", parameters: { nameEquals: "Fixture action" } }));
  assert.equal(waiting.kind, "verified");
  detail = "scroll-action";
  const scrolling = decode(await call("computer_act", { ...identity(await observe(), "new-task-scroll", "Fixture scroll"), action: "scroll", parameters: { delta: 60 } }));
  assert.equal(scrolling.kind, "verified");
  assert.equal(await fixture.locator("#scroll").evaluate(element => element.scrollTop), 60); pass(stage);

  stage = "new-grant-mcp-owner-and-stale-rejected";
  const binding = await rpc("mcp-binding");
  other = sessionClient({ ...binding, session: "foreign-app" });
  assert.equal((await other.call("computer_observe", { ...selectedRun, screenshot: false })).isError, true);
  await other.close(); other = null;
  assert.equal((await call("computer_act", { ...clickedArgs, actionId: "new-task-stale", snapshotId: "00000000-0000-4000-8000-000000000099" })).isError, true);
  assert.equal(await fixture.evaluate(() => globalThis.clicks), 1); pass(stage);

  // Each member's five actions use its own fresh observation and independent DOM.
  const memberActions = async (page, prefix, member) => {
    const phase = (action, name) => {
      detail = `member-${member}-${action}-${name}`;
      memberActionPhase = { member, action, phase: name };
    };
    const act = async (action, args, expectedKind) => {
      phase(action, "dispatch");
      const reply = await call("computer_act", args);
      const evidence = { member, action, replyError: reply.isError === true, kind: "unparsed" };
      assert(memberActionEvidence.length < 32, "bounded fixed-action diagnostic");
      memberActionEvidence.push(evidence);
      phase(action, "decode");
      const result = decode(reply);
      evidence.kind = outcomeKind(result.kind);
      phase(action, "outcome");
      assert.equal(result.kind, expectedKind);
      return result;
    };
    const inputPostcondition = async (action, expected) => {
      phase(action, "independent-input");
      const actual = await page.locator("#input").inputValue();
      memberActionEvidence.at(-1).inputMatches = actual === expected;
      assert.equal(actual, expected);
    };
    phase("observe", "marker");
    const fresh = await observe();
    assert(fresh.text.includes(`member ${member}`));
    const click = { ...identity(fresh, `${prefix}-click`, "Fixture action"), action: "click", parameters: {} };
    const clicked = await act("click", click, "applied");
    const duplicate = await act("click_duplicate", click, "applied");
    phase("click_duplicate", "dedupe");
    assert.deepEqual(duplicate, clicked);
    phase("set_value", "observe");
    await act("set_value", { ...identity(await observe(), `${prefix}-set`, "Fixture input"), action: "set_value", parameters: { text: "你好" } }, "verified");
    await inputPostcondition("set_value", "你好");
    phase("type_text", "observe");
    await act("type_text", { ...identity(await observe(), `${prefix}-type`, "Fixture input"), action: "type_text", parameters: { text: "，世界🙂" } }, "verified");
    await inputPostcondition("type_text", "你好，世界🙂");
    phase("wait", "observe");
    await act("wait", { ...identity(await observe(), `${prefix}-wait`, "Fixture action"), action: "wait", parameters: { nameEquals: "Fixture action" } }, "verified");
    phase("scroll", "observe");
    await act("scroll", { ...identity(await observe(), `${prefix}-scroll`, "Fixture scroll"), action: "scroll", parameters: { delta: 60 } }, "verified");
    phase("actions", "returned");
    return click;
  };
  const rejectOldAction = async (args, actionId) => {
    assert.equal((await call("computer_act", { ...args, actionId })).isError, true);
  };
  const numericPostcondition = async (member, name, read, expected) => {
    detail = `member-${member}-independent-${name}`;
    memberActionPhase = { member, action: name, phase: "independent-dom" };
    const actual = await read();
    assert(memberDomEvidence.length < 16, "bounded fixed DOM diagnostic");
    const value = typeof actual === "number" && Number.isFinite(actual) && actual >= 0 && actual <= 10000 ? actual : "other";
    memberDomEvidence.push({ member, property: name, value, expected });
    assert.equal(actual, expected);
  };
  stage = "new-grant-member-b-five-actions";
  await memberB.page.bringToFront();
  const openedB = decode(await call("computer_open_target", { runId: selectedRun.runId, targetId: `tab:${memberB.id}` }));
  assert.equal(openedB.targetId, `tab:${memberB.id}`);
  assert(openedB.targetGeneration > observation.targetGeneration);
  selectedRun.targetId = openedB.targetId;
  await rejectOldAction(clickedArgs, "a-ref-after-switch-b");
  const oldB = await memberActions(memberB.page, "member-b", "B");
  await numericPostcondition("B", "click-count", () => memberB.page.evaluate(() => globalThis.clicks), 1);
  await numericPostcondition("B", "scroll-top", () => memberB.page.locator("#scroll").evaluate(element => element.scrollTop), 60);
  await numericPostcondition("A", "click-unchanged", () => fixture.evaluate(() => globalThis.clicks), 1);
  detail = "member-C-no-read-or-input";
  await noMemberCReadOrInput(); pass(stage);

  stage = "new-grant-switch-back-a-retires-old-refs";
  await fixture.bringToFront();
  const openedA = decode(await call("computer_open_target", { runId: selectedRun.runId, targetId: `tab:${fixtureId}` }));
  assert.equal(openedA.targetId, `tab:${fixtureId}`);
  assert(openedA.targetGeneration > openedB.targetGeneration);
  selectedRun.targetId = openedA.targetId;
  await rejectOldAction(oldB, "b-ref-after-switch-a");
  await rejectOldAction(clickedArgs, "original-a-ref-after-roundtrip");
  await memberActions(fixture, "member-a-again", "A");
  await numericPostcondition("A", "click-count", () => fixture.evaluate(() => globalThis.clicks), 2);
  await numericPostcondition("A", "scroll-top", () => fixture.locator("#scroll").evaluate(element => element.scrollTop), 120);
  await numericPostcondition("B", "click-unchanged", () => memberB.page.evaluate(() => globalThis.clicks), 1);
  detail = "member-C-no-read-or-input";
  await noMemberCReadOrInput(); pass(stage);

  stage = "new-grant-stop-keeps-real-share-and-tab";
  decode(await call("computer_stop", selectedRun));
  await until(async () => (await rpc("stopped", { runId: selectedRun.runId })).ok);
  assert.equal((await call("computer_observe", { ...selectedRun, screenshot: false })).isError, true);
  assert.equal(fixture.isClosed(), false);
  assert.equal((await rpc("preparation-after-stop")).shared, 3);
  assert.equal(await fixture.evaluate(() => globalThis.clicks), 2);
  assert.equal(memberB.page.isClosed(), false);
  assert.equal(memberC.page.isClosed(), false);
  await noMemberCReadOrInput(); pass(stage);
  reportPassed();
} catch (error) {
  // Fixed stage only: assertion payloads and transport binding contain credentials.
  const errorClass = ["AssertionError", "Error", "TypeError"].includes(error?.name) ? error.name : "other";
  const errorCode = ["ENOENT", "EACCES", "EPERM", "ERR_ASSERTION"].includes(error?.code) ? error.code : "other";
  // Classify only fixed known families. Never record error text/stack/URLs.
  const errorFamily = error?.message === "private fixture deadline" ? "private-rpc-timeout"
    : error?.message === "native fixture postcondition deadline" ? "postcondition-timeout"
    : error?.message === "native worker evaluation failed" ? "native-worker-evaluation-failed"
    : error?.message === "native worker evaluation timeout" ? "native-worker-evaluation-timeout"
    : error?.message === "native worker attachment unavailable" ? "native-worker-attachment-unavailable"
    : error?.message === "original module capture deadline" ? "original-module-capture-timeout"
    : error?.constructor?.name === "TimeoutError" ? "timeout"
    : /Execution context was destroyed|Cannot find context with specified id/i.test(error?.message || "") ? "execution-context-destroyed"
    : /Worker.*closed|Worker.*has been closed/i.test(error?.message || "") ? "worker-closed"
    : /Target.*closed|context.*closed|browser.*closed/i.test(error?.message || "") ? "target-closed"
    : /Protocol error \(/.test(error?.message || "") ? "protocol-error"
    : /net::ERR_ABORTED/.test(error?.message || "") ? "navigation-aborted"
    : /net::ERR_FAILED/.test(error?.message || "") ? "navigation-failed"
    : "other";
  if (startupEvidence.phase !== "complete") startupEvidence.elapsedMs = Math.round(performance.now() - startupStarted);
  let primaryPersisted = false;
  try {
    if (phaseEvidence) {
      phaseEvidence.primary({ stage, detail, errorClass, errorCode, errorFamily, checks, startupEvidence,
        nativeEvaluation: nativeEvaluationFailure(error),
        connectionEvidence: worker?.connectionEvidence() });
      primaryPersisted = true;
    }
  } catch { /* Keep the primary error; original Job/profile teardown remains authoritative. */ }
  process.stderr.write(`new TaskGrant joint primary failure: persisted=${primaryPersisted}, checks=${checks.length}, family=${errorFamily}\n`);
  // No browser query, worker resume or other auxiliary await may precede the primary record.
  if (pausedWorker) {
    try { await pausedWorker.resume(); } catch { /* Original context/Job still owns teardown. */ }
    pausedWorker = undefined;
  }
  const status = await popup?.locator("#status").textContent().catch(() => "");
  const statusClass = ["Paired with Grok App.", "This page cannot be shared.", "The operation was cancelled.", "Check the App address and code, then try again."].includes(status) ? status : "other";
  const statusState = await popup?.locator("#status").getAttribute("data-state").catch(() => "");
  const statusStateClass = ["paired", "unpaired", "pairingRejected", "connectionFailed", "invalidCode", "invalidAddress", "busy", "cancelled", "challengeChanged"].includes(statusState) ? statusState : "other";
  const tabState = await popup?.locator("#tab-status").getAttribute("data-state").catch(() => "");
  const tabStateClass = ["shared", "unshared", "shareUnavailable"].includes(tabState) ? tabState : "other";
  const diagnostic = await worker?.evaluate(async () => {
    // Fixture-only storage witness after failure; never read pairing/action records.
    let storageWrite = "ok";
    let accessLevel = "ok";
    try { await chrome.storage.local.setAccessLevel({ accessLevel: "TRUSTED_CONTEXTS" }); }
    catch (failure) { accessLevel = failure.name === "TypeError" ? "type-error" : "failed"; }
    try { await chrome.storage.local.set({ cuJointStorageWitness: true }); await chrome.storage.local.remove("cuJointStorageWitness"); }
    catch (failure) { storageWrite = /IO error|I\/O error/i.test(failure.message) ? "io-error" : /quota/i.test(failure.message) ? "quota-error" : "failed"; }
    return {
    totalScripts: globalThis.__jointCalls?.length,
    inspections: globalThis.__jointCalls?.filter(row => row.function === "inspectDocument").length,
    fences: globalThis.__jointCalls?.filter(row => row.function === "fenceDocument").length,
    captures: globalThis.__jointCaptures,
    capturePermissionDenials: globalThis.__jointCapturePermissionDenials,
    http: globalThis.__jointHttp,
    connectionFailures: (await import(new URL("./connection-diagnostics.mjs", self.location.href).href)).readConnectionDiagnostics(),
    browserMarkerPresent: (await chrome.storage.local.get("cuBrowserSession")).cuBrowserSession !== undefined,
    executionDatabasePresent: (await indexedDB.databases()).some(row => row.name === "grok-cu-execution"),
    storageWrite,
    accessLevel,
    executeApi: typeof chrome.scripting?.executeScript === "function",
    captureApi: typeof chrome.tabs?.captureVisibleTab === "function",
  }; }).catch(() => ({}));
  if (profile) await writeFile(path.join(profile, "joint-failure.json"), JSON.stringify({ stage, detail, errorClass, errorCode, errorFamily, startupEvidence, replacementEvidence, bootstrapEvidence, preapprovalLeaseEvidence, ownerReplacementEvidence, ownerInputEvidence, originalObservationEvidence, statusClass, statusStateClass, tabStateClass, checks, memberActionPhase, memberActionEvidence, memberDomEvidence, diagnostic }), { flag: "wx" }).catch(() => {});
  process.stderr.write(`new TaskGrant joint fixture failed: stage=${stage}, detail=${detail}, class=${errorClass}, code=${errorCode}, family=${errorFamily}, startup=${JSON.stringify(startupEvidence)}, replacement=${JSON.stringify(replacementEvidence)}, originalObservation=${JSON.stringify(originalObservationEvidence)}, ownerInput=${JSON.stringify(ownerInputEvidence)}, status=${statusClass}, statusState=${statusStateClass}, tab=${tabStateClass}, memberActions=${JSON.stringify(memberActionEvidence)}, memberDom=${JSON.stringify(memberDomEvidence)}, diagnostic=${JSON.stringify(diagnostic)}\n`);
  process.stdout.write(JSON.stringify({ event: "failed", stage }) + "\n");
  process.exitCode = 1;
} finally {
  phaseEvidence?.close();
  await pausedWorker?.resume();
  await other?.close(); await client?.close();
  await worker?.dispose?.();
  await context?.close();
  await attachedBrowser?.close(); await nativeBrowser?.close();
  await new Promise(resolve => fixtureServer.close(resolve));
  lines.close(); process.stdin.destroy();
}
}
await runJoint();
