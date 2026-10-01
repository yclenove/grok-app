// Isolated browser integration checks; not evidence of a real toolbar activeTab gesture.
import assert from "node:assert/strict";
import { verifyObservationRaces } from "./observation-races-live.mjs";
import { verifyCaptureNormalization } from "./capture-normalization-live.mjs";
import { verifyExistingMcp } from "./existing-mcp-live.mjs";

export async function verifySharing({ context, popup, fixtureUrl, rpc, until, stage, passed, appHost }) {
  const worker = context.serviceWorkers()[0];
  assert(worker, "the paired service worker must be alive");
  // Instrument calls, not permission checks or results. The original Chrome API still executes.
  await worker.evaluate(() => {
    const execute = chrome.scripting.executeScript.bind(chrome.scripting);
    const capture = chrome.tabs.captureVisibleTab.bind(chrome.tabs);
    const fetch = globalThis.fetch.bind(globalThis);
    globalThis.__cuProbeHttp = [];
    globalThis.fetch = async (input, init) => {
      const route = new URL(typeof input === "string" ? input : input.url).pathname;
      if (globalThis.__cuPauseResult && route === "/cu/extension-result") {
        globalThis.__cuPauseResult = false;
        await new Promise(resolve => { globalThis.__cuReleaseResult = resolve; });
      }
      const record = { route, status: null, failed: false };
      if (route.startsWith("/cu/")) {
        globalThis.__cuProbeHttp.push(record);
        if (globalThis.__cuProbeHttp.length > 128) globalThis.__cuProbeHttp.shift();
      }
      try {
        const response = await fetch(input, init); record.status = response.status; return response;
      } catch (error) { record.failed = true; throw error; }
    };
    globalThis.__cuProbeCaptures = [];
    chrome.tabs.captureVisibleTab = async (...args) => {
      const record = { windowId: args[0], permissionDenied: false, completed: false };
      globalThis.__cuProbeCaptures.push(record);
      try { const result = await capture(...args); record.completed = true; return result; }
      catch (error) { record.permissionDenied = /activeTab|permission/i.test(error.message); throw error; }
    };
    globalThis.__cuProbeInspections = [];
    chrome.scripting.executeScript = async request => {
      globalThis.__cuProbeInspections.push(request.target.tabId);
      const result = await execute(request);
      if (globalThis.__cuPauseObservation && request.func?.name === "observeDocument") {
        globalThis.__cuPauseObservation = false;
        await new Promise(resolve => { globalThis.__cuReleaseObservation = resolve; });
      }
      if (globalThis.__cuPauseInspection) {
        globalThis.__cuPauseInspection = false;
        await new Promise(resolve => { globalThis.__cuReleaseInspection = resolve; });
      }
      return result;
    };
  });
  const inspections = () => worker.evaluate(() => globalThis.__cuProbeInspections);
  const activeId = () => worker.evaluate(async () => {
    const [tab] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
    return tab.id;
  });
  const share = async () => {
    await until(async () => await popup.locator("#tab-title").textContent() === "Computer Use Fixture");
    await until(async () => await popup.locator("#share-current").isEnabled());
    await popup.locator("#share-current").click();
    await until(async () => await popup.locator("#unshare-current").isEnabled());
    await until(async () => (await rpc("shared")).length === 1);
  };
  // An unrelated HTTP origin has no host permission. Fulfillment is strictly fixture-local.
  const unrelatedUrl = "http://unshared.cu.test/fixture";
  await context.route(unrelatedUrl, route => route.fulfill({ contentType: "text/html",
    body: "<title>Unshared fixture</title><main>Private fixture content</main>" }));
  const unrelated = await context.newPage();
  await unrelated.goto(unrelatedUrl);
  await unrelated.bringToFront();
  const unrelatedId = await activeId();
  const fixture = await context.newPage();
  await fixture.goto(fixtureUrl);
  await fixture.bringToFront();
  const fixtureId = await activeId();
  assert.notEqual(fixtureId, unrelatedId);

  stage("unshared-tab-is-not-inspected");
  assert.equal((await rpc("shared")).length, 0, "pairing/navigation never shares a tab");
  assert.deepEqual(await inspections(), [], "opening tabs/popups never inspects documents");
  passed("unshared-tab-is-not-inspected");

  stage("explicit-share-current-tab");
  await share();
  const [offered] = await rpc("shared");
  assert.equal(offered.title, "Computer Use Fixture");
  assert.equal(offered.url, fixtureUrl);
  assert((await inspections()).length >= 3);
  assert((await inspections()).every(id => id === fixtureId), "unshared origin was never inspected");
  passed("explicit-share-current-tab");

  stage("typed-observation-through-extension");
  const reply = await rpc("observe-shared", { selector: offered.id });
  assert.equal(reply.ok, true, "real extension observation must arrive through authenticated product HTTP");
  const observation = reply.observation;
  assert.equal(observation.title, "Computer Use Fixture"); assert.equal(observation.url, fixtureUrl);
  assert(observation.text.includes("Visible fixture 你好"));
  assert(observation.nodes.some(node => node.name === "Fixture action" && node.elementRef.startsWith(observation.snapshotId)));
  assert(observation.viewportWidth > 0 && observation.viewportHeight > 0 && observation.documentId);
  for (const secret of ["VALUE_SECRET", "PASSWORD_SECRET", "PASSWORD_VALUE", "HIDDEN_SECRET", "TEXTAREA_SECRET"]) {
    assert(!JSON.stringify(observation).includes(secret), "sensitive fixture content must be excluded");
  }
  assert((await inspections()).every(id => id === fixtureId));
  passed("typed-observation-through-extension");

  stage("screenshot-requires-active-tab");
  const denied = await rpc("observe-shared", { selector: offered.id, screenshot: true });
  assert.equal(denied.ok, false, "loopback host permission alone cannot capture a tab");
  // Diagnostic uses Chrome's public action API; it is not a native toolbar gesture.
  const action = await worker.evaluate(async () => {
    try { await chrome.action.openPopup(); return "opened"; } catch { return "unavailable"; }
  });
  const capture = await rpc("observe-shared", { selector: offered.id, screenshot: true });
  assert.equal(capture.ok, false, "action.openPopup does not stand in for a toolbar activeTab grant");
  const captures = await worker.evaluate(() => globalThis.__cuProbeCaptures);
  assert.equal(captures.length, 2, "both requests must reach the real Chrome capture API");
  assert(captures.every(row => row.permissionDenied && !row.completed));
  process.stderr.write(`capture permission: popup=${action}, requests=2, Chrome-denied=2\n`);
  passed("screenshot-requires-active-tab");

  stage("capture-normalization");
  assert(await worker.evaluate(() => typeof createImageBitmap === "function" && typeof OffscreenCanvas === "function"));
  const normalized = await verifyCaptureNormalization(popup);
  process.stderr.write(`capture normalization: solid=${normalized.solid.width}x${normalized.solid.height}, noise=${normalized.noise.width}x${normalized.noise.height}, noiseBase64=${normalized.noise.bytes}\n`);
  passed("capture-normalization");

  if (appHost) {
    await verifyExistingMcp({ worker, fixture, fixtureId, selector: offered.id, rpc, until, stage, passed });
  }

  await verifyObservationRaces({ worker, fixture, unrelated, popup, rpc, until, share, stage, passed });

  stage("observation-large-document");
  await fixture.evaluate(() => {
    const stress = document.createElement("div"); stress.id = "cu-stress";
    stress.innerHTML = "<button>Budget fixture</button>".repeat(10000);
    document.body.append(stress);
  });
  const [stressCandidate] = await rpc("shared");
  const started = Date.now();
  const large = await rpc("observe-shared", { selector: stressCandidate.id });
  assert.equal(large.ok, true); assert.equal(large.observation.truncated, true);
  assert(large.observation.nodes.length <= 64 && large.observation.text.length <= 32000);
  assert(Date.now() - started < 5000, "large-page observation must not exhaust the 10-second request deadline");
  process.stderr.write(`large DOM: elements=10000, truncated=true, hostRoundTripMs=${Date.now() - started}\n`);
  await fixture.evaluate(() => document.getElementById("cu-stress").remove());
  passed("observation-large-document");

  stage("explicit-unshare-keeps-tab-open");
  await popup.locator("#unshare-current").click();
  await until(async () => (await rpc("shared")).length === 0);
  await until(async () => await popup.locator("#share-current").isEnabled());
  assert(!fixture.isClosed()); assert.equal(fixture.url(), fixtureUrl);
  passed("explicit-unshare-keeps-tab-open");

  stage("pending-share-navigation-does-not-offer");
  await worker.evaluate(() => { globalThis.__cuPauseInspection = true; });
  await popup.locator("#share-current").click();
  await until(() => worker.evaluate(() => typeof globalThis.__cuReleaseInspection === "function"));
  await fixture.goto(fixtureUrl + "?pending=1");
  await worker.evaluate(() => { globalThis.__cuReleaseInspection(); delete globalThis.__cuReleaseInspection; });
  await until(async () => await popup.locator("#tab-status").getAttribute("data-state") === "shareUnavailable");
  assert.equal((await rpc("shared")).length, 0, "old metadata cannot offer the new document");
  assert((await rpc("status")).paired === true);
  await fixture.goto(fixtureUrl);
  passed("pending-share-navigation-does-not-offer");

  stage("same-url-reload-revokes-candidate");
  await share();
  const [beforeReload] = await rpc("shared");
  await fixture.reload();
  await until(async () => (await rpc("shared")).length === 0);
  await until(async () => await popup.locator("#share-current").isEnabled());
  assert.equal(fixture.url(), fixtureUrl);
  await share();
  const [afterReload] = await rpc("shared");
  assert.notEqual(beforeReload.id, afterReload.id, "same-URL reload needs fresh picker consent");
  passed("same-url-reload-revokes-candidate");

  stage("navigation-revokes-old-document");
  await fixture.goto(fixtureUrl + "?next=1");
  await until(async () => (await rpc("shared")).length === 0);
  assert((await rpc("status")).paired === true);
  passed("navigation-revokes-old-document");

  stage("tab-close-revokes-candidate");
  await fixture.goto(fixtureUrl); await fixture.bringToFront();
  await share(); await fixture.close();
  await until(async () => (await rpc("shared")).length === 0);
  passed("tab-close-revokes-candidate");

  stage("non-loopback-needs-action-grant");
  await unrelated.bringToFront();
  await until(async () => (await popup.locator("#tab-title").textContent()) !== "Computer Use Fixture");
  await until(async () => await popup.locator("#share-current").isEnabled());
  assert(!(await inspections()).includes(unrelatedId));
  await popup.locator("#share-current").click();
  await until(async () => await popup.locator("#tab-status").getAttribute("data-state") === "shareUnavailable");
  assert.equal((await rpc("shared")).length, 0, "ordinary extension tab does not grant activeTab");
  assert((await rpc("status")).paired === true, "permission denial must not drop a healthy connection");
  passed("non-loopback-needs-action-grant");

  stage("restricted-page-cannot-share");
  await unrelated.goto("chrome://version/"); await unrelated.bringToFront();
  // Clear the previous UI error before the next independent check.
  await popup.reload();
  await unrelated.bringToFront();
  await until(async () => await popup.locator("#share-current").isEnabled());
  await popup.locator("#share-current").click();
  await until(async () => await popup.locator("#tab-status").getAttribute("data-state") === "shareUnavailable");
  assert.equal((await rpc("shared")).length, 0);
  assert((await rpc("status")).paired === true);
  passed("restricted-page-cannot-share");
  await unrelated.close();
}
