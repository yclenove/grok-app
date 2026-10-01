// Native startup evidence only: install once, then reopen the same owned profile.
// Never invokes onStartup listeners, clears storage, or uses a daily profile.
import assert from "node:assert/strict";
import { mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { setTimeout as delay } from "node:timers/promises";
import { EXTENSION_ID } from "../computer-use-extension/pairing-client.mjs";
import { OwnedCdpBrowser } from "./owned-cdp-browser.mjs";

const args = process.argv.slice(2);
assert.equal(args.length, 2, "usage: node browser-startup-live.mjs <isolated-test browser> <restart rounds>");
const [executablePath, roundText] = args;
const rounds = Number(roundText);
assert(Number.isInteger(rounds) && rounds >= 1 && rounds <= 10);
const root = await mkdtemp(path.join(tmpdir(), "grok-cu-browser-startup-"));
const profile = path.join(root, "profile");
await writeFile(path.join(root, "owner.json"), JSON.stringify({ purpose: "cu-native-startup-probe", pid: process.pid, profile }), { flag: "wx" });
const extension = fileURLToPath(new URL("../computer-use-extension", import.meta.url));
const popupUrl = `chrome-extension://${EXTENSION_ID}/popup.html`;
let context;
let stage = "initial-launch";
const evidence = { profile, installation: "CDP Extensions.loadUnpacked once", rounds: [], reloadPreservedIdentity: false };

async function launch() {
  context = new OwnedCdpBrowser(executablePath, profile);
  await context.send("Browser.getVersion");
  return context;
}

async function snapshot(page, predicate = () => true) {
  const deadline = Date.now() + 10000;
  while (Date.now() < deadline) {
    const current = await context.evaluate(page, '(async () => typeof chrome === "object" && chrome.storage?.local ? (await chrome.storage.local.get("cuBrowserSession")).cuBrowserSession : null)()');
    if (current && predicate(current)) return current;
    await delay(50);
  }
  throw new Error("native browser session postcondition timeout");
}

try {
  await launch();
  stage = "install-once";
  evidence.browser = (await context.send("Browser.getVersion")).product;
  const installed = await context.send("Extensions.loadUnpacked", { path: extension });
  assert.equal(installed.id, EXTENSION_ID);
  stage = "read-installed-session";
  let popup = await context.page(popupUrl);
  let prior = await snapshot(popup);
  assert.equal(prior.started, false, "installing in a running browser must not fabricate startup");
  evidence.installedStarted = prior.started;
  process.stdout.write("native extension install: PASS (no fabricated startup)\n");
  for (let round = 0; round < rounds; round++) {
    stage = `close-browser-${round + 1}`;
    await context.close(); context = null;
    stage = `reopen-browser-${round + 1}`;
    await launch();
    popup = await context.page(popupUrl);
    stage = `verify-startup-${round + 1}`;
    const current = await snapshot(popup, value => value.started && value.id !== prior.id);
    assert.equal(current.previousId, prior.id);
    evidence.rounds.push({ round: round + 1, rotated: true, previousMatched: true, started: current.started });
    process.stdout.write(`native browser restart ${round + 1}: PASS\n`);
    prior = current;
  }
  stage = "reload-extension-not-browser";
  await context.evaluate(popup, 'setTimeout(() => chrome.runtime.reload(), 0); true');
  await delay(250);
  popup = await context.page(popupUrl);
  assert.deepEqual(await snapshot(popup), prior, "extension reload cannot manufacture browser startup");
  evidence.reloadPreservedIdentity = true;
  evidence.status = "passed";
} catch (error) {
  evidence.status = "failed";
  evidence.stage = stage;
  evidence.error = error.message;
  if (context) {
    try {
      const { targetInfos } = await context.send("Target.getTargets");
      evidence.targets = targetInfos.map(({ type, url }) => ({ type, url }));
    } catch { evidence.targetInspection = "unavailable"; }
  }
  process.exitCode = 1;
} finally {
  try { await context?.close(); } catch (error) {
    evidence.cleanupError = error.message; evidence.status = "failed"; process.exitCode = 1;
  }
  await writeFile(path.join(root, "evidence.json"), JSON.stringify(evidence, null, 2));
  process.stdout.write(JSON.stringify(evidence) + "\n");
}
