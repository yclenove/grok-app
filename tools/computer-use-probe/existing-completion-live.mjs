// App Host receipts + real Chromium fixed scripts. Admission uses a private fixture pipe,
// not production model action polling. Classes run in a trusted extension page, not SW restart acceptance.
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { createInterface } from "node:readline";
import { readFile, realpath } from "node:fs/promises";
import { createServer } from "node:http";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { EXTENSION_ID } from "../computer-use-extension/pairing-client.mjs";

const require = createRequire(new URL("../computer-use-browser/package.json", import.meta.url));
const { chromium } = require("playwright-core");
const args = process.argv.slice(2);
const option = key => args[args.indexOf(key) + 1];
assert(args.includes("--app-host") && args.includes("--owned-profile"), "private App parent and owned profile required");
const lines = createInterface({ input: process.stdin }); let pending;
lines.on("line", line => { const resolve = pending; pending = null; resolve?.(JSON.parse(line)); });
async function rpc(command, extra = {}) {
  assert(!pending, "private requests must be serial"); let timer;
  const reply = new Promise((resolve, reject) => {
    pending = resolve; timer = setTimeout(() => { pending = null; reject(new Error("private Host timeout")); }, 20000);
  });
  process.stdout.write(JSON.stringify({ command, ...extra }) + "\n");
  try { return await reply; } finally { clearTimeout(timer); }
}
async function until(predicate) {
  const deadline = Date.now() + 10000;
  while (Date.now() < deadline) { if (await predicate()) return; await delay(25); }
  throw new Error("fixture postcondition timeout");
}
assert((await rpc("ready")).ok);
const profile = await realpath(option("--owned-profile"));
const owner = JSON.parse(await readFile(path.join(path.dirname(profile), "owner.json"), "utf8"));
assert.equal(owner.owner, option("--profile-owner")); assert.equal(owner.purpose, "cu-pairing-probe");
assert.equal(await realpath(owner.profile), profile);
const server = createServer((_request, response) => {
  response.writeHead(200, { "content-type": "text/html; charset=utf-8" });
  response.end(`<!doctype html><title>Completion owned fixture</title><style>button,input{display:block;margin:20px}</style>
<button id="clicker">Count</button><input id="field" aria-label="Field"><button id="waiting">Ready</button>
<script>window.clicks=0;document.getElementById('clicker').onclick=()=>window.clicks++;</script>`);
});
let context; let extensionPage; let fixture; let stage = "launch"; let checkpoint = "setup"; const passed = [];
const pass = name => { passed.push(name); process.stderr.write(`completion live: ${name} PASS\n`); };
try {
  await new Promise((resolve, reject) => { server.once("error", reject); server.listen(0, "127.0.0.1", resolve); });
  const url = `http://127.0.0.1:${server.address().port}/fixture`;
  const extension = fileURLToPath(new URL("../computer-use-extension", import.meta.url));
  context = await chromium.launchPersistentContext(profile, {
    executablePath: option("--browser"), headless: true, viewport: { width: 1000, height: 700 },
    args: ["--headless=new", "--disable-background-networking", "--disable-features=DisableLoadExtensionCommandLineSwitch",
      "--disable-extensions-except=" + extension, "--load-extension=" + extension],
  });
  context.setDefaultTimeout(10000);
  const worker = context.serviceWorkers()[0] || await context.waitForEvent("serviceworker", { timeout: 10000 });
  assert(worker.url().startsWith(`chrome-extension://${EXTENSION_ID}/`));
  extensionPage = await context.newPage(); await extensionPage.goto(`chrome-extension://${EXTENSION_ID}/popup.html`);
  await extensionPage.evaluate(async () => {
    const [{ PairingClient }, { SharedTabs }, { ExtensionTransport }, { ActionCompletions }] = await Promise.all([
      import(chrome.runtime.getURL("pairing-client.mjs")), import(chrome.runtime.getURL("shared-tabs.mjs")),
      import(chrome.runtime.getURL("extension-transport.mjs")), import(chrome.runtime.getURL("action-completions.mjs")),
    ]);
    const faults = { loseClaim: false, loseSettle: false, delayScripts: false };
    let sharing; let transport; let actions;
    const client = new PairingClient({ storage: chrome.storage.session,
      fetch: async (url, init) => {
        const response = await fetch(url, init);
        if (url.endsWith("/claim") && faults.loseClaim) { faults.loseClaim = false; await response.arrayBuffer(); throw new Error("fixture reply loss"); }
        if (url.endsWith("/settle") && faults.loseSettle) { faults.loseSettle = false; await response.arrayBuffer(); throw new Error("fixture reply loss"); }
        return response;
      }, onSessionChanged: () => { sharing?.reset(); actions?.reset(); transport?.start(); } });
    await client.ready;
    const { allocateWorkerEpoch, ExecutionClock } = await import(chrome.runtime.getURL("execution-clock.mjs"));
    const clock = new ExecutionClock(allocateWorkerEpoch());
    sharing = new SharedTabs({ client, clock, tabs: chrome.tabs, scripting: { executeScript: async request => {
      const rows = await chrome.scripting.executeScript(request);
      if (faults.delayScripts && request.func.name === "actDocument") await new Promise(resolve => { faults.releaseAction = resolve; });
      if (faults.delayScripts && request.func.name === "cancelDocumentOperation") await new Promise(resolve => { faults.releaseCancel = resolve; });
      return rows;
    } } });
    // This historical receipt-only fixture admits unbound offers, not v2 queued actions.
    actions = new ActionCompletions({ client, act: (request, signal) => sharing.act(request, signal),
      claim: (epoch, _request, proof, signal) => client.claimCompletion(epoch, proof, signal) });
    transport = new ExtensionTransport({ client, observe: request => sharing.observe(request) });
    chrome.tabs.onActivated.addListener(() => sharing.activityChanged());
    chrome.windows.onFocusChanged.addListener(() => sharing.activityChanged());
    globalThis.__completionFixture = { client, sharing, actions, transport, faults };
  });
  const challenge = await rpc("begin"); assert((await rpc("confirm", { nonce: challenge.nonce })).ok);
  await extensionPage.evaluate(({ endpoint, code }) => globalThis.__completionFixture.client.pair(endpoint, code), challenge);
  fixture = await context.newPage(); await fixture.goto(url); await fixture.bringToFront();
  const tabId = await extensionPage.evaluate(async expectedUrl => {
    const [tab] = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
    if (tab?.url !== expectedUrl) throw new Error("wrong fixture tab");
    await globalThis.__completionFixture.sharing.share(tab.id); return String(tab.id);
  }, url);
  const candidates = await rpc("shared"); assert.equal(candidates.length, 1); const selector = candidates[0].id;
  let sequence = 0;
  async function packet(name, action, parameters) {
    const { observation } = await rpc("completion-observe", { selector });
    const elementRef = observation.nodes.find(node => node.name === name)?.elementRef;
    assert(elementRef, "fixture reference missing");
    const { proof } = await rpc("completion-offer", { tabId, snapshotId: observation.snapshotId });
    const { snapshotId, ...binding } = proof.binding;
    return { proof, request: { ...binding, sequence: ++sequence, deadlineMs: Date.now() + 10000,
      command: { kind: "act", snapshotId, elementRef, action, parameters } } };
  }
  const run = packet => extensionPage.evaluate(({ request, proof }) => {
    const { client, actions } = globalThis.__completionFixture;
    return actions.run(client.connectionEpoch, request, proof);
  }, packet);
  stage = "real-pairing-and-observation";
  let current = await packet("Count", "click", {}); pass(stage);
  stage = "click-once";
  assert.equal((await run(current)).status, "applied"); assert.equal(await fixture.evaluate(() => clicks), 1);
  assert.equal((await rpc("completion-state")).idle, true); pass(stage);
  stage = "duplicate-is-zero-dispatch";
  assert.equal((await run(current)).status, "rejected"); assert.equal(await fixture.evaluate(() => clicks), 1); pass(stage);
  stage = "cjk-input";
  current = await packet("Field", "set_value", { text: "回执完成🚀" });
  assert.equal((await run(current)).status, "verified"); assert.equal(await fixture.locator("#field").inputValue(), "回执完成🚀"); pass(stage);
  stage = "lost-claim-reply-is-zero-dispatch";
  current = await packet("Count", "click", {});
  await extensionPage.evaluate(() => { globalThis.__completionFixture.faults.loseClaim = true; });
  assert.equal((await run(current)).status, "rejected"); assert.equal(await fixture.evaluate(() => clicks), 1);
  assert.equal((await rpc("completion-state")).idle, true); pass(stage);
  stage = "lost-settle-reply-retries-only-cleanup";
  checkpoint = "prepare";
  current = await packet("Count", "click", {});
  await extensionPage.evaluate(() => { globalThis.__completionFixture.faults.loseSettle = true; });
  checkpoint = "action-result";
  const settlementResult = await run(current);
  if (settlementResult.status !== "applied") process.stderr.write(`completion result: status=${settlementResult.status}, detail=${settlementResult.detail}\n`);
  assert.equal(settlementResult.status, "applied");
  checkpoint = "counter";
  assert.equal(await fixture.evaluate(() => clicks), 2);
  checkpoint = "host-idle";
  assert.equal((await rpc("completion-state")).idle, true);
  checkpoint = "cleanup-retry";
  await extensionPage.evaluate(() => globalThis.__completionFixture.actions.retryCleanup());
  checkpoint = "client-idle";
  assert.equal(await extensionPage.evaluate(() => globalThis.__completionFixture.actions.idle()), true);
  checkpoint = "counter-after-retry";
  assert.equal(await fixture.evaluate(() => clicks), 2); pass(stage);
  stage = "host-cancel-joins-real-wait";
  current = await packet("Ready", "wait", { nameEquals: "Never", timeoutMs: 10000 });
  const waiting = run(current);
  await until(() => extensionPage.evaluate(async tabId => {
    const [row] = await chrome.scripting.executeScript({ target: { tabId: Number(tabId), frameIds: [0] }, world: "ISOLATED",
      func: () => !!globalThis.__grokComputerUseSnapshot?.operation }); return row.result;
  }, tabId));
  assert.equal((await rpc("completion-state")).idle, false);
  assert((await rpc("completion-cancel")).ok);
  assert.equal((await waiting).status, "unknown"); assert.equal((await rpc("completion-state")).idle, true);
  assert(!fixture.isClosed()); pass(stage);
  stage = "unpair-joins-original-and-cancel-script";
  current = await packet("Count", "click", {});
  await extensionPage.evaluate(() => { globalThis.__completionFixture.faults.delayScripts = true; });
  const delayed = run(current);
  await until(() => extensionPage.evaluate(() => typeof globalThis.__completionFixture.faults.releaseAction === "function"));
  assert.equal(await fixture.evaluate(() => clicks), 3);
  await extensionPage.evaluate(() => globalThis.__completionFixture.client.forget());
  await until(() => extensionPage.evaluate(() => typeof globalThis.__completionFixture.faults.releaseCancel === "function"));
  assert.equal((await rpc("status")).stored, false); assert.equal((await rpc("completion-state")).idle, false);
  await extensionPage.evaluate(() => globalThis.__completionFixture.faults.releaseCancel());
  assert.equal((await rpc("completion-state")).idle, false, "cancel reply cannot release the original native promise");
  await extensionPage.evaluate(() => globalThis.__completionFixture.faults.releaseAction());
  assert.equal((await delayed).status, "unknown"); assert.equal((await rpc("completion-state")).idle, true);
  assert.equal(await extensionPage.evaluate(() => globalThis.__completionFixture.actions.idle()), true);
  assert.equal(await fixture.evaluate(() => clicks), 3); assert(!fixture.isClosed()); pass(stage);
  process.stdout.write(JSON.stringify({ event: "passed", checks: passed }) + "\n");
} catch (error) {
  process.stderr.write(`completion live failed at ${stage}, checkpoint=${checkpoint}\n`);
  process.stderr.write(`completion failure flags: illegalInvocation=${/Illegal invocation/.test(String(error?.message))}\n`);
  process.stdout.write(JSON.stringify({ event: "failed", check: stage }) + "\n");
  process.exitCode = 1;
} finally {
  await extensionPage?.evaluate(async () => {
    const state = globalThis.__completionFixture;
    state?.faults.releaseAction?.(); state?.faults.releaseCancel?.();
    state?.actions.reset(); await state?.client.forget();
  }).catch(() => {});
  await context?.close().catch(() => {});
  await new Promise(resolve => server.close(resolve));
  lines.close();
}
