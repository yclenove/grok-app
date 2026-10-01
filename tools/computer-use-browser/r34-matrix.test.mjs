import test from "node:test";
import assert from "node:assert/strict";
import { createHash, randomBytes } from "node:crypto";
import { spawn } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  realpathSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { createServer, request } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { childEnv } from "./child-env.mjs";
import { childPids } from "./browser-pids.mjs";
import { confirmRunCancellation, confirmWorkerShutdown } from "./fixtures/cleanup-reconcile.mjs";
import { finishOwnedCleanup } from "./fixtures/cleanup-verified.mjs";

const ROOT = fileURLToPath(new URL(".", import.meta.url));
const FORM_HTML = readFileSync(join(ROOT, "fixtures", "form.html"), "utf8");
const WORKER_PATH = resolve(process.env.GROK_CU_TEST_WORKER || join(ROOT, "server.mjs"));
const WORKER_ROOT = dirname(WORKER_PATH);
const SERVER_SRC = readFileSync(WORKER_PATH, "utf8");
const CHROME_CANDIDATES = [
  process.env.GROK_CU_TEST_CHROME,
  "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
  "C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe",
].filter(Boolean);

function chromePath() {
  return CHROME_CANDIDATES.find((candidate) => existsSync(candidate)) || null;
}

