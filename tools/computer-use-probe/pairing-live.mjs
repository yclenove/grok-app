// Real MV3 popup -> service worker -> product Rust IPC. Never uses a daily profile.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createRequire } from "node:module";
import { createInterface } from "node:readline";
import { mkdtemp, readFile, realpath, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { setTimeout as delay } from "node:timers/promises";
import { createServer } from "node:http";
import { EXTENSION_ID } from "../computer-use-extension/pairing-client.mjs";
import { verifySharing } from "./sharing-live.mjs";
import { verifyToolbarSharing } from "./toolbar-sharing-live.mjs";
import { reportPairingFailure } from "./pairing-diagnostics.mjs";

const root = fileURLToPath(new URL("../../", import.meta.url));
const require = createRequire(new URL("../computer-use-browser/package.json", import.meta.url));
const { chromium } = require("playwright-core");
const args = process.argv.slice(2);
const option = key => args[args.indexOf(key) + 1];
if (!args.includes("--browser")) throw new Error("--browser must name an isolated-test capable Chromium binary");
const appHost = args.includes("--app-host");
const toolbarGesture = args.includes("--toolbar-gesture");
const host = appHost ? null : spawn(option("--host"), [], { stdio: ["pipe", "pipe", "pipe"], windowsHide: true });
const input = appHost ? process.stdin : host.stdout;
const output = appHost ? process.stdout : host.stdin;
const lines = createInterface({ input });
host?.stderr.on("data", () => { /* Never forward raw child output into evidence. */ });
let pending;
lines.on("line", line => {
  if (!pending) return;
  const resolve = pending; pending = null; resolve(JSON.parse(line));
});
async function rpc(command, extra = {}) {
  assert(!pending, "probe control requests must be serial");
  let timer;
  const reply = new Promise((resolve, reject) => {
    pending = resolve;
    timer = setTimeout(() => { pending = null; reject(new Error("Host fixture timeout")); }, 15000);
  });
  output.write(JSON.stringify({ command, ...extra }) + "\n");
  try { return await reply; } finally { clearTimeout(timer); }
}

// The App parent owns process-tree/profile cleanup even if this Node process dies.
if (appHost) assert((await rpc("ready")).ok === true, "parent must acquire process ownership first");
const externallyOwned = args.includes("--owned-profile");
const profile = externallyOwned ? await realpath(option("--owned-profile"))
  : await mkdtemp(path.join(tmpdir(), "grok-cu-pairing-live-"));
if (externallyOwned) {
  const owner = JSON.parse(await readFile(path.join(path.dirname(profile), "owner.json"), "utf8"));
  assert.equal(owner.owner, option("--profile-owner"));
  assert.equal(owner.purpose, "cu-pairing-probe");
  assert.equal(await realpath(owner.profile), profile);
} else {
  await writeFile(path.join(profile, ".probe-owner.json"), JSON.stringify({ pid: process.pid, purpose: "cu-pairing-probe" }), { flag: "wx" });
}
let context;
let popup;
let check = "launch";
const passed = [];
const requests = [];
const lifecycle = [];
const startedAt = performance.now();
const recordLifecycle = event => {
  lifecycle.push({ event, stage: check, elapsedMs: Math.round(performance.now() - startedAt) });
  if (lifecycle.length > 32) lifecycle.shift();
};
const fixtureServer = createServer((_request, response) => {
  response.writeHead(200, { "content-type": "text/html; charset=utf-8" });
  response.end("<title>Computer Use Fixture</title><main><p>Visible fixture 你好</p><button id='fixture-action'>Fixture action</button><input id='fixture-input' value='VALUE_SECRET'><input type='password' aria-label='PASSWORD_SECRET' value='PASSWORD_VALUE'><p hidden>HIDDEN_SECRET</p><textarea>TEXTAREA_SECRET</textarea></main>");
});
await new Promise((resolve, reject) => {
  fixtureServer.once("error", reject);
  fixtureServer.listen(0, "127.0.0.1", resolve);
});
const fixtureUrl = `http://127.0.0.1:${fixtureServer.address().port}/fixture`;
async function until(predicate, timeout = 15_000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) { if (await predicate()) return; await delay(100); }
  throw new Error("UI postcondition timeout");
}
try {
  const extension = path.join(root, "tools/computer-use-extension");
  const launch = async () => {
    const browserContext = await chromium.launchPersistentContext(profile, {
    executablePath: option("--browser"), headless: !toolbarGesture, locale: "en-US",
    args: [...(toolbarGesture ? [] : ["--headless=new"]), "--lang=en-US", "--disable-background-networking",
      "--disable-features=DisableLoadExtensionCommandLineSwitch",
      "--disable-extensions-except=" + extension, "--load-extension=" + extension],
    });
    // Passive lifecycle evidence: do not poll extension APIs during the idle test.
    const watched = new WeakSet();
    const watchWorker = candidate => {
      if (candidate.url() !== `chrome-extension://${EXTENSION_ID}/sw.js` || watched.has(candidate)) return;
      watched.add(candidate); recordLifecycle("worker-created");
      candidate.on("close", () => recordLifecycle("worker-closed"));
    };
    browserContext.on("serviceworker", watchWorker);
    browserContext.serviceWorkers().forEach(watchWorker);
    browserContext.on("close", () => recordLifecycle("context-closed"));
    browserContext.on("response", response => {
    const url = new URL(response.url());
    if (url.hostname === "127.0.0.1" && url.pathname.startsWith("/cu/")) {
      requests.push({ path: url.pathname, status: response.status() });
      if (requests.length > 128) requests.shift();
    }
    });
    return browserContext;
  };
  context = await launch();
  const worker = context.serviceWorkers()[0] || await context.waitForEvent("serviceworker");
  assert(worker.url().startsWith("chrome-extension://" + EXTENSION_ID + "/"), "stable extension identity");
  passed.push("stable-extension-id");
  popup = await context.newPage();
  await popup.goto("chrome-extension://" + EXTENSION_ID + "/popup.html");
  const waitStatus = text => until(async () => await popup.locator("#status").textContent() === text);
  await waitStatus("Not paired.");
  check = "begin-challenge";
  let challenge = await rpc("begin");
  const publicCheck = await worker.evaluate(async endpoint => {
    const response = await fetch(endpoint + "/cu/pairing-challenge", {
      method: "POST", headers: { "content-type": "application/json" }, body: "{}"
    });
    return { status: response.status };
  }, challenge.endpoint);
  assert.equal(publicCheck.status, 200, "public challenge must be readable by the extension");
  const pair = async () => {
    await popup.locator("#endpoint").fill(challenge.endpoint);
    await popup.locator("#code").fill(challenge.code);
    await popup.locator("#pair").click();
    await until(async () => await popup.locator("#pair").isEnabled());
    assert((await popup.locator("#code").inputValue()) === "", "code must be cleared after submission");
  };

  check = "manual-confirmation";
  assert((await rpc("status")).paired === false, "opening popup cannot pair");
  check = "reject-without-app-confirmation";
  await pair();
  await waitStatus("Check the App address and code, then try again.");
  assert((await rpc("status")).paired === false, "extension alone cannot pair");
  check = "confirm-in-app";
  assert((await rpc("confirm", { nonce: challenge.nonce })).ok === true);
  check = "pair-with-app-confirmation";
  await pair();
  await waitStatus("Paired with Grok App.");
  assert((await rpc("status")).paired === true, "Host must observe authenticated pairing");
  passed.push(check);

  check = "no-popup-key-message";
  const exposed = await popup.evaluate(() => chrome.runtime.sendMessage({ type: "cu-status" }));
  assert(exposed.ok && exposed.paired && !Object.hasOwn(exposed, "sessionKey"));
  const unknown = await popup.evaluate(async () => {
    try { return await chrome.runtime.sendMessage({ type: "cu-get-key" }); } catch { return null; }
  });
  assert(!unknown || !Object.hasOwn(unknown, "key"));
  passed.push(check);

  if (toolbarGesture) {
    await verifyToolbarSharing({ context, popup, rpc, until,
      stage: name => { check = name; }, passed: name => passed.push(name) });
    popup = null;
  } else {
  check = "extension-revoke";
  await popup.locator("#forget").click();
  await waitStatus("Not paired.");
  assert((await rpc("status")).paired === false, "extension revoke must clear Host authority");
  passed.push(check);

  check = "app-revoke";
  // Separate scenarios, not retries; pairing is intentionally rate limited.
  await delay(900);
  challenge = await rpc("begin");
  await rpc("confirm", { nonce: challenge.nonce });
  await pair(); await waitStatus("Paired with Grok App.");
  await rpc("revoke");
  // Heartbeat failure must update a popup already on screen; no reload/user interaction.
  await waitStatus("Not paired.");
  const empty = await popup.evaluate(async () => Object.keys(await chrome.storage.session.get(null)).length === 0);
  assert(empty, "revoked key must leave session storage");
  passed.push(check);

  check = "worker-restart";
  await delay(900);
  challenge = await rpc("begin");
  await rpc("confirm", { nonce: challenge.nonce });
  await pair(); await waitStatus("Paired with Grok App.");
  const cdp = await context.newCDPSession(popup);
  const { targetInfos } = await cdp.send("Target.getTargets");
  const workerTarget = targetInfos.find(target => target.type === "service_worker" && target.url === worker.url());
  assert(workerTarget, "extension worker target must be present");
  await cdp.send("Target.closeTarget", { targetId: workerTarget.targetId });
  await popup.reload(); await waitStatus("Not paired.");
  assert((await rpc("status")).paired === false, "worker restart must revoke old Host pairing");
  passed.push(check);
  // Do not emit a screenshot while the one-time code is on screen.
  if (args.includes("--screenshot")) await popup.screenshot({ path: option("--screenshot") });

  check = "heartbeat-survives-without-popup";
  await delay(900);
  challenge = await rpc("begin");
  await rpc("confirm", { nonce: challenge.nonce });
  await pair(); await waitStatus("Paired with Grok App.");
  // Fixture credentials stay in memory, never in evidence, arguments or files.
  const old = await popup.evaluate(async () => (await chrome.storage.session.get("cuPairing")).cuPairing);
  recordLifecycle("heartbeat-popup-closing");
  await popup.close(); popup = null;
  await delay(35000);
  assert((await rpc("status")).paired === true, "worker must renew without an open popup");
  passed.push(check);

  check = "browser-close-expires-host-key";
  await context.close(); context = null;
  const expired = async () => {
    const state = await rpc("status");
    return state.paired === false && state.stored === false;
  };
  await until(expired, 35000);
  passed.push(check);

  const replay = async () => {
    const { instanceId, connectionNonce, generation } = old;
    const response = await fetch(old.endpoint + "/cu/extension-status", {
      method: "POST", redirect: "error", signal: AbortSignal.timeout(5000),
      headers: { "content-type": "application/json", origin: "chrome-extension://" + EXTENSION_ID,
        authorization: "Bearer " + old.sessionKey },
      body: JSON.stringify({ instanceId, connectionNonce, generation }),
    });
    assert.equal(response.status, 403, "expired identity must be rejected by product HTTP");
  };
  check = "late-heartbeat-rejected";
  await replay(); assert(await expired(), "late heartbeat must not restore Host state");
  passed.push(check);

  check = "old-heartbeat-cannot-touch-new-pairing";
  context = await launch();
  popup = await context.newPage();
  await popup.goto("chrome-extension://" + EXTENSION_ID + "/popup.html");
  await waitStatus("Not paired.");
  challenge = await rpc("begin");
  await rpc("confirm", { nonce: challenge.nonce });
  await pair(); await waitStatus("Paired with Grok App.");
  await replay();
  assert((await rpc("status")).paired === true, "old heartbeat must not retire the new connection");
  passed.push(check);

  await verifySharing({ context, popup, fixtureUrl, rpc, until, appHost,
    stage: name => { check = name; }, passed: name => passed.push(name) });

  }
  if (appHost) output.write(JSON.stringify({ event: "passed", checks: passed }) + "\n");
  else console.log(JSON.stringify({ status: "passed", level: "E2-real-extension-real-IPC", checks: passed }));
} catch (error) {
  await reportPairingFailure({ check, error, context, popup, extensionId: EXTENSION_ID,
    fixtureUrl, requests, lifecycle });
  if (appHost) output.write(JSON.stringify({ event: "failed", check }) + "\n");
  process.exitCode = 1;
} finally {
  await context?.close().catch(() => {});
  lines.close();
  if (host) {
    host.stdin.end(JSON.stringify({ command: "shutdown" }) + "\n");
    if (host.exitCode === null && host.signalCode === null) {
      const exited = new Promise(resolve => host.once("exit", resolve));
      const timer = setTimeout(() => host.kill(), 5000);
      await exited; clearTimeout(timer);
    }
  }
  await new Promise(resolve => fixtureServer.close(() => resolve()));
  if (!externallyOwned) {
    const resolved = await realpath(profile);
    const temp = await realpath(tmpdir());
    const owner = JSON.parse(await readFile(path.join(resolved, ".probe-owner.json"), "utf8"));
    if (path.dirname(resolved) !== temp || !path.basename(resolved).startsWith("grok-cu-pairing-live-")
      || owner.pid !== process.pid || owner.purpose !== "cu-pairing-probe") {
      throw new Error("probe profile cleanup escaped its owner directory");
    }
    await rm(resolved, { recursive: true, force: true, maxRetries: 4 });
  }
}
