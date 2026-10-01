import test from "node:test";
import assert from "node:assert/strict";
import { randomBytes } from "node:crypto";
import { spawn } from "node:child_process";
import {
  existsSync,
  lstatSync,
  mkdtempSync,
  readdirSync,
  realpathSync,
  rmSync,
} from "node:fs";
import { createServer, request } from "node:http";
import {
  PNG_SIGNATURE,
  inspectPng,
  modelObservationView,
} from "./observation-extract.mjs";
import { tmpdir } from "node:os";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { childEnv } from "./child-env.mjs";
import { confirmWorkerShutdown } from "./fixtures/cleanup-reconcile.mjs";
import { finishOwnedCleanup } from "./fixtures/cleanup-verified.mjs";

const ROOT = fileURLToPath(new URL(".", import.meta.url));
const CHROME_CANDIDATES = [
  process.env.GROK_CU_TEST_CHROME,
  "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
  "C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe",
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
  "/usr/bin/google-chrome-stable",
  "/usr/bin/google-chrome",
  "/usr/bin/chromium-browser",
  "/usr/bin/chromium",
].filter(Boolean);

function chromePath() {
  return CHROME_CANDIDATES.find((candidate) => existsSync(candidate)) || null;
}

function boundedAppend(current, chunk) {
  return `${current}${chunk.toString("utf8")}`.slice(-16_384);
}

function post(port, path, body, token, { host = false, timeoutMs = 15_000 } = {}) {
  return new Promise((resolvePromise, reject) => {
    const data = Buffer.from(JSON.stringify(body || {}), "utf8");
    const headers = {
      authorization: `Bearer ${token}`,
      "content-type": "application/json",
      "content-length": data.length,
    };
    if (host) headers["x-grok-cu-host"] = "1";
    const req = request(
      { hostname: "127.0.0.1", port, path, method: "POST", headers },
      (res) => {
        const chunks = [];
        let total = 0;
        res.on("data", (chunk) => {
          total += chunk.length;
          if (total <= 256 * 1024) chunks.push(chunk);
        });
        res.on("end", () => {
          if (total > 256 * 1024) {
            reject(new Error(`worker response exceeded test cap: ${total}`));
            return;
          }
          const raw = Buffer.concat(chunks).toString("utf8");
          try {
            resolvePromise({ status: res.statusCode, body: JSON.parse(raw) });
          } catch {
            reject(new Error(`worker returned invalid JSON: ${raw.slice(0, 200)}`));
          }
        });
      },
    );
    req.setTimeout(timeoutMs, () => req.destroy(new Error(`POST ${path} timed out`)));
    req.on("error", reject);
    req.end(data);
  });
}

function listen(server) {
  return new Promise((resolvePromise, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => resolvePromise(server.address().port));
  });
}

function closeServer(server) {
  return new Promise((resolvePromise) => server.close(() => resolvePromise()));
}

function waitForExit(child, timeoutMs) {
  if (child.exitCode != null || child.signalCode != null) {
    return Promise.resolve(child.exitCode);
  }
  return new Promise((resolvePromise, reject) => {
    const timer = setTimeout(() => {
      cleanup();
      reject(new Error(`worker ${child.pid} did not exit within ${timeoutMs}ms`));
    }, timeoutMs);
    const cleanup = () => {
      clearTimeout(timer);
      child.off("exit", onExit);
    };
    const onExit = (code) => {
      cleanup();
      resolvePromise(code);
    };
    child.once("exit", onExit);
  });
}