function post(port, path, body, token, { host = false, timeoutMs = 20_000 } = {}) {
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
        res.on("data", (chunk) => chunks.push(chunk));
        res.on("end", () => {
          const raw = Buffer.concat(chunks).toString("utf8");
          try {
            resolvePromise({ status: res.statusCode, body: JSON.parse(raw) });
          } catch {
            reject(new Error(`invalid JSON ${raw.slice(0, 200)}`));
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

function nodeNamed(observation, name, role) {
  const needle = String(name).replace(/\s+/g, " ").trim().toLowerCase();
  const match = observation.nodes.find((node) => {
    if (typeof node.elementRef !== "string") return false;
    if (role && node.role !== role) return false;
    const nodeName = String(node.name || "").replace(/\s+/g, " ").trim().toLowerCase();
    return nodeName === needle || nodeName.startsWith(`${needle} `) || nodeName.includes(needle);
  });
  if (!match) {
    throw new Error(`missing node ${name} in ${JSON.stringify(observation.nodes)}`);
  }
  return match;
}

async function observe(port, token, ident) {
  for (let attempt = 0; attempt < 4; attempt += 1) {
    const res = await post(port, "/observe", ident, token);
    if (res.status === 200 && res.body.ok === true) return res.body;
    const gen = res.body?.error?.currentPageGeneration;
    if (res.status === 409 && Number.isInteger(gen) && gen > 0) {
      ident.pageGeneration = gen;
      continue;
    }
    throw new Error(`observe ${JSON.stringify(res)}`);
  }
  throw new Error("observe did not settle");
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

function waitForExit(child, timeoutMs) {
  if (child.exitCode != null || child.signalCode != null) return Promise.resolve(child.exitCode);
  return new Promise((resolvePromise, reject) => {
    const timer = setTimeout(() => {
      cleanup();
      reject(new Error(`owned worker did not exit within ${timeoutMs}ms`));
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
  assert.equal(target.startsWith(base), true);
  assert.equal(relative(realpathSync(tmpdir()), resolve(root)).startsWith(".."), false);
  rmSync(root, { recursive: true, force: true });
}

async function getOracle(pagePort, runId) {
  return new Promise((resolvePromise, reject) => {
    const req = request(
      { hostname: "127.0.0.1", port: pagePort, path: `/oracle?run=${encodeURIComponent(runId)}` },
      (res) => {
        const chunks = [];
        res.on("data", (chunk) => chunks.push(chunk));
        res.on("end", () => {
          try {
            resolvePromise(JSON.parse(Buffer.concat(chunks).toString("utf8")));
          } catch {
            reject(new Error("oracle json"));
          }
        });
      },
    );
    req.on("error", reject);
    req.end();
  });
}

function jsonGet(port, path) {
  return new Promise((resolvePromise, reject) => {
    const req = request({ hostname: "127.0.0.1", port, path }, (res) => {
      const chunks = [];
      res.on("data", (chunk) => chunks.push(chunk));
      res.on("end", () => {
        try {
          resolvePromise(JSON.parse(Buffer.concat(chunks).toString("utf8") || "null"));
        } catch (error) {
          reject(error);
        }
      });
    });
    req.on("error", reject);
    req.end();
  });
}

async function waitUntil(predicate, timeoutMs = 8000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await predicate()) return;
    await new Promise((resolvePromise) => setTimeout(resolvePromise, 20));
  }
  throw new Error("waitUntil timeout");
}

function releaseHang(pagePort) {
  return jsonGet(pagePort, "/hang-release");
}

async function hangStartedCount(pagePort) {
  const row = await jsonGet(pagePort, "/hang-started");
  return Number(row?.started || 0);
}

async function waitForHang(pagePort, before) {
  await waitUntil(async () => (await hangStartedCount(pagePort)) > before);
}

async function startHarness(t, { attachAfter = true } = {}) {
  const executable = chromePath();
  assert.ok(executable, "Chrome/Chromium is required for R3.4");
  const oracle = new Map();
  const hangHolders = [];
  let hangStartedCount = 0;
  const pageServer = createServer((req, res) => {
    const url = new URL(req.url || "/", "http://127.0.0.1");
    if (url.pathname === "/hang") {
      hangStartedCount += 1;
      hangHolders.push({ res, mode: url.searchParams.get("mode") || "ok" });
      return;
    }
    if (url.pathname === "/hang-started") {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify({ started: hangStartedCount }));
      return;
    }
    if (url.pathname === "/hang-release") {
      const pending = hangHolders.splice(0, hangHolders.length);
      for (const item of pending) {
        try {
          if (item.mode === "abort") item.res.destroy();
          else if (item.mode === "forbidden") {
            item.res.writeHead(302, { location: "http://127.0.0.1:1/" });
            item.res.end();
          } else {
            item.res.writeHead(200, { "content-type": "text/html; charset=utf-8" });
            item.res.end("<!doctype html><title>hang-done</title>");
          }
        } catch {
          /* ignore */
        }
      }
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify({ ok: true, released: pending.length }));
      return;
    }
    if (req.method === "POST" && url.pathname === "/oracle") {
      const chunks = [];
      req.on("data", (chunk) => chunks.push(chunk));
      req.on("end", () => {
        try {
          const body = JSON.parse(Buffer.concat(chunks).toString("utf8"));
          if (body.runId) oracle.set(String(body.runId), body);
        } catch {
          /* ignore */
        }
        res.writeHead(204);
        res.end();
      });
      return;
    }
    if (req.method === "GET" && url.pathname === "/oracle") {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify(oracle.get(url.searchParams.get("run") || "") || null));
      return;
    }
    if (url.pathname === "/frame.html") {
      res.writeHead(200, { "content-type": "text/html; charset=utf-8" });
      res.end(readFileSync(join(ROOT, "fixtures", "frame.html")));
      return;
    }
    if (url.pathname === "/popup.html") {
      res.writeHead(200, { "content-type": "text/html; charset=utf-8" });
      res.end(readFileSync(join(ROOT, "fixtures", "popup.html")));
      return;
    }
    if (url.pathname === "/file.bin") {
      res.writeHead(200, { "content-type": "application/octet-stream" });
      res.end(Buffer.from("staged-by-run"));
      return;
    }
    if (url.pathname === "/redirect-dl") {
      res.writeHead(302, { location: "/file.bin" });
      res.end();
      return;
    }
    if (url.pathname === "/evil-dl") {
      res.writeHead(302, { location: "javascript:alert(1)" });
      res.end();
      return;
    }
    if (url.pathname === "/huge.bin") {
      res.writeHead(200, { "content-type": "application/octet-stream" });
      res.end(Buffer.alloc(9 * 1024 * 1024, 1));
      return;
    }
    res.writeHead(200, { "content-type": "text/html; charset=utf-8" });
    res.end(FORM_HTML);
  });
  const pagePort = await listen(pageServer);
  const token = randomBytes(32).toString("hex");
  const tempRoot = mkdtempSync(join(realpathSync(tmpdir()), "grok-cu-r34-"));
  let workerOutput = "";
  const child = spawn(process.execPath, [WORKER_PATH], {
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
  child.stderr.on("data", (chunk) => {
    workerOutput = `${workerOutput}${chunk.toString("utf8")}`.slice(-16_384);
  });
  const workerPort = await new Promise((resolvePromise, reject) => {
    const timer = setTimeout(() => reject(new Error(`port timeout ${workerOutput}`)), 20_000);
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
          /* wait */
        }
      }
    };
    child.stdout.on("data", onData);
    child.once("error", reject);
    child.once("exit", (code) => {
      clearTimeout(timer);
      reject(new Error(`worker exited ${code}: ${workerOutput}`));
    });
  });
  const ownedPids = new Set([child.pid]);
  let cleanupPromise;
  async function finishHarnessCleanup() {
    const pids = await post(workerPort, "/pids", {}, token, { host: true }).catch(() => null);
    if (Array.isArray(pids?.body?.browserPids)) {
      for (const pid of pids.body.browserPids) ownedPids.add(Number(pid));
    }
    await finishOwnedCleanup({
      shutdown: async () => {
        const shutdown = await confirmWorkerShutdown((path, timeoutMs) =>
          post(workerPort, path, {}, token, { host: true, timeoutMs }), {
          budgetMs: 20_000,
          onPending: () => t.diagnostic("shutdown pending; observing cleanup within the original 20s transport budget"),
        });
        assert.equal(shutdown.status, 200, `shutdown unconfirmed; retain ${tempRoot}`);
        assert.equal(shutdown.body.shutdown, true);
      },
      workerExit: async () => {
        try {
          assert.equal(await waitForExit(child, 10_000), 0, "owned worker must exit cleanly");
        } catch (error) {
          // This exact fixture child is ours. Forced exit remains a failure,
          // never evidence of graceful shutdown or permission to remove files.
          if (child.exitCode == null && child.signalCode == null) child.kill();
          await waitForExit(child, 5_000).catch(() => null);
          throw error;
        }
      },
      fixtureClose: () => new Promise((resolvePromise) => pageServer.close(() => resolvePromise())),
      browserExit: async () => {
        assert.equal(pids?.body?.workerPid, child.pid, "owned worker inventory was not confirmed");
        assert.equal(Array.isArray(pids?.body?.browserPids), true, "owned browser inventory was not confirmed");
        const live = await waitForPidsToExit([...ownedPids]);
        assert.deepEqual(live, [], `worker/browser PIDs remained alive: ${live.join(",")}`);
      },
      removeProfile: () => removeOwnedTempRoot(tempRoot),
    });
    assert.equal(existsSync(tempRoot), false, "isolated profile root must be removed");
    return [];
  }
  const harness = {
    pagePort,
    port: workerPort,
    token,
    tempRoot,
    child,
    ownedPids,
    executable,
    shutdown() {
      // Repeated observers receive the original completion or failure. A
      // previously failed cleanup cannot silently become a successful [].
      cleanupPromise ??= finishHarnessCleanup();
      return cleanupPromise;
    },
  };
  if (attachAfter) t.after(() => harness.shutdown());
  return harness;
}

async function openLanded(harness, profile, owner) {
  const opened = await post(harness.port, "/open", { profile, owner }, harness.token);
  assert.equal(opened.status, 200, JSON.stringify(opened.body));
  const ident = {
    profile,
    owner,
    pageId: opened.body.pageId,
    pageGeneration: opened.body.pageGeneration,
  };
  const landed = await post(
    harness.port,
    "/goto",
    {
      ...ident,
      url: `http://127.0.0.1:${harness.pagePort}/?run=${encodeURIComponent(owner)}`,
      actionId: `goto-${owner}`,
    },
    harness.token,
  );
  assert.equal(landed.status, 200, JSON.stringify(landed.body));
  ident.pageId = landed.body.pageId;
  ident.pageGeneration = landed.body.pageGeneration;
  return ident;
}

test("R3.4 named fixture matrix", async (t) => {
  t.diagnostic(`worker=${WORKER_PATH}; sha256=${createHash("sha256").update(SERVER_SRC).digest("hex")}; node=${process.version}`);
  const harness = await startHarness(t);

  await t.test("1 dual tab directional isolation", async () => {
    const a = await openLanded(harness, "tab-a", "run-a");
    const b = await openLanded(harness, "tab-b", "run-b");
    const obsA = await observe(harness.port, harness.token, a);
    const plus = nodeNamed(obsA, "+1", "button");
    const clicked = await post(
      harness.port,
      "/act",
      {
        ...a,
        snapshotId: obsA.snapshotId,
        actionId: "click-a",
        kind: "click",
        elementRef: plus.elementRef,
      },
      harness.token,
    );
    assert.equal(clicked.status, 200);
    await new Promise((resolvePromise) => setTimeout(resolvePromise, 60));
    assert.equal((await getOracle(harness.pagePort, "run-a"))?.count, 1);
    assert.equal(await getOracle(harness.pagePort, "run-b"), null);
    await post(harness.port, "/close", { profile: "tab-a", owner: "run-a" }, harness.token);
    await post(harness.port, "/close", { profile: "tab-b", owner: "run-b" }, harness.token);
  });

  await t.test("2 popup has independent identity", async () => {
    const ident = await openLanded(harness, "pop-p", "pop-o");
    const obs = await observe(harness.port, harness.token, ident);
    const pop = nodeNamed(obs, "popup", "button");
    const clicked = await post(
      harness.port,
      "/act",
      {
        ...ident,
        snapshotId: obs.snapshotId,
        actionId: "open-pop",
        kind: "click",
        elementRef: pop.elementRef,
      },
      harness.token,
    );
    assert.equal(clicked.status, 200, JSON.stringify(clicked.body));
    let pages = [];
    for (let attempt = 0; attempt < 20; attempt += 1) {
      const tabs = await post(
        harness.port,
        "/tabs",
        { profile: ident.profile, owner: ident.owner },
        harness.token,
      );
      assert.equal(tabs.status, 200);
      pages = tabs.body.pages || [];
      if (new Set(pages.map((page) => page.pageId)).size >= 2) break;
      await new Promise((resolvePromise) => setTimeout(resolvePromise, 100));
    }
    assert.ok(new Set(pages.map((page) => page.pageId)).size >= 2, JSON.stringify(pages));
    await post(harness.port, "/close", { profile: ident.profile, owner: ident.owner }, harness.token);
  });

  await t.test("3 same-origin iframe is listed; cross-origin stays omitted", async () => {
    const ident = await openLanded(harness, "frame-p", "frame-o");
    await observe(harness.port, harness.token, ident);
    let frames;
    for (let attempt = 0; attempt < 20; attempt += 1) {
      frames = await post(harness.port, "/frames", ident, harness.token);
      assert.equal(frames.status, 200, JSON.stringify(frames.body));
      if ((frames.body.frames || []).some((url) => String(url).includes("frame"))) break;
      await new Promise((resolvePromise) => setTimeout(resolvePromise, 100));
    }
    assert.ok(
      (frames.body.frames || []).some((url) => String(url).includes("frame")),
      JSON.stringify(frames.body),
    );
    await post(harness.port, "/close", { profile: ident.profile, owner: ident.owner }, harness.token);
  });

  await t.test("4 navigation and same-URL reload invalidate old identity", async () => {
    const ident = await openLanded(harness, "nav-p", "nav-o");
    const first = await observe(harness.port, harness.token, ident);
    const reloaded = await post(
      harness.port,
      "/act",
      { ...ident, actionId: "reload-same", kind: "reload" },
      harness.token,
    );
    assert.equal(reloaded.status, 200);
    assert.ok(reloaded.body.pageGeneration > first.pageGeneration);
    const stale = await post(
      harness.port,
      "/act",
      {
        ...ident,
        snapshotId: first.snapshotId,
        actionId: "stale-after-reload",
        kind: "click",
        elementRef: nodeNamed(first, "+1", "button").elementRef,
      },
      harness.token,
    );
    assert.ok(stale.status === 409 || stale.status === 400);
    await post(harness.port, "/close", { profile: ident.profile, owner: ident.owner }, harness.token);
  });

  await t.test("5 close does not fall back to another tab", async () => {
    const ident = await openLanded(harness, "close-p", "close-o");
    const extra = await post(
      harness.port,
      "/new-tab",
      { profile: ident.profile, owner: ident.owner, actionId: "new-tab-1" },
      harness.token,
    );
    assert.equal(extra.status, 200);
    assert.notEqual(extra.body.pageId, ident.pageId);
    await extra.body.pageId;
    const closed = ident.pageId;
    await post(harness.port, "/close", { profile: ident.profile, owner: ident.owner }, harness.token);
    const actDead = await post(
      harness.port,
      "/act",
      {
        profile: ident.profile,
        owner: ident.owner,
        pageId: closed,
        pageGeneration: ident.pageGeneration,
        snapshotId: "snapshot-x",
        actionId: "after-close",
        kind: "click",
        elementRef: "element-x",
      },
      harness.token,
    );
    assert.ok(actDead.status === 404 || actDead.status === 409);
  });

  await t.test("6 same actionId same fingerprint executes once and replays", async () => {
    const ident = await openLanded(harness, "id-p", "id-o");
    const obs = await observe(harness.port, harness.token, ident);
    const plus = nodeNamed(obs, "+1", "button");
    const body = {
      ...ident,
      snapshotId: obs.snapshotId,
      actionId: "once-click",
      kind: "click",
      elementRef: plus.elementRef,
    };
    const first = await post(harness.port, "/act", body, harness.token);
    assert.equal(first.status, 200);
    const second = await post(harness.port, "/act", body, harness.token);
    assert.equal(second.status, 200);
    assert.equal(second.body.replayed, true);
    await new Promise((resolvePromise) => setTimeout(resolvePromise, 60));
    assert.equal((await getOracle(harness.pagePort, "id-o"))?.count, 1);
    await post(harness.port, "/close", { profile: ident.profile, owner: ident.owner }, harness.token);
  });

  await t.test("7 same actionId different fingerprint is rejected", async () => {
    const ident = await openLanded(harness, "cf-p", "cf-o");
    const obs = await observe(harness.port, harness.token, ident);
    const plus = nodeNamed(obs, "+1", "button");
    const submit = nodeNamed(obs, "Submit", "button");
    const first = await post(
      harness.port,
      "/act",
      {
        ...ident,
        snapshotId: obs.snapshotId,
        actionId: "conflict-id",
        kind: "click",
        elementRef: plus.elementRef,
      },
      harness.token,
    );
    assert.equal(first.status, 200);
    const conflict = await post(
      harness.port,
      "/act",
      {
        ...ident,
        snapshotId: obs.snapshotId,
        actionId: "conflict-id",
        kind: "click",
        elementRef: submit.elementRef,
      },
      harness.token,
    );
    assert.equal(conflict.status, 409);
    await post(harness.port, "/close", { profile: ident.profile, owner: ident.owner }, harness.token);
  });

  await t.test("8 different actionIds execute twice", async () => {
    const ident = await openLanded(harness, "two-p", "two-o");
    const firstObs = await observe(harness.port, harness.token, ident);
    const plus = nodeNamed(firstObs, "+1", "button");
    const one = await post(
      harness.port,
      "/act",
      {
        ...ident,
        snapshotId: firstObs.snapshotId,
        actionId: "click-1",
        kind: "click",
        elementRef: plus.elementRef,
      },
      harness.token,
    );
    assert.equal(one.status, 200);
    ident.pageGeneration = one.body.pageGeneration || ident.pageGeneration;
    const secondObs = await observe(harness.port, harness.token, ident);
    const plus2 = nodeNamed(secondObs, "+1", "button");
    const two = await post(
      harness.port,
      "/act",
      {
        ...ident,
        snapshotId: secondObs.snapshotId,
        actionId: "click-2",
        kind: "click",
        elementRef: plus2.elementRef,
      },
      harness.token,
    );
    assert.equal(two.status, 200);
    await new Promise((resolvePromise) => setTimeout(resolvePromise, 60));
    assert.equal((await getOracle(harness.pagePort, "two-o"))?.count, 2);
    await post(harness.port, "/close", { profile: ident.profile, owner: ident.owner }, harness.token);
  });

  await t.test("9 pending does not execute a second time", async () => {
    const ident = await openLanded(harness, "pend-p", "pend-o");
    const hangUrl = `http://127.0.0.1:${harness.pagePort}/hang`;
    const before = await hangStartedCount(harness.pagePort);
    const body = { ...ident, url: hangUrl, actionId: "pend-hang" };
    const first = post(harness.port, "/goto", body, harness.token);
    await waitForHang(harness.pagePort, before);
    const second = await post(harness.port, "/goto", body, harness.token);
    assert.equal(second.status, 409);
    assert.match(String(second.body.error?.code || ""), /in_flight|pending/);
    assert.equal(await hangStartedCount(harness.pagePort), before + 1);
    await releaseHang(harness.pagePort);
    const firstRes = await first;
    assert.equal(firstRes.status, 200, JSON.stringify(firstRes.body));
    await post(harness.port, "/close", { profile: ident.profile, owner: ident.owner }, harness.token);
  });

  await t.test("10 unknown is not replayed and blocks writes until observe", async () => {
    const ident = await openLanded(harness, "unk-p", "unk-o");
    const obs = await observe(harness.port, harness.token, ident);
    const plus = nodeNamed(obs, "+1", "button");
    const hangUrl = `http://127.0.0.1:${harness.pagePort}/hang?mode=forbidden`;
    const before = await hangStartedCount(harness.pagePort);
    const hangBody = { ...ident, url: hangUrl, actionId: "unk-hang" };
    const first = post(harness.port, "/goto", hangBody, harness.token);
    await waitForHang(harness.pagePort, before);
    const busy = await post(
      harness.port,
      "/act",
      {
        ...ident,
        snapshotId: obs.snapshotId,
        actionId: "unk-busy",
        kind: "click",
        elementRef: plus.elementRef,
      },
      harness.token,
    );
    assert.equal(busy.status, 409);
    await releaseHang(harness.pagePort);
    const firstRes = await first.then(
      (row) => row,
      (error) => ({ status: 599, body: { error: String(error.message || error) } }),
    );
    assert.ok(firstRes.status >= 400, JSON.stringify(firstRes.body));
    const replay = await post(harness.port, "/goto", hangBody, harness.token);
    assert.equal(replay.status, 409);
    const blocked = await post(
      harness.port,
      "/act",
      {
        ...ident,
        snapshotId: obs.snapshotId,
        actionId: "unk-next",
        kind: "click",
        elementRef: plus.elementRef,
      },
      harness.token,
    );
    assert.equal(blocked.status, 409);
    await observe(harness.port, harness.token, ident);
    const reland = await post(
      harness.port,
      "/goto",
      {
        ...ident,
        url: `http://127.0.0.1:${harness.pagePort}/?run=unk-o`,
        actionId: "unk-reland",
      },
      harness.token,
    );
    assert.equal(reland.status, 200, JSON.stringify(reland.body));
    ident.pageId = reland.body.pageId || ident.pageId;
    ident.pageGeneration = reland.body.pageGeneration || ident.pageGeneration;
    const unlocked = await observe(harness.port, harness.token, ident);
    const plus2 = nodeNamed(unlocked, "+1", "button");
    const clicked = await post(
      harness.port,
      "/act",
      {
        ...ident,
        pageId: unlocked.pageId,
        pageGeneration: unlocked.pageGeneration,
        snapshotId: unlocked.snapshotId,
        actionId: "unk-after",
        kind: "click",
        elementRef: plus2.elementRef,
      },
      harness.token,
    );
    assert.equal(clicked.status, 200, JSON.stringify(clicked.body));
    await waitUntil(async () => (await getOracle(harness.pagePort, "unk-o"))?.count === 1);
    await post(harness.port, "/close", { profile: ident.profile, owner: ident.owner }, harness.token);
  });

  await t.test("11 stale snapshot/ref has zero side effects", async () => {
    const ident = await openLanded(harness, "stale-p", "stale-o");
    const first = await observe(harness.port, harness.token, ident);
    const second = await observe(harness.port, harness.token, ident);
    const plus = nodeNamed(first, "+1", "button");
    const stale = await post(
      harness.port,
      "/act",
      {
        ...ident,
        snapshotId: first.snapshotId,
        actionId: "stale-click",
        kind: "click",
        elementRef: plus.elementRef,
      },
      harness.token,
    );
    assert.equal(stale.status, 409);
    assert.equal(await getOracle(harness.pagePort, "stale-o"), null);
    assert.notEqual(second.snapshotId, first.snapshotId);
    await post(harness.port, "/close", { profile: ident.profile, owner: ident.owner }, harness.token);
  });

  await t.test("12 slow action cancel has no postcondition", async () => {
    const ident = await openLanded(harness, "can-p", "can-o");
    const hangUrl = `http://127.0.0.1:${harness.pagePort}/hang`;
    const before = await hangStartedCount(harness.pagePort);
    const hanging = post(
      harness.port,
      "/goto",
      { ...ident, url: hangUrl, actionId: "cancel-hang" },
      harness.token,
    );
    await waitForHang(harness.pagePort, before);
    const cancelled = await confirmRunCancellation((path, body, timeoutMs) =>
      post(harness.port, path, body, harness.token, { host: true, timeoutMs }),
    { owner: ident.owner, runRevision: 1 }, {
      budgetMs: 20_000,
      onPending: () => t.diagnostic("cancel pending; require original run physical idle within the original 20s transport budget"),
    });
    assert.equal(cancelled.status, 200, JSON.stringify(cancelled.body));
    assert.equal(cancelled.body.owner, ident.owner);
    const stopped = await post(harness.port, "/run-status", { owner: ident.owner, runRevision: 1 }, harness.token, { host: true });
    assert.equal(stopped.status, 200);
    assert.equal(stopped.body.phase, "stopped");
    assert.equal(stopped.body.idle, true);
    assert.equal(stopped.body.activeOperations, 0);
    const hangRes = await hanging.then(
      (row) => row,
      (error) => ({ status: 599, body: { error: String(error.message || error) } }),
    );
    assert.ok(hangRes.status >= 400, JSON.stringify(hangRes.body));
    await releaseHang(harness.pagePort);
    const oracle = await getOracle(harness.pagePort, "can-o");
    assert.equal(oracle?.pings ?? 0, 0);
    assert.equal(oracle?.count ?? 0, 0);
  });

  await t.test("13 run A staging cannot be uploaded by run B", async () => {
    const a = await openLanded(harness, "up-a", "up-run-a");
    const b = await openLanded(harness, "up-b", "up-run-b");
    const stagingA = join(harness.tempRoot, "staging", "computer-use-staging", "up-run-a");
    mkdirSync(stagingA, { recursive: true });
    writeFileSync(join(stagingA, "note.txt"), "secret-a");
    const stolen = await post(
      harness.port,
      "/upload",
      {
        ...b,
        actionId: "steal",
        selector: "#file",
        path: join(stagingA, "note.txt"),
      },
      harness.token,
    );
    assert.equal(stolen.status, 400);
    await post(harness.port, "/close", { profile: a.profile, owner: a.owner }, harness.token);
    await post(harness.port, "/close", { profile: b.profile, owner: b.owner }, harness.token);
  });

  await t.test("14 download redirect, reserved name, over-limit leave no leftover part", async () => {
    const ident = await openLanded(harness, "dl-p", "dl-o");
    const obs = await observe(harness.port, harness.token, ident);
    const link = nodeNamed(obs, "download", "link");
    const reserved = await post(
      harness.port,
      "/download",
      {
        ...ident,
        snapshotId: obs.snapshotId,
        actionId: "dl-nul",
        filename: "nul",
        elementRef: link.elementRef,
      },
      harness.token,
    );
    assert.equal(reserved.status, 400);
    const saved = await post(
      harness.port,
      "/download",
      {
        ...ident,
        snapshotId: obs.snapshotId,
        actionId: "dl-ok",
        filename: "report.bin",
        elementRef: link.elementRef,
      },
      harness.token,
    );
    assert.equal(saved.status, 200, JSON.stringify(saved.body));
    assert.equal(readFileSync(saved.body.path, "utf8"), "staged-by-run");
    const stagingRoot = join(harness.tempRoot, "staging");
    const parts = [];
    const walk = (dir) => {
      if (!existsSync(dir)) return;
      for (const name of readdirSync(dir, { withFileTypes: true })) {
        const full = join(dir, name.name);
        if (name.isDirectory()) walk(full);
        else if (name.name.endsWith(".part")) parts.push(full);
      }
    };
    walk(stagingRoot);
    assert.deepEqual(parts, []);
    await post(harness.port, "/close", { profile: ident.profile, owner: ident.owner }, harness.token);
  });

  await t.test("15 owner/profile traversal is rejected over HTTP", async () => {
    const bad = await post(
      harness.port,
      "/open",
      { profile: "../evil", owner: "run" },
      harness.token,
    );
    assert.equal(bad.status, 400);
    const owner = await post(
      harness.port,
      "/open",
      { profile: "okp", owner: "..\\evil" },
      harness.token,
    );
    assert.equal(owner.status, 400);
  });

  await t.test("16 unauthorized token and non-Host clear are rejected", async () => {
    const denied = await post(harness.port, "/health", {}, "deadbeef");
    assert.equal(denied.status, 401);
    const ident = await openLanded(harness, "auth-p", "auth-o");
    const clear = await post(
      harness.port,
      "/clear",
      { profile: ident.profile },
      harness.token,
    );
    assert.equal(clear.status, 403);
    await post(harness.port, "/close", { profile: ident.profile, owner: ident.owner }, harness.token);
  });

  await t.test("17 screenshot, ARIA, and node caps hold", async () => {
    const ident = await openLanded(harness, "cap-p", "cap-o");
    const obs = await observe(harness.port, harness.token, ident);
    assert.ok(obs.nodes.length <= 64);
    assert.ok(obs.aria && obs.aria.length > 0);
    assert.equal(obs.textOnly, false);
    assert.match(obs.pngBase64 || "", /^iVBOR/);
    await post(harness.port, "/close", { profile: ident.profile, owner: ident.owner }, harness.token);
  });

  await t.test("18 shutdown leaves no worker/Chromium PIDs from this harness", async () => {
    const isolated = await startHarness(t, { attachAfter: false });
    try {
      await openLanded(isolated, "pid-p", "pid-o");
      const pids = await post(isolated.port, "/pids", {}, isolated.token, { host: true });
      assert.equal(pids.status, 200);
      assert.equal(pids.body.workerPid, isolated.child.pid);
      for (const pid of [...(pids.body.browserPids || []), ...childPids(isolated.child.pid)]) {
        isolated.ownedPids.add(Number(pid));
      }
      assert.ok(
        isolated.ownedPids.size >= 2,
        `expected worker+browser pids, got ${[...isolated.ownedPids]} pids=${JSON.stringify(pids.body)}`,
      );
      const live = await isolated.shutdown();
      assert.deepEqual(live, []);
    } catch (error) {
      await isolated.shutdown().catch(() => {});
      throw error;
    }
  });

  await t.test("19 open-observe-act-png-shutdown on the production worker", async () => {
    const ident = await openLanded(harness, "mc-p", "mc-o");
    const obs = await observe(harness.port, harness.token, ident);
    assert.match(obs.pngBase64 || "", /^iVBOR/);
    const plus = nodeNamed(obs, "+1", "button");
    const acted = await post(
      harness.port,
      "/act",
      {
        ...ident,
        snapshotId: obs.snapshotId,
        actionId: "mc-click",
        kind: "click",
        elementRef: plus.elementRef,
      },
      harness.token,
    );
    assert.equal(acted.status, 200, JSON.stringify(acted.body));
    ident.pageGeneration = acted.body.pageGeneration || ident.pageGeneration;
    const verified = await observe(harness.port, harness.token, ident);
    assert.notEqual(verified.snapshotId, obs.snapshotId);
    assert.equal(verified.pageId, obs.pageId);
    await waitUntil(async () => (await getOracle(harness.pagePort, "mc-o"))?.count === 1);
    await post(harness.port, "/close", { profile: ident.profile, owner: ident.owner }, harness.token);
  });

  await t.test("20 sentinel secrets stay out of production routes and selector backdoors", async () => {
    const typed = readFileSync(join(WORKER_ROOT, "typed-act.mjs"), "utf8");
    const workerRs = readFileSync(
      join(ROOT, "../../src-tauri/src/computer_use/playwright_worker.rs"),
      "utf8",
    );
    const hostRs = readFileSync(
      join(ROOT, "../../src-tauri/computer-use-core/src/browser.rs"),
      "utf8",
    );
    for (const [name, src] of [
      ["server", SERVER_SRC],
      ["typed", typed],
      ["worker", workerRs],
      ["host", hostRs],
    ]) {
      for (const needle of [
        'pathname === "/marker"',
        'pathname === "/state"',
        'pathname === "/crash"',
        'pathname === "/popup"',
        'path == "/popup"',
        "slow_click",
        "managed_action_id",
        '|| "#pop"',
        "a#dl",
        "arrayBuffer()",
      ]) {
        assert.equal(src.includes(needle), false, `${name} ${needle}`);
      }
    }
    const previousOpen = process.env.OPENAI_API_KEY;
    const previousGrok = process.env.GROK_API_KEY;
    process.env.OPENAI_API_KEY = "SHOULD_NOT_REACH_CHILD";
    process.env.GROK_API_KEY = "SHOULD_NOT_REACH_CHILD";
    try {
      const env = childEnv({ GROK_CU_BROWSER_TOKEN: harness.token });
      const child = spawn(
        process.execPath,
        [
          "-e",
          "process.stdout.write(JSON.stringify({o:process.env.OPENAI_API_KEY||null,g:process.env.GROK_API_KEY||null}))",
        ],
        { env, stdio: ["ignore", "pipe", "ignore"] },
      );
      const raw = await new Promise((resolvePromise, reject) => {
        const chunks = [];
        child.stdout.on("data", (chunk) => chunks.push(chunk));
        child.on("error", reject);
        child.on("exit", (code) => {
          if (code !== 0) reject(new Error(`env probe ${code}`));
          else resolvePromise(Buffer.concat(chunks).toString("utf8"));
        });
      });
      const seen = JSON.parse(raw);
      assert.equal(seen.o, null);
      assert.equal(seen.g, null);
    } finally {
      if (previousOpen === undefined) delete process.env.OPENAI_API_KEY;
      else process.env.OPENAI_API_KEY = previousOpen;
      if (previousGrok === undefined) delete process.env.GROK_API_KEY;
      else process.env.GROK_API_KEY = previousGrok;
    }
  });
});
