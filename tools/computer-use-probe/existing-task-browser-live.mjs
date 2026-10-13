// Original new TaskGrant/App/MCP survives a full owned browser exit. Host gestures
// and ACP remain synthetic; no daily profiles, clock edits, or forced disconnect.
import assert from "node:assert/strict";
import { lstat, mkdir, realpath, writeFile } from "node:fs/promises";
import path from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { EXTENSION_ID } from "../computer-use-extension/pairing-client.mjs";
import { pinnedChromeLaunchPath } from "../computer-use-browser/immutable-chrome.mjs";
import { instrumentJointWorker } from "./existing-task-worker-diagnostics.mjs";
import { OwnedCdpBrowser } from "./owned-cdp-browser.mjs";
import { openSourceJointWorker } from "./existing-task-bootstrap-live.mjs";

// Reuse the owned native browser fixture's install-once contract. Repeating
// --load-extension at each launch reloads the extension and is not onStartup.
export async function launchOwnedJointBrowser({ browser, profile, chromium, extension,
  install, publishBrowser, phase }) {
  const nativeBrowser = new OwnedCdpBrowser(
    pinnedChromeLaunchPath(browser, path.join(profile, ".chrome-exec")), profile);
  publishBrowser({ nativeBrowser, attachedBrowser: undefined, context: undefined,
    worker: undefined, popup: undefined });
  phase("native-version-ready");
  await nativeBrowser.send("Browser.getVersion");
  if (install) {
    phase("native-install-once");
    assert.equal((await nativeBrowser.send("Extensions.loadUnpacked", { path: extension })).id, EXTENSION_ID);
  }
  phase("native-owned-context-attach");
  const attachedBrowser = await chromium.connectOverCDP(await nativeBrowser.playwrightEndpoint(), { timeout: 10000 });
  publishBrowser({ attachedBrowser });
  const [context] = attachedBrowser.contexts();
  assert(context, "owned native browser context missing");
  publishBrowser({ context });
  return context;
}