function processAlive(pid) {
  if (!Number.isInteger(pid) || pid <= 0) return false;
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

async function waitForPidsToExit(pids, timeoutMs = 10_000) {
  const deadline = Date.now() + timeoutMs;
  let live = pids.filter(processAlive);
  while (live.length && Date.now() < deadline) {
    await new Promise((resolvePromise) => setTimeout(resolvePromise, 50));
    live = pids.filter(processAlive);
  }
  return live;
}

function removeOwnedTempRoot(root) {
  const base = `${realpathSync(tmpdir())}${sep}`.toLowerCase();
  const target = `${resolve(root)}${sep}`.toLowerCase();
  assert.equal(target.startsWith(base), true, `refusing to remove non-temp root: ${root}`);
  assert.equal(relative(realpathSync(tmpdir()), resolve(root)).startsWith(".."), false);
  rmSync(root, { recursive: true, force: true });
}

async function startHarness(t) {
  const executable = chromePath();
  assert.ok(executable, "a supported Chrome/Chromium executable is required for this contract");
  const runtimeRoot = dirname(executable);
  const runtimeFiles = () => readdirSync(runtimeRoot, { recursive: true }).sort().map(path => {
    const { size, mtimeMs } = lstatSync(join(runtimeRoot, path));
    return { path, size, mtimeMs };
  });
  const initialRuntimeFiles = runtimeFiles();

  const headings = Array.from(
    { length: 70 },
    (_, index) => `<h2 role="heading">noise-${index}</h2>`,
  ).join("");
  const html = `<!doctype html>
    <meta charset="utf-8">
    <title>Observation contract</title>
    ${headings}
    <button id="late-button" onclick="fetch('/clicked', {method:'POST'})">Late button</button>
    <input aria-label="Name field">
    <select aria-label="Choice"><option>One</option><option>Two</option></select>
    <div tabindex="0" aria-label="Focusable custom control">Focusable custom control</div>
    <button hidden>SECRET_HIDDEN_BUTTON</button>
    <div inert><a href="/">SECRET_INERT_LINK</a></div>
    <div aria-hidden="true"><input aria-label="SECRET_ARIA_HIDDEN_INPUT"></div>`;
  let clickCount = 0;
  const pageServer = createServer((req, res) => {
    if (req.url === "/clicked" && req.method === "POST") {
      clickCount += 1; res.writeHead(204); res.end(); return;
    }
    res.writeHead(200, { "content-type": "text/html; charset=utf-8" });
    res.end(html);
  });
  const pagePort = await listen(pageServer);

  const token = randomBytes(32).toString("hex");
  const tempRoot = mkdtempSync(join(realpathSync(tmpdir()), "grok-cu-observe-contract-"));
  let workerOutput = "";
  const child = spawn(process.execPath, [join(ROOT, "server.mjs")], {
    cwd: ROOT,
    stdio: ["ignore", "pipe", "pipe"],
    env: childEnv({
      GROK_CU_BROWSER_TOKEN: token,
      GROK_CU_BROWSER_PORT: "0",
      GROK_CU_BROWSER_PROFILE_ROOT: join(tempRoot, "profiles"),
      GROK_CU_BROWSER_STAGING_ROOT: join(tempRoot, "staging"),
      GROK_CU_CHROME: executable,
    }),
  });
  child.stdout.on("data", (chunk) => {
    workerOutput = boundedAppend(workerOutput, chunk);
  });
  child.stderr.on("data", (chunk) => {
    workerOutput = boundedAppend(workerOutput, chunk);
  });

  const workerPort = await new Promise((resolvePromise, reject) => {
    const timer = setTimeout(
      () => reject(new Error(`worker port timeout: ${workerOutput}`)),
      15_000,
    );
    let stdout = "";
    const onData = (chunk) => {
      stdout += chunk.toString("utf8");
      for (const line of stdout.split(/\r?\n/)) {
        try {
          const parsed = JSON.parse(line);
          if (Number.isInteger(parsed.port) && parsed.port > 0) {
            clearTimeout(timer);
            child.stdout.off("data", onData);
            resolvePromise(parsed.port);
            return;
          }
        } catch {
          // Wait for a complete JSON line.
        }
      }
    };
    child.stdout.on("data", onData);
    child.once("error", (error) => {
      clearTimeout(timer);
      reject(error);
    });
    child.once("exit", (code) => {
      clearTimeout(timer);
      reject(new Error(`worker exited ${code}: ${workerOutput}`));
    });
  });

  const ownedPids = new Set([child.pid]);
  let cleaned = false;
  t.after(async () => {
    if (cleaned) return;
    cleaned = true;
    const pids = await post(workerPort, "/pids", {}, token, { host: true }).catch(() => null);
    if (Array.isArray(pids?.body?.browserPids)) {
      for (const pid of pids.body.browserPids) ownedPids.add(Number(pid));
    }
    await finishOwnedCleanup({
      shutdown: async () => {
        const shutdown = await confirmWorkerShutdown((path, timeoutMs) =>
          post(workerPort, path, {}, token, { host: true, timeoutMs }), {
          budgetMs: 15_000,
          onPending: () => t.diagnostic("shutdown pending; observing cleanup within the original 15s transport budget"),
        });
        assert.equal(shutdown.status, 200, `shutdown unconfirmed; retain ${tempRoot}`);
        assert.equal(shutdown.body.shutdown, true);
      },
      workerExit: async () => {
        try {
          assert.equal(await waitForExit(child, 10_000), 0, "owned worker must exit cleanly");
        } catch (error) {
          // This exact child is owned by the fixture. Forced exit cannot turn
          // the failed native gate green or authorize deleting its profile.
          if (child.exitCode == null && child.signalCode == null) child.kill();
          await waitForExit(child, 5_000).catch(() => null);
          throw error;
        }
      },
      fixtureClose: () => closeServer(pageServer),
      browserExit: async () => {
        assert.equal(Array.isArray(pids?.body?.browserPids), true, "owned browser inventory was not confirmed");
        const live = await waitForPidsToExit([...ownedPids]);
        assert.deepEqual(live, [], `worker/browser PIDs remained alive: ${live.join(",")}`);
      },
      removeProfile: () => removeOwnedTempRoot(tempRoot),
    });
    assert.equal(existsSync(tempRoot), false, "isolated profile root must be removed");
    assert.deepEqual(runtimeFiles(), initialRuntimeFiles,
      "Chromium must not write logs, dictionaries or other files into its immutable runtime");
  });

  return {
    executable,
    pageUrl: `http://127.0.0.1:${pagePort}/fixture?token=SECRET_QUERY_VALUE#SECRET_FRAGMENT`,
    port: workerPort,
    token,
    workerPid: child.pid,
    clickCount: () => clickCount,
  };
}

function assertTypedError(response, status, completion = "not_started") {
  assert.equal(response.status, status);
  assert.equal(response.body.ok, false);
  assert.equal(response.body.error.status, status);
  assert.match(response.body.error.code, /^[a-z0-9_]{1,64}$/);
  assert.equal(response.body.error.completion, completion);
  assert.equal(typeof response.body.error.message, "string");
}

test("production observe publishes fresh bounded opaque snapshots over authenticated loopback", async (t) => {
  const harness = await startHarness(t);
  const profile = "observe-contract";
  const owner = "observe-owner";

  const opened = await post(harness.port, "/open", { profile, owner }, harness.token);
  assert.equal(opened.status, 200);
  assert.equal(opened.body.ok, true);
  assert.match(opened.body.pageId, /^page-[0-9a-f-]{36}$/);
  assert.equal(opened.body.pageGeneration, 1);

  const navigated = await post(
    harness.port,
    "/goto",
    {
      profile,
      owner,
      pageId: opened.body.pageId,
      pageGeneration: opened.body.pageGeneration,
      actionId: "observe-goto-1",
      url: harness.pageUrl,
    },
    harness.token,
  );
  assert.equal(navigated.status, 200);
  assert.equal(navigated.body.ok, true);
  assert.ok(navigated.body.pageGeneration > opened.body.pageGeneration);

  const observeBody = {
    profile,
    owner,
    pageId: navigated.body.pageId,
    pageGeneration: navigated.body.pageGeneration,
  };
  const first = await post(harness.port, "/observe", observeBody, harness.token);
  assert.equal(first.status, 200);
  assert.equal(first.body.ok, true);
  assert.equal(first.body.pageId, navigated.body.pageId);
  assert.equal(first.body.pageGeneration, navigated.body.pageGeneration);
  assert.match(first.body.snapshotId, /^snapshot-[0-9a-f-]{36}$/);
  assert.ok(Array.isArray(first.body.nodes));
  assert.ok(first.body.nodes.length > 0 && first.body.nodes.length <= 64);
  assert.ok(first.body.nodes.every((node) => !Object.hasOwn(node, "selector")));
  assert.ok(first.body.nodes.some((node) => node.name === "Late button"));
  assert.ok(first.body.nodes.some((node) => node.name === "Focusable custom control"));
  const nodeNames = first.body.nodes.map((node) => node.name).join("\n");
  for (const nonActionable of [
    "SECRET_HIDDEN_BUTTON",
    "SECRET_INERT_LINK",
    "SECRET_ARIA_HIDDEN_INPUT",
  ]) {
    assert.equal(nodeNames.includes(nonActionable), false, nonActionable);
  }
  const firstRefs = first.body.nodes
    .map((node) => node.elementRef)
    .filter((elementRef) => typeof elementRef === "string");
  assert.ok(firstRefs.length > 0);
  assert.equal(new Set(firstRefs).size, firstRefs.length);
  assert.equal(first.body.url, `http://127.0.0.1:${new URL(harness.pageUrl).port}/fixture`);
  assert.ok(first.body.aria && first.body.aria.length > 0);
  assert.equal(first.body.textOnly, false);
  assert.ok(first.body.pngBase64);
  const livePng = Buffer.from(first.body.pngBase64, "base64");
  assert.deepEqual([...livePng.subarray(0, 8)], [...PNG_SIGNATURE]);
  const liveInspect = inspectPng(livePng);
  assert.equal(liveInspect.ok, true);
  assert.equal(liveInspect.empty, false);
  assert.equal(first.body.image.width, liveInspect.width);
  assert.equal(first.body.image.height, liveInspect.height);
  const liveModel = JSON.stringify(modelObservationView(first.body));
  assert.equal(liveModel.includes(first.body.pngBase64), false);
  assert.equal(liveModel.includes("pngBase64"), false);

  const serialized = JSON.stringify(first.body);
  for (const secret of [
    "SECRET_QUERY_VALUE",
    "SECRET_FRAGMENT",
    profile,
  ]) {
    assert.equal(serialized.includes(secret), false, secret);
  }

  const second = await post(harness.port, "/observe", observeBody, harness.token);
  assert.equal(second.status, 200);
  assert.notEqual(second.body.snapshotId, first.body.snapshotId);
  const secondRefs = new Set(second.body.nodes.map((node) => node.elementRef).filter(Boolean));
  assert.equal(firstRefs.some((elementRef) => secondRefs.has(elementRef)), false);

  for (const invalid of [{ preview: "true" }, { screenshot: 0 }]) {
    assertTypedError(await post(harness.port, "/observe", { ...observeBody, ...invalid }, harness.token), 400);
  }
  const preview = await post(harness.port, "/observe", { ...observeBody, preview: true, screenshot: false }, harness.token);
  assert.equal(preview.status, 200);
  assert.notEqual(preview.body.snapshotId, second.body.snapshotId);
  assert.equal(preview.body.pngBase64, undefined);
  assert.equal(preview.body.textOnly, true);
  const previewNode = preview.body.nodes.find(node => node.name === "Late button");
  const rejected = await post(harness.port, "/act", { ...observeBody,
    actionId: "preview-is-not-authority", kind: "click", snapshotId: preview.body.snapshotId,
    elementRef: previewNode.elementRef }, harness.token);
  assertTypedError(rejected, 409);
  assert.equal(harness.clickCount(), 0);
  const modelNode = second.body.nodes.find(node => node.name === "Late button");
  const clicked = await post(harness.port, "/act", { ...observeBody,
    actionId: "model-click-after-preview", kind: "click", snapshotId: second.body.snapshotId,
    elementRef: modelNode.elementRef }, harness.token);
  assert.equal(clicked.status, 200, "preview must not invalidate the model's real Playwright handle");
  const deadline = Date.now() + 3000;
  while (harness.clickCount() === 0 && Date.now() < deadline) await new Promise(resolve => setTimeout(resolve, 20));
  assert.equal(harness.clickCount(), 1, "independent fixture HTTP counter must witness one actual click");

  const reloaded = await post(
    harness.port,
    "/goto",
    {
      ...observeBody,
      actionId: "observe-goto-2",
      url: harness.pageUrl,
    },
    harness.token,
  );
  assert.equal(reloaded.status, 200);
  assert.ok(reloaded.body.pageGeneration > observeBody.pageGeneration);

  const stale = await post(harness.port, "/observe", observeBody, harness.token);
  assertTypedError(stale, 409);
  assert.equal(stale.body.error.currentPageGeneration, reloaded.body.pageGeneration);
});
