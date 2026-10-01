// Real MV3 executeScript + isolated-world DOM kernel. This is NOT Host/MCP/toolbar acceptance.
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { mkdtemp, realpath, readFile, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { createServer } from "node:http";
import { randomUUID } from "node:crypto";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { EXTENSION_ID } from "../computer-use-extension/pairing-client.mjs";

const require = createRequire(new URL("../computer-use-browser/package.json", import.meta.url));
const { chromium } = require("playwright-core");
const browserIndex = process.argv.indexOf("--browser");
assert(browserIndex >= 0 && process.argv[browserIndex + 1], "--browser is required");
const browser = await realpath(process.argv[browserIndex + 1]);
const extension = fileURLToPath(new URL("../computer-use-extension", import.meta.url));
const owner = randomUUID();
const profile = await mkdtemp(path.join(tmpdir(), "grok-cu-action-kernel-"));
const ownerPath = path.join(profile, ".probe-owner.json");
await writeFile(ownerPath, JSON.stringify({ owner, purpose: "cu-action-kernel", profile }), { flag: "wx" });
const html = `<!doctype html><meta charset="utf-8"><title>Owned action kernel fixture</title>
<style>body { margin:20px } input,textarea,button { display:block; margin:10px; }
#scroll { width:240px;height:80px;overflow-y:auto;scroll-behavior:smooth; }</style>
<input id="field" aria-label="Field"><textarea id="area" aria-label="Area"></textarea>
<button id="clicker">Run</button><button id="waiting">Pending</button>
<div id="scroll" role="button" aria-label="Scroller"><div style="height:1000px">Scrollable</div></div>
<form id="form" action="/submit" target="_self"></form><button id="submit" form="form">Submit</button>
<script>window.fixture={clicks:0,events:[]};
document.querySelector('#clicker').addEventListener('click',()=>fixture.clicks++);
for(const id of ['field','area']) for(const name of ['beforeinput','input','change'])
document.getElementById(id).addEventListener(name,e=>fixture.events.push({id,type:e.type,trusted:e.isTrusted}));
</script>`;
const server = createServer((_request, response) => {
  response.writeHead(200, { "content-type": "text/html; charset=utf-8" }); response.end(html);
});
let context;
const passed = [];
let stage = "launch";
try {
  await new Promise((resolve, reject) => { server.once("error", reject); server.listen(0, "127.0.0.1", resolve); });
  const url = `http://127.0.0.1:${server.address().port}/fixture`;
  context = await chromium.launchPersistentContext(profile, {
    executablePath: browser, headless: true, viewport: { width: 1100, height: 800 },
    args: ["--headless=new", "--disable-background-networking", "--disable-features=DisableLoadExtensionCommandLineSwitch",
      "--disable-extensions-except=" + extension, "--load-extension=" + extension],
  });
  context.setDefaultTimeout(10000);
  const worker = context.serviceWorkers()[0] || await context.waitForEvent("serviceworker", { timeout: 10000 });
  assert(worker.url().startsWith(`chrome-extension://${EXTENSION_ID}/`));
  // MV3 disallows dynamic import in service workers. The trusted extension page loads the fixed
  // source functions for this kernel probe; the production SW/Host action path is deliberately off.
  const extensionPage = await context.newPage();
  await extensionPage.goto(`chrome-extension://${EXTENSION_ID}/popup.html`);
  const page = await context.newPage(); await page.goto(url); await page.bringToFront();
  const tabId = await extensionPage.evaluate(async expectedUrl => {
    const tabs = await chrome.tabs.query({ active: true, lastFocusedWindow: true });
    if (tabs.length !== 1 || tabs[0].url !== expectedUrl) throw new Error("fixture not active");
    return tabs[0].id;
  }, url);
  const inject = (method, args, documentId = null) => extensionPage.evaluate(async ({ tabId, method, args, documentId }) => {
    const source = method === "observeDocument" ? "observe-document.mjs" : method === "fenceDocument" ? "document-execution.mjs" : "act-document.mjs";
    const module = await import(chrome.runtime.getURL(source));
    const results = await chrome.scripting.executeScript({ target: documentId ? { tabId, documentIds: [documentId] } : { tabId, frameIds: [0] },
      world: "ISOLATED", func: module[method], args });
    if (results.length !== 1 || results[0].frameId !== 0 || (documentId && results[0].documentId !== documentId)) throw new Error("wrong document");
    return { ...results[0].result, documentId: results[0].documentId };
  }, { tabId, method, args, documentId });
  const kernelEpoch = await extensionPage.evaluate(async () => {
    const { allocateWorkerEpoch } = await import(chrome.runtime.getURL("execution-clock.mjs"));
    return allocateWorkerEpoch();
  });
  let kernelSequence = 1;
  await inject("fenceDocument", [{ epoch: kernelEpoch, sequence: kernelSequence }]);
  const read = () => inject("observeDocument", [randomUUID(), true, { epoch: kernelEpoch, sequence: ++kernelSequence }]);
  const command = (observed, name, action, parameters) => {
    const node = observed.nodes.find(n => n.name === name); assert(node, `missing fixture node: ${name}`);
    return { operationId: randomUUID(), snapshotId: observed.snapshotId, elementRef: node.elementRef,
      action, parameters, deadlineMs: Date.now() + 10000 };
  };
  const act = (observed, value) => inject("actDocument", [value], observed.documentId);
  const check = async (name, work) => { stage = name; await work(); passed.push(name); };
  const entered = () => extensionPage.evaluate(async tabId => {
    const deadline = Date.now() + 2000;
    for (;;) {
      const [value] = await chrome.scripting.executeScript({ target: { tabId, frameIds: [0] }, world: "ISOLATED",
        func: () => Boolean(globalThis.__grokComputerUseSnapshot?.operation) });
      if (value.result) return;
      if (Date.now() >= deadline) throw new Error("wait not entered");
      await new Promise(resolve => setTimeout(resolve, 10));
    }
  }, tabId);

  await check("isolated-world-state-not-readable-by-page", async () => {
    await read(); assert.equal(await page.evaluate(() => globalThis.__grokComputerUseSnapshot), undefined);
  });
  await check("semantic-click-once-with-independent-counter", async () => {
    const observed = await read(); const req = command(observed, "Run", "click", {});
    assert.equal((await act(observed, req)).status, "applied");
    assert.equal((await act(observed, req)).status, "rejected");
    assert.equal(await page.evaluate(() => fixture.clicks), 1);
  });
  await check("cjk-emoji-native-value-events-and-selection", async () => {
    let observed = await read();
    assert.equal((await act(observed, command(observed, "Field", "set_value", { text: "你好🌍" }))).status, "verified");
    assert.equal(await page.locator("#field").inputValue(), "你好🌍");
    await page.locator("#field").evaluate(el => el.setSelectionRange(2, 4));
    observed = await read();
    const result = await act(observed, command(observed, "Field", "type_text", { text: "世界🚀" }));
    assert.equal(result.status, "verified"); assert(!JSON.stringify(result).includes("世界"));
    assert.equal(await page.locator("#field").inputValue(), "你好世界🚀");
    const events = await page.evaluate(() => fixture.events);
    assert.deepEqual(events.map(e => e.type), ["beforeinput", "input", "change", "beforeinput", "input"]);
    assert(events.every(e => e.trusted === false), "semantic input must not claim trusted OS input");
    assert(!JSON.stringify(await read()).includes("你好世界"));
    observed = await read();
    assert.equal((await act(observed, command(observed, "Area", "set_value", { text: "第一行\n第二行🚀" }))).status, "verified");
    assert.equal(await page.locator("#area").inputValue(), "第一行\n第二行🚀");
  });
  await check("original-node-remove-reinsert-and-identity-aba-rejected", async () => {
    for (const mutation of ["remove", "replace", "disable"]) {
      const observed = await read(); const req = command(observed, "Run", "click", {});
      await page.locator("#clicker").evaluate((el, kind) => {
        if (kind === "remove") { const next = el.nextSibling; const parent = el.parentNode; el.remove(); parent.insertBefore(el, next); }
        if (kind === "replace") el.replaceWith(el.cloneNode(true));
        if (kind === "disable") { el.disabled = true; el.disabled = false; }
      }, mutation);
      assert.equal((await act(observed, req)).status, "rejected");
      assert.equal(await page.evaluate(() => fixture.clicks), 1);
    }
  });
  await check("associated-form-action-aba-rejected", async () => {
    const observed = await read(); const req = command(observed, "Submit", "click", {});
    await page.locator("#form").evaluate(form => { form.action = "/other"; form.action = "/submit"; });
    assert.equal((await act(observed, req)).status, "rejected"); assert.equal(page.url(), url);
  });
  await check("scroll-real-position-and-boundary-without-smooth-tail", async () => {
    let observed = await read();
    assert.equal((await act(observed, command(observed, "Scroller", "scroll", { delta: 240 }))).status, "verified");
    assert.equal(await page.locator("#scroll").evaluate(el => el.scrollTop), 240);
    // Wait for the browser's actual scroll notification before making the next model observation.
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    observed = await read();
    assert.equal((await act(observed, command(observed, "Scroller", "scroll", { delta: 2400 }))).status, "verified");
    assert.equal(await page.locator("#scroll").evaluate(el => el.scrollTop), 920);
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    observed = await read();
    const result = await act(observed, command(observed, "Scroller", "scroll", { delta: 240 }));
    assert.equal(result.status, "verified"); assert.equal(result.detail, "scroll_boundary");
  });
  await check("wait-matches-original-ref-and-preview-preserves-it", async () => {
    const observed = await read(); const req = command(observed, "Pending", "wait", { nameEquals: "Ready", timeoutMs: 2000 });
    const waiting = act(observed, req);
    try {
      await entered();
      await inject("observeDocument", [randomUUID(), false, { epoch: kernelEpoch, sequence: ++kernelSequence }], observed.documentId);
      await page.locator("#waiting").evaluate(el => { el.textContent = "Ready"; });
      assert.equal((await waiting).status, "verified");
      assert.equal((await act(observed, req)).detail, "duplicate_operation");
    } finally { await inject("cancelDocumentOperation", [req.snapshotId, req.operationId], observed.documentId); await waiting; }
  });
  await check("cancel-wait-joins-and-late-entry-cannot-click", async () => {
    const observed = await read(); const req = command(observed, "Ready", "wait", { nameEquals: "Never", timeoutMs: 10000 });
    const waiting = act(observed, req);
    try {
      await entered();
      const cancellation = await inject("cancelDocumentOperation", [req.snapshotId, req.operationId], observed.documentId);
      assert.equal(cancellation.status, "settled"); assert.equal((await waiting).status, "rejected");
      assert.equal((await act(observed, command(observed, "Run", "click", {}))).status, "rejected");
      assert.equal(await page.evaluate(() => fixture.clicks), 1);
      assert(!page.isClosed(), "cancellation never closes the borrowed tab");
    } finally { await inject("cancelDocumentOperation", [req.snapshotId, req.operationId], observed.documentId); await waiting; }
  });
  await check("beforeinput-prevention-does-not-fake-success", async () => {
    await page.locator("#field").evaluate(el => el.addEventListener("beforeinput", e => e.preventDefault(), { once: true }));
    const observed = await read();
    assert.equal((await act(observed, command(observed, "Field", "set_value", { text: "forbidden" }))).status, "unknown");
    assert.equal(await page.locator("#field").inputValue(), "你好世界🚀");
  });
  await check("switch-away-and-back-retires-observed-authority", async () => {
    // Headless Chrome can coalesce/suppress DOM focus events. The production sharing controller
    // must retain the observation's extension activity epoch as well. Only the Host client is a
    // no-network fixture here; actual Chrome tab/document events and fixed scripts are unchanged.
    const snapshotId = randomUUID(); const requestId = randomUUID();
    const observed = await extensionPage.evaluate(async ({ tabId, snapshotId, requestId }) => {
      const { SharedTabs } = await import(chrome.runtime.getURL("shared-tabs.mjs"));
      const { allocateWorkerEpoch, ExecutionClock } = await import(chrome.runtime.getURL("execution-clock.mjs"));
      const sharing = new SharedTabs({ client: { connectionEpoch: 1, shareCurrentTab: async () => {}, unshareTab: async () => {} },
        clock: new ExecutionClock(allocateWorkerEpoch()), tabs: chrome.tabs, scripting: chrome.scripting });
      globalThis.__kernelSharing = sharing;
      chrome.tabs.onActivated.addListener(() => sharing.activityChanged());
      chrome.windows.onFocusChanged.addListener(() => sharing.activityChanged());
      await sharing.share(tabId);
      globalThis.__kernelRequest = { requestId, tabId: String(tabId), session: "kernel", runId: "kernel-run",
        documentGeneration: 1, grantGeneration: 1, deadlineMs: Date.now() + 10000,
        command: { kind: "observe", snapshotId, screenshot: false, preview: false } };
      const observed = await sharing.observe(globalThis.__kernelRequest);
      globalThis.__kernelRequest.documentId = observed.documentId;
      return observed;
    }, { tabId, snapshotId, requestId });
    const req = command(observed, "Run", "click", {});
    const other = await context.newPage();
    try {
      await other.goto(url + "?other"); await other.bringToFront(); await page.bringToFront();
      const result = await extensionPage.evaluate(req => globalThis.__kernelSharing.act({ ...globalThis.__kernelRequest,
        requestId: req.operationId, command: { kind: "act", snapshotId: req.snapshotId, elementRef: req.elementRef,
          action: req.action, parameters: req.parameters } }), req);
      assert.equal(result.status, "rejected");
      assert.equal(await page.evaluate(() => fixture.clicks), 1);
      assert.equal(await other.evaluate(() => fixture.clicks), 0);
    } finally { await other.close(); }
  });
  await check("sharing-controller-dispatches-with-current-observed-scope", async () => {
    const result = await extensionPage.evaluate(async ids => {
      const request = { ...globalThis.__kernelRequest, requestId: ids[0], deadlineMs: Date.now() + 10000,
        command: { kind: "observe", snapshotId: ids[1], screenshot: false, preview: false } };
      const observed = await globalThis.__kernelSharing.observe(request);
      return globalThis.__kernelSharing.act({ ...request, documentId: observed.documentId, requestId: ids[2], command: { kind: "act",
        snapshotId: ids[1], elementRef: observed.nodes.find(n => n.name === "Field").elementRef,
        action: "set_value", parameters: { text: "当前授权✅" } } });
    }, [randomUUID(), randomUUID(), randomUUID()]);
    assert.equal(result.status, "verified"); assert.equal(result.physicallySettled, true);
    assert.equal(await page.locator("#field").inputValue(), "当前授权✅");
  });
  await check("sharing-controller-cancels-real-wait-before-releasing-admission", async () => {
    const running = extensionPage.evaluate(async ids => {
      const request = { ...globalThis.__kernelRequest, requestId: ids[0], deadlineMs: Date.now() + 10000,
        command: { kind: "observe", snapshotId: ids[1], screenshot: false, preview: false } };
      const observed = await globalThis.__kernelSharing.observe(request);
      globalThis.__scopeAbort = new AbortController();
      return globalThis.__kernelSharing.act({ ...request, documentId: observed.documentId, requestId: ids[2], command: { kind: "act",
        snapshotId: ids[1], elementRef: observed.nodes.find(n => n.name === "Ready").elementRef,
        action: "wait", parameters: { nameEquals: "Never", timeoutMs: 10000 } } }, globalThis.__scopeAbort.signal);
    }, [randomUUID(), randomUUID(), randomUUID()]);
    try {
      await entered();
      assert.equal(await extensionPage.evaluate(() => globalThis.__kernelSharing.actionsIdle()), false);
      await extensionPage.evaluate(() => globalThis.__scopeAbort.abort());
      const result = await running;
      assert.equal(result.status, "unknown"); assert.equal(result.physicallySettled, true);
      assert.equal(await extensionPage.evaluate(() => globalThis.__kernelSharing.actionsIdle()), true);
      assert(!page.isClosed());
    } finally { await extensionPage.evaluate(() => globalThis.__scopeAbort?.abort()); await running; }
  });
  await check("same-url-reload-cannot-reuse-chrome-document-id", async () => {
    const observed = await extensionPage.evaluate(snapshotId => globalThis.__kernelSharing.observe({
      ...globalThis.__kernelRequest, deadlineMs: Date.now() + 10000,
      command: { kind: "observe", snapshotId, screenshot: false, preview: false },
    }), randomUUID());
    const req = command(observed, "Run", "click", {});
    await page.reload();
    await assert.rejects(act(observed, req));
    assert.equal(await page.evaluate(() => fixture.clicks), 0);
  });
  stage = "evidence";
  console.log(JSON.stringify({ scope: "MV3 DOM kernel and sharing controller; Host authorization client is a fixture, no Host action capability enabled", node: process.version,
    browser: await page.evaluate(() => navigator.userAgent), passed }));
} catch (error) {
  console.error(JSON.stringify({ stage, passed, error: error.message })); process.exitCode = 1;
} finally {
  await context?.close();
  server.closeAllConnections(); await new Promise(resolve => server.close(resolve));
  const marker = JSON.parse(await readFile(ownerPath, "utf8"));
  const resolved = await realpath(profile);
  assert.equal(marker.owner, owner); assert.equal(marker.purpose, "cu-action-kernel"); assert.equal(marker.profile, profile);
  assert.equal(path.dirname(resolved).toLowerCase(), (await realpath(tmpdir())).toLowerCase());
  assert(path.basename(resolved).startsWith("grok-cu-action-kernel-"));
  await rm(resolved, { recursive: true });
  console.log("Owned kernel fixture browser, loopback server and temporary profile released");
}