export async function verifyBrowserReplacement({ scenario, rpc, call, decode, until,
  observe, identity, context, popup, worker, nativeBrowser, attachedBrowser,
  fixture, fixtureId, memberB, memberC,
  selectedRun, initial, profile, browser, chromium, extension, fixtureUrl, mcpPid,
  publishBrowser, evidence, pass, phase }) {
  const freshProfile = scenario !== "active-browser-reopen";
  const collisionProfile = scenario === "active-profile-tab-id-collision";
  const setPhase = (stage, detail) => { evidence.phase = detail; phase(stage, detail); };
  let stage = "original-grant-fresh-observation-no-input";
  setPhase(stage, "original-observation-and-browser-witness");
  const original = await observe();
  assert(original.text.includes("Owned visible fixture 你好"));
  const oldClick = { ...identity(original, "original-browser-never-clicked", "Fixture action"),
    action: "click", parameters: {} };
  const originalBrowser = await worker.evaluate(async () =>
    (await chrome.storage.local.get("cuBrowserSession")).cuBrowserSession);
  assert.equal(typeof originalBrowser?.id, "string");
  const originalDocument = await worker.evaluate(async tabId =>
    (await chrome.webNavigation.getFrame({ tabId, frameId: 0 }))?.documentId, fixtureId);
  assert.equal(typeof originalDocument, "string");
  for (const page of [fixture, memberB.page, memberC.page]) {
    assert.equal(await page.evaluate(() => globalThis.clicks), 0);
    assert.equal(await page.locator("#input").inputValue(), "");
    assert.equal(await page.locator("#scroll").evaluate(element => element.scrollTop), 0);
  }
  const originalDispatches = await worker.evaluate(tabId => ({
    reads: globalThis.__jointCalls.filter(row => row.function === "observeDocument").length,
    inputs: globalThis.__jointCalls.filter(row => row.function === "actDocument").length,
    unselected: globalThis.__jointCalls.filter(row => row.tab === tabId
      && ["observeDocument", "actDocument"].includes(row.function)).length,
    captures: globalThis.__jointCaptures, disconnects: globalThis.__jointDisconnectRequests,
  }), memberC.id);
  assert.equal(originalDispatches.reads, 1);
  assert.equal(originalDispatches.inputs, 0);
  assert.equal(originalDispatches.unselected, 0);
  assert.equal(originalDispatches.captures, 0);
  assert.equal(originalDispatches.disconnects, 0);
  pass(stage);

  // Keep exactly this peer and its original MCP child, plus only their original
  // console hosts. Windows creates conhost even for these hidden Node children.
  // A total count alone cannot distinguish a console host from a live browser.
  stage = "entire-original-browser-exited-with-original-mcp-alive";
  setPhase(stage, "original-job-before-entire-context-close");
  const lease = await rpc("lease-witness");
  assert.equal(lease.stored, true);
  assert.equal(lease.leaseMs, 30000);
  const originalOwner = await rpc("browser-owner-state");
  evidence.originalJobProcesses = originalOwner.activeProcesses;
  evidence.originalJobMembers = originalOwner.members;
  assert(evidence.originalJobProcesses > 2);
  const nodeIds = [process.pid, mcpPid].sort((a, b) => a - b);
  const memberNodeIds = state => state.members.filter(row => row.family === "node")
    .map(row => row.pid).sort((a, b) => a - b);
  assert.deepEqual(memberNodeIds(originalOwner), nodeIds);
  assert(originalOwner.members.some(row => row.family === "chrome" && row.active));
  const consoleIds = new Set(originalOwner.members.filter(row => row.family === "console-host").map(row => row.pid));
  const originalBrowserGone = async () => {
    const state = await rpc("browser-owner-state");
    evidence.lastJobProcesses = state.activeProcesses;
    evidence.lastJobMembers = state.members;
    return state.activeProcesses === state.members.length
      && JSON.stringify(memberNodeIds(state)) === JSON.stringify(nodeIds)
      && state.members.every(row => row.active === true
        && (row.family === "node" || row.family === "console-host" && consoleIds.has(row.pid)));
  };
  const exitStarted = performance.now();
  setPhase(stage, "entire-original-context-close");
  try {
    await worker.dispose?.();
    evidence.originalWorkerDisposed = true;
    setPhase(stage, "original-native-attachment-disposed");
  } finally {
    // This exact attachment has been consumed; a later failure must not detach it twice.
    // Leave disposal errors visible while the original browser/Job still owns teardown.
    publishBrowser({ worker: undefined });
  }
  await attachedBrowser.close();
  await nativeBrowser.close();
  for (const page of [popup, fixture, memberB.page, memberC.page]) assert.equal(page.isClosed(), true);
  evidence.originalPagesClosed = true;
  setPhase(stage, "original-job-browser-members-physically-exited");
  await until(originalBrowserGone);
  evidence.afterExitJobProcesses = (await rpc("browser-owner-state")).activeProcesses;
  evidence.originalBrowserProcessesGone = true;
  const rejectedOldGrant = async suffix => {
    for (const screenshot of [false, true]) {
      assert.equal((await call("computer_observe", { ...selectedRun, screenshot })).isError, true);
    }
    assert.equal((await call("computer_act", { ...oldClick, actionId: `exit-${suffix}-click` })).isError, true);
    assert.equal((await call("computer_open_target", {
      runId: selectedRun.runId, targetId: `tab:${memberB.id}`,
    })).isError, true);
  };
  pass(stage);

  stage = "original-browser-exit-natural-lease-retires-shares";
  setPhase(stage, "production-http-maintenance-original-lease");
  const leaseDeadline = exitStarted + lease.leaseMs + 2000;
  for (;;) {
    const current = await rpc("lease-witness");
    assert.equal(current.leaseMs, lease.leaseMs);
    if (!current.stored) break;
    assert(performance.now() < leaseDeadline, "original browser exit lease did not naturally retire");
    await delay(100);
  }
  evidence.leaseElapsedMs = Math.round(performance.now() - exitStarted);
  assert(evidence.leaseElapsedMs >= lease.leaseMs - 11000);
  assert.equal((await rpc("shared-count")).count, 0);
  assert.equal(await originalBrowserGone(), true);
  await rejectedOldGrant("before-relaunch");
  pass(stage);

  stage = "replacement-browser-new-session-and-document";
  setPhase(stage, "replacement-owned-profile-validation");
  let replacementProfile = profile;
  if (freshProfile) {
    // A child of the original registered profile inherits the same guard's
    // teardown. Never create an unregistered sibling or delete a failed root.
    assert.equal((await lstat(profile)).isSymbolicLink(), false);
    replacementProfile = path.join(profile, "replacement-profile");
    await mkdir(replacementProfile);
    assert.equal(await realpath(replacementProfile), replacementProfile);
    assert.equal((await lstat(replacementProfile)).isSymbolicLink(), false);
  }
  if (collisionProfile) {
    // Chromium 140 SessionIdGenerator initializes from this non-secret Int64
    // pref, then adds 50. Fresh profiles otherwise choose a random counter.
    // Seed only a NEW owned profile to model restored/colliding tab IDs; never
    // copy another profile or modify an extension/Host identity or lease clock.
    assert(Number.isSafeInteger(fixtureId) && fixtureId > 66);
    await writeFile(path.join(replacementProfile, "Local State"), JSON.stringify({
      session_id_generator_last_value: String(fixtureId - 66),
    }), { flag: "wx" });
    evidence.tabIdCollisionSeeded = true;
  }
  evidence.freshProfile = freshProfile;
  setPhase(stage, "replacement-context-launch");
  const replacementContext = await launchOwnedJointBrowser({
    browser, profile: replacementProfile, chromium, extension, install: freshProfile,
    publishBrowser, phase: detail => setPhase(stage, detail),
  });
  // Transfer only fixture diagnostics/teardown handles, never Host authority.
  publishBrowser({ context: replacementContext });
  const { worker: replacementWorker, popup: replacementPopup } = await openSourceJointWorker({
    context: replacementContext, until, publishBrowser, evidence,
    phase: detail => setPhase(stage, `replacement-${detail}`),
  });
  setPhase(stage, "replacement-source-api-instrument");
  await instrumentJointWorker(replacementWorker);
  setPhase(stage, "replacement-browser-session-ready");
  const browserSession = () => replacementWorker.evaluate(async () =>
    (await chrome.storage.local.get("cuBrowserSession")).cuBrowserSession);
  await until(async () => {
    const session = await browserSession();
    evidence.sessionMarkerPresent = typeof session?.id === "string";
    evidence.sessionStarted = session?.started === true;
    evidence.browserSessionChanged = session?.id !== originalBrowser.id;
    evidence.previousSessionMatches = session?.previousId === originalBrowser.id;
    return typeof session?.id === "string" && session.id !== originalBrowser.id
      && (freshProfile || session.started && session.previousId === originalBrowser.id);
  });
  const currentBrowser = await browserSession();
  evidence.browserSessionChanged = currentBrowser.id !== originalBrowser.id;
  evidence.previousSessionMatches = currentBrowser.previousId === originalBrowser.id;
  assert.equal(currentBrowser.previousId, freshProfile ? null : originalBrowser.id);
  await until(async () => await replacementPopup.locator("#status").getAttribute("data-state") === "unpaired");
  evidence.replacementUnpaired = true;

  const replacements = [];
  const paddingPages = [];
  setPhase(stage, "replacement-owned-pages-open");
  for (const [member, route] of [["A", "/fixture"], ["B", "/member-b"], ["C", "/member-c"]]) {
    // This bounded loop constructs owned fixture pages, never retries a model
    // action. The actual native tab counter, not a fabricated API reply, must
    // reach the original active tab ID. All padding pages stay unshared.
    for (let allocation = 0; allocation < 24; allocation++) {
      const page = await replacementContext.newPage();
      await page.goto(fixtureUrl + route);
      await page.bringToFront();
      const id = await replacementWorker.evaluate(async () =>
        (await chrome.tabs.query({ active: true, lastFocusedWindow: true }))[0].id);
      if (!collisionProfile || member !== "A" || id === fixtureId) {
        replacements.push({ member, page, id }); break;
      }
      paddingPages.push(page);
      assert(id < fixtureId, "native fixture counter passed the required collision");
    }
    assert.equal(replacements.length, ["A", "B", "C"].indexOf(member) + 1,
      "bounded native fixture construction did not produce the required page");
  }
  evidence.unsharedPaddingPages = paddingPages.length;
  const newA = replacements[0];
  const replacementDocument = await replacementWorker.evaluate(async tabId =>
    (await chrome.webNavigation.getFrame({ tabId, frameId: 0 }))?.documentId, newA.id);
  assert.equal(typeof replacementDocument, "string");
  assert.notEqual(replacementDocument, originalDocument);
  evidence.documentChanged = true;
  evidence.activeTabIdReused = newA.id === fixtureId;
  evidence.originalFixtureTabs = [fixtureId, memberB.id, memberC.id];
  evidence.replacementFixtureTabs = replacements.map(row => row.id);
  evidence.reusedSelectedTabIds = replacements.map(row => row.id)
    .filter(id => [fixtureId, memberB.id].includes(id));
  // The controlled new profile must produce an actual reused active tab ID.
  setPhase(stage, "replacement-reused-active-tab-id-witness");
  if (collisionProfile) assert.equal(evidence.activeTabIdReused, true);
  assert((await rpc("browser-owner-state")).activeProcesses > 2);
  pass(stage);

  const replacementUnchanged = async () => {
    for (const page of [...replacements.map(row => row.page), ...paddingPages]) {
      assert.equal(page.isClosed(), false);
      assert.equal(await page.evaluate(() => globalThis.clicks), 0);
      assert.equal(await page.locator("#input").inputValue(), "");
      assert.equal(await page.locator("#scroll").evaluate(element => element.scrollTop), 0);
    }
    const dispatches = await replacementWorker.evaluate(() => ({
      contentAndInput: globalThis.__jointCalls.filter(row =>
        ["observeDocument", "actDocument"].includes(row.function)).length,
      captures: globalThis.__jointCaptures,
    }));
    assert.deepEqual(dispatches, { contentAndInput: 0, captures: 0 });
  };
  stage = "old-grant-denied-before-and-after-replacement-share";
  setPhase(stage, "old-grant-before-replacement-pair");
  await rejectedOldGrant("before-pair");
  await replacementUnchanged();
  setPhase(stage, "original-host-new-pairing-challenge");
  const challenge = await rpc("repair-challenge");
  await replacementPopup.locator("#endpoint").fill(challenge.endpoint);
  await replacementPopup.locator("#code").fill(challenge.verificationCode);
  await replacementPopup.locator("#pair").click();
  await until(async () => await replacementPopup.locator("#status").getAttribute("data-state") === "paired");
  assert.equal(await replacementWorker.evaluate(() => globalThis.__jointPairingConfirmed), 1);
  await rejectedOldGrant("after-pair");
  await replacementUnchanged();
  setPhase(stage, "source-popup-share-new-documents");
  for (const [index, { member, page }] of replacements.entries()) {
    await page.bringToFront();
    const title = "CU New TaskGrant Fixture" + (member === "A" ? "" : ` ${member}`);
    await until(async () => await replacementPopup.locator("#tab-title").textContent() === title);
    await until(async () => await replacementPopup.locator("#share-current").isEnabled());
    await replacementPopup.locator("#share-current").click();
    await until(async () => (await rpc("shared-count")).count === index + 1);
  }
  await rejectedOldGrant("after-share");
  await replacementUnchanged();
  const authority = await rpc("lifecycle-authority");
  assert.equal(authority.grants, 1);
  assert.equal(authority.continuations, 1);
  assert.equal(authority.ownedWorkers, 0);
  assert.equal(authority.state.grants.length, 1);
  assert.equal(authority.state.executions.length, 1);
  assert.deepEqual(authority.state.executions[0].key, initial.state.request.key);
  assert.equal(authority.state.executions[0].runId, selectedRun.runId);
  pass(stage);

  stage = "old-run-stop-retains-replacement-browser-shares-and-tabs";
  setPhase(stage, "stop-original-run-through-original-mcp");
  decode(await call("computer_stop", selectedRun));
  setPhase(stage, "stop-response-decoded");
  await until(async () => (await rpc("stopped", { runId: selectedRun.runId })).ok);
  setPhase(stage, "original-run-stopped");
  assert.equal((await rpc("shared-count")).count, 3);
  setPhase(stage, "replacement-shares-retained");
  assert.equal(await replacementPopup.locator("#status").getAttribute("data-state"), "paired");
  setPhase(stage, "replacement-pair-retained");
  await rejectedOldGrant("after-stop");
  await replacementUnchanged();
  assert((await rpc("browser-owner-state")).activeProcesses > 2);
  evidence.phase = "complete";
  process.stderr.write(`new TaskGrant browser replacement witness: ${JSON.stringify(evidence)}; original MCP/App retained; production lease; no clock edit, forced disconnect or legacy grant\n`);
  pass(stage);
}
