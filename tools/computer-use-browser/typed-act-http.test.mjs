import test from "node:test";
import assert from "node:assert/strict";
import { randomBytes } from "node:crypto";
import { spawn } from "node:child_process";
import {
  existsSync,
  mkdtempSync,
  mkdirSync,
  readFileSync,
  realpathSync,
  writeFileSync,
} from "node:fs";
import { rm } from "node:fs/promises";
import { createServer, request } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { childEnv } from "./child-env.mjs";
import { childPids } from "./browser-pids.mjs";
import { confirmWorkerShutdown } from "./fixtures/cleanup-reconcile.mjs";

const ROOT = fileURLToPath(new URL(".", import.meta.url));
const FORM_HTML = readFileSync(join(ROOT, "fixtures", "form.html"), "utf8");
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
        let total = 0;
        res.on("data", (chunk) => {
          total += chunk.length;
          if (total <= 256 * 1024) chunks.push(chunk);
        });
        res.on("end", () => {
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

async function removeOwnedTempRoot(root) {
  const base = `${realpathSync(tmpdir())}${sep}`.toLowerCase();
  const target = `${resolve(root)}${sep}`.toLowerCase();
  assert.equal(target.startsWith(base), true, `refusing to remove non-temp root: ${root}`);
  assert.equal(relative(realpathSync(tmpdir()), resolve(root)).startsWith(".."), false);
  // Only after owned child exit has been verified. In pinned Node 20.18,
  // rmSync's first rmdir EBUSY bypasses maxRetries; the asynchronous path
  // applies the same bounded retry policy to that initial directory error.
  await rm(root, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
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
    throw new Error(
      `missing node name=${name} role=${role || "*"} in ${JSON.stringify(observation.nodes)}`,
    );
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

async function startHarness(t) {
  const executable = chromePath();
  assert.ok(executable, "a supported Chrome/Chromium executable is required for this contract");

  const started = performance.now();
  const fixtureTraffic = [];
  let stalledNavigations = 0;
  const oracle = new Map();
  const waitControls = new Map();
  const pageServer = createServer((req, res) => {
    const url = new URL(req.url || "/", "http://127.0.0.1");
    if (process.env.GROK_CU_TEST_BROWSER_DEBUG === "1") {
      const route = ["/", "/drag-fixture", "/wait-fixture", "/wait-control", "/navigated", "/oracle", "/frame.html", "/never-finish-navigation"]
        .includes(url.pathname) ? url.pathname : "other";
      const record = phase => {
        fixtureTraffic.push({ route, phase, elapsedMs: Math.round(performance.now() - started) });
        if (fixtureTraffic.length > 100) fixtureTraffic.shift();
      };
      record("request");
      res.once("finish", () => record("finish"));
      res.once("close", () => record("close"));
    }
    if (req.method === "GET" && url.pathname === "/never-finish-navigation") {
      stalledNavigations += 1;
      res.writeHead(200, { "content-type": "text/html; charset=utf-8" });
      if (url.searchParams.get("initial") === "ready" && stalledNavigations === 1) {
        res.end("<!doctype html><title>Ready for reload</title><button>Ready</button>");
        return;
      }
      // Deliberately leave the parser waiting on an incomplete script. The
      // real navigation, not a mock timer, must exhaust its product deadline.
      res.write("<!doctype html><title>Pending navigation</title><script>");
      return;
    }
    if (req.method === "GET" && url.pathname === "/wait-control") {
      const run = url.searchParams.get("run");
      waitControls.set(run, res);
      const timer = setTimeout(() => res.end("noop"), 10_000);
      res.once("close", () => { clearTimeout(timer); if (waitControls.get(run) === res) waitControls.delete(run); });
      return;
    }
    if (req.method === "GET" && url.pathname === "/wait-fixture") {
      res.writeHead(200, { "content-type": "text/html; charset=utf-8" });
      res.end(`<!doctype html><button id="watched">Waiting</button><button>Ready</button>
        <script>
          const runId = new URLSearchParams(location.search).get('run');
          let clicks = 0;
          const watched = document.getElementById('watched');
          document.addEventListener('click', () => { clicks += 1; });
          fetch('/wait-control?run=' + encodeURIComponent(runId)).then(r => r.text()).then(op => {
            if (op === 'name') watched.textContent = 'Ready';
            if (op === 'replace') {
              const replacement = watched.cloneNode(true); replacement.textContent = 'Ready';
              watched.replaceWith(replacement);
            }
            if (op === 'hide') { watched.hidden = true; watched.textContent = 'Ready'; }
            return fetch('/oracle', {method:'POST',headers:{'content-type':'application/json'},
              body:JSON.stringify({runId,count:clicks,operation:op})
            }).then(() => { if (op === 'navigate') location.replace('/navigated'); });
          });
        </script>`);
      return;
    }
    if (req.method === "GET" && url.pathname === "/navigated") {
      res.writeHead(200, { "content-type": "text/html; charset=utf-8" });
      res.end('<!doctype html><button id="watched">Ready</button>');
      return;
    }
    if (req.method === "GET" && url.pathname === "/drag-fixture") {
      res.writeHead(200, { "content-type": "text/html; charset=utf-8" });
      res.end(`<!doctype html><div tabindex="0" aria-label="Source" id="source"
        style="position:absolute;left:20px;top:40px;width:80px;height:40px;background:#aaa">Source</div>
        <div tabindex="0" aria-label="Destination" id="destination"
        style="position:absolute;left:180px;top:160px;width:80px;height:60px;background:#bbb">Destination</div>
        <script>
          const runId = new URLSearchParams(location.search).get('run');
          let held = false, drops = 0;
          document.getElementById('source').onmousedown = () => { held = true; };
          document.getElementById('destination').onmouseup = () => {
            if (held) drops += 1;
            held = false;
            fetch('/oracle', {method:'POST',headers:{'content-type':'application/json'},
              body:JSON.stringify({runId,drops})});
          };
        </script>`);
      return;
    }
    if (url.pathname === "/frame.html") {
      res.writeHead(200, { "content-type": "text/html; charset=utf-8" });
      res.end('<!doctype html><button>frame fixture</button>');
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
      const row = oracle.get(url.searchParams.get("run") || "") || null;
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify(row));
      return;
    }
    res.writeHead(200, { "content-type": "text/html; charset=utf-8" });
    res.end(FORM_HTML);
  });
  const pagePort = await listen(pageServer);
  const token = randomBytes(32).toString("hex");
  const tempRoot = mkdtempSync(join(realpathSync(tmpdir()), "grok-cu-typed-act-"));
  let workerOutput = "";
  const workerScript = process.env.GROK_CU_TEST_WORKER || join(ROOT, "server.mjs");
  assert.ok(existsSync(workerScript), "the selected test worker must exist");
  const child = spawn(process.execPath, [workerScript], {
    cwd: dirname(workerScript),
    windowsHide: true,
    stdio: ["ignore", "pipe", "pipe"],
    env: { ...childEnv({
      GROK_CU_BROWSER_TOKEN: token,
      GROK_CU_BROWSER_PORT: "0",
      GROK_CU_BROWSER_PROFILE_ROOT: join(tempRoot, "profiles"),
      GROK_CU_BROWSER_STAGING_ROOT: join(tempRoot, "staging"),
      GROK_CU_CHROME: executable,
    }), ...(process.env.GROK_CU_TEST_BROWSER_DEBUG === "1" ? { DEBUG: "pw:browser,pw:api" } : {}) },
  });
  child.stderr.on("data", (chunk) => {
    workerOutput = `${workerOutput}${chunk.toString("utf8")}`.slice(-16_384);
  });
  const workerPort = await new Promise((resolvePromise, reject) => {
    const timer = setTimeout(
      () => reject(new Error(`worker port timeout: ${workerOutput}`)),
      20_000,
    );
    let stdout = "";
    const onData = (chunk) => {
      stdout += chunk.toString("utf8");
      workerOutput = `${workerOutput}${chunk.toString("utf8")}`.slice(-16_384);
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
    const shutdown = await confirmWorkerShutdown((path, timeoutMs) =>
      post(workerPort, path, {}, token, { host: true, timeoutMs }), {
      onPending: () => t.diagnostic("shutdown returned pending; observing physical cleanup within the original 20s budget"),
    })
      .catch(error => ({ transportError: error.message }));
    let forcedWorkerExit = false;
    try {
      await waitForExit(child, 10_000);
    } catch {
      forcedWorkerExit = true;
      child.kill();
      await waitForExit(child, 5_000).catch(() => null);
    }
    await closeServer(pageServer);
    const live = await waitForPidsToExit([...ownedPids]);
    if (process.env.GROK_CU_TEST_BROWSER_DEBUG === "1") {
      t.diagnostic(`isolated fixture request phases: ${JSON.stringify(fixtureTraffic)}`);
      t.diagnostic(`shutdown confirmation: ${JSON.stringify({ status: shutdown.status,
        code: shutdown.body?.error?.code, completion: shutdown.body?.error?.completion,
        transportError: shutdown.transportError })}`);
      t.diagnostic(`isolated fixture worker diagnostics:\n${workerOutput}`);
    }
    // Never mask a shutdown failure with a later rm error, or delete files
    // underneath a process that has not actually exited.
    assert.deepEqual(live, [], `owned processes still alive; temp retained: ${tempRoot}`);
    assert.equal(forcedWorkerExit, false, `worker required forced exit; temp retained: ${tempRoot}`);
    assert.equal(shutdown.status, 200, `shutdown failed: ${JSON.stringify(shutdown)}; temp retained: ${tempRoot}`);
    await removeOwnedTempRoot(tempRoot);
  });

  return {
    tempRoot,
    oracle,
    diagnoseNavigationFailure(reply) {
      // Capture at the failing call, before later cases overwrite the bounded
      // worker tail. The reply summary excludes request bodies and URL queries.
      const error = reply.body?.error;
      t.diagnostic(`fixture navigation reply: ${JSON.stringify({ status: reply.status,
        code: typeof error?.code === "string" ? error.code : undefined,
        completion: typeof error?.completion === "string" ? error.completion : undefined })}`);
      if (process.env.GROK_CU_TEST_BROWSER_DEBUG === "1") {
        t.diagnostic(`fixture navigation phases: ${JSON.stringify(fixtureTraffic)}`);
        t.diagnostic(`fixture navigation worker diagnostics:\n${workerOutput}`);
      }
    },
    stalledNavigations: () => stalledNavigations,
    async releaseWaitControl(run, operation) {
      const deadline = performance.now() + 2000;
      while (!waitControls.has(run) && performance.now() < deadline) {
        await new Promise((resolve) => setTimeout(resolve, 10));
      }
      const response = waitControls.get(run);
      assert.ok(response, "fixture control connection must be live");
      response.end(operation);
    },
    pageUrl: (run) => `http://127.0.0.1:${pagePort}/?run=${encodeURIComponent(run)}`,
    pagePort,
    port: workerPort,
    token,
  };
}

async function getOracle(pagePort, runId) {
  return new Promise((resolvePromise, reject) => {
    const req = request(
      {
        hostname: "127.0.0.1",
        port: pagePort,
        path: `/oracle?run=${encodeURIComponent(runId)}`,
      },
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

test("each simultaneous managed profile reports its own actual browser process", async t => {
  const h = await startHarness(t);
  for (const profile of ["pid-first", "pid-second"]) {
    const opened = await post(h.port, "/open", { profile, owner: profile }, h.token);
    assert.equal(opened.status, 200, JSON.stringify(opened.body));
  }
  const before = await post(h.port, "/pids", {}, h.token, { host: true });
  assert.equal(before.status, 200);
  assert.equal(before.body.browserPids.length, 2);
  assert.equal(new Set(before.body.browserPids).size, 2, "profiles must not both report the first child");
  const actualChildren = childPids(before.body.workerPid);
  assert.equal(before.body.browserPids.every(pid => actualChildren.includes(pid)), true,
    "each reported browser must belong to the worker in the independent OS snapshot");
  const [firstPid, secondPid] = before.body.browserPids;
  const closed = await post(h.port, "/close", { profile: "pid-first", owner: "pid-first" }, h.token);
  assert.equal(closed.status, 200, JSON.stringify(closed.body));
  assert.deepEqual(await waitForPidsToExit([firstPid]), []);
  assert.equal(processAlive(secondPid), true, "closing the first profile must not close the second");
  const after = await post(h.port, "/pids", {}, h.token, { host: true });
  assert.deepEqual(after.body.browserPids, [secondPid]);
  const tabs = await post(h.port, "/tabs", { profile: "pid-second", owner: "pid-second" }, h.token);
  assert.equal(tabs.status, 200, "remaining profile stays usable");
});

test("native navigation timeout is typed unknown and cannot redispatch its action", async (t) => {
  for (const kind of ["goto", "reload"]) await t.test(kind, async child => {
    const h = await startHarness(child);
    const owner = `navigation-timeout-${kind}`, profile = owner;
    const opened = await post(h.port, "/open", { owner, profile }, h.token);
    assert.equal(opened.status, 200, JSON.stringify(opened.body));
    const url = `http://127.0.0.1:${h.pagePort}/never-finish-navigation?token=fixture-private-marker&initial=${kind === "reload" ? "ready" : "pending"}`;
    const body = { owner, profile, pageId: opened.body.pageId, pageGeneration: opened.body.pageGeneration,
      actionId: "stalled-navigation" };
    const path = kind === "reload" ? "/act" : "/goto";
    if (kind === "reload") {
      const landed = await post(h.port, "/goto", { ...body, url, actionId: "initial-navigation" }, h.token);
      assert.equal(landed.status, 200, JSON.stringify(landed.body));
      body.pageGeneration = landed.body.pageGeneration;
      body.kind = "reload";
    } else body.url = url;
    const timed = await post(h.port, path, body, h.token);
    assert.equal(timed.status, 504, JSON.stringify(timed.body));
    assert.equal(timed.body.error.code, "navigation_timeout");
    assert.equal(timed.body.error.completion, "unknown");
    assert.equal(JSON.stringify(timed.body).includes("fixture-private-marker"), false);
    const dispatched = kind === "reload" ? 2 : 1;
    assert.equal(h.stalledNavigations(), dispatched);
    const duplicate = await post(h.port, path, body, h.token);
    assert.equal(duplicate.status, 409);
    assert.equal(duplicate.body.error.code, "unknown_quarantine");
    assert.equal(h.stalledNavigations(), dispatched, "an uncertain navigation cannot be retried automatically");
  });
});

test("concurrent close callers do not retire a profile before its browser exits", async t => {
  const h = await startHarness(t);
  const identity = { profile: "concurrent-close", owner: "concurrent-close-owner" };
  assert.equal((await post(h.port, "/open", identity, h.token)).status, 200);
  const before = await post(h.port, "/pids", {}, h.token, { host: true });
  assert.equal(before.body.browserPids.length, 1);
  const [oldPid] = before.body.browserPids;
  const attempts = await Promise.all([1, 2].map(async () => {
    const result = await post(h.port, "/close", identity, h.token);
    // A caller admitted after retirement can legitimately receive not-found.
    assert.ok([200, 404].includes(result.status), JSON.stringify(result.body));
    assert.equal(processAlive(oldPid), false, "close response must follow actual browser exit");
    return result.status;
  }));
  assert.ok(attempts.includes(200));
  assert.deepEqual((await post(h.port, "/pids", {}, h.token, { host: true })).body.browserPids, []);
  const replacement = await post(h.port, "/open", identity, h.token);
  assert.equal(replacement.status, 200, JSON.stringify(replacement.body));
  const after = await post(h.port, "/pids", {}, h.token, { host: true });
  assert.equal(after.body.browserPids.length, 1);
  assert.equal(processAlive(after.body.browserPids[0]), true);
  assert.equal((await post(h.port, "/tabs", identity, h.token)).status, 200);
});

test("typed act identity matrix: click set_value type_text select key", async (t) => {
  const harness = await startHarness(t);
  const profile = "typed-act";
  const owner = "typed-owner";
  const opened = await post(harness.port, "/open", { profile, owner }, harness.token);
  assert.equal(opened.status, 200);
  const ident = {
    profile,
    owner,
    pageId: opened.body.pageId,
    pageGeneration: opened.body.pageGeneration,
  };
  const landed = await post(
    harness.port,
    "/goto",
    { ...ident, url: harness.pageUrl(owner), actionId: "typed-goto" },
    harness.token,
  );
  assert.equal(landed.status, 200);
  ident.pageId = landed.body.pageId;
  ident.pageGeneration = landed.body.pageGeneration;

  const first = await observe(harness.port, harness.token, ident);
  ident.pageGeneration = first.pageGeneration;
  const plus = nodeNamed(first, "+1", "button");
  const nameField = nodeNamed(first, "Name");
  const colorField = nodeNamed(first, "color");
  const submit = nodeNamed(first, "Submit", "button");
  assert.ok(plus?.elementRef, "plus button ref");
  assert.ok(nameField?.elementRef, "name field ref");
  assert.ok(colorField?.elementRef, "color select ref");
  assert.ok(submit?.elementRef, "submit ref");

  const roleOnly = await post(
    harness.port,
    "/act",
    {
      ...ident,
      snapshotId: first.snapshotId,
      actionId: "click-role-only",
      kind: "click",
      role: "button",
      name: "+1",
    },
    harness.token,
  );
  assert.equal(roleOnly.status, 400);
  assert.equal(roleOnly.body.ok, false);
  assert.equal(roleOnly.body.error.completion, "not_started");
  assert.match(roleOnly.body.error.code, /element_ref|invalid_request|schema/);
  assert.equal(await getOracle(harness.pagePort, owner), null);

  const dummyKey = await post(
    harness.port,
    "/act",
    {
      ...ident,
      snapshotId: first.snapshotId,
      actionId: "key-dummy",
      kind: "key",
      elementRef: "dummy",
      key: "enter",
    },
    harness.token,
  );
  assert.ok(dummyKey.status === 400 || dummyKey.status === 409);
  assert.equal(dummyKey.body.ok, false);
  assert.equal(dummyKey.body.error.completion, "not_started");

  const keyNoSnap = await post(
    harness.port,
    "/act",
    {
      ...ident,
      actionId: "key-no-snap",
      kind: "key",
      key: "enter",
    },
    harness.token,
  );
  assert.equal(keyNoSnap.status, 400);
  assert.equal(keyNoSnap.body.error.completion, "not_started");

  const click = await post(
    harness.port,
    "/act",
    {
      ...ident,
      snapshotId: first.snapshotId,
      actionId: "click-plus",
      kind: "click",
      elementRef: plus.elementRef,
    },
    harness.token,
  );
  assert.equal(click.status, 200, JSON.stringify(click.body));
  assert.equal(click.body.ok, true);
  ident.pageGeneration = click.body.pageGeneration || ident.pageGeneration;
  await new Promise((resolvePromise) => setTimeout(resolvePromise, 50));
  const afterClick = await getOracle(harness.pagePort, owner);
  assert.equal(afterClick?.count, 1);

  const afterClickObs = await observe(harness.port, harness.token, ident);
  ident.pageGeneration = afterClickObs.pageGeneration;
  const name2 = nodeNamed(afterClickObs, "Name");
  const setFirst = await post(
    harness.port,
    "/act",
    {
      ...ident,
      snapshotId: afterClickObs.snapshotId,
      actionId: "set-ab",
      kind: "set_value",
      elementRef: name2.elementRef,
      text: "ab",
    },
    harness.token,
  );
  assert.equal(setFirst.status, 200, JSON.stringify(setFirst.body));

  const afterSet = await observe(harness.port, harness.token, ident);
  ident.pageGeneration = afterSet.pageGeneration;
  const name3 = nodeNamed(afterSet, "Name");
  const typed = await post(
    harness.port,
    "/act",
    {
      ...ident,
      snapshotId: afterSet.snapshotId,
      actionId: "type-cd",
      kind: "type_text",
      elementRef: name3.elementRef,
      text: "cd",
    },
    harness.token,
  );
  assert.equal(typed.status, 200, JSON.stringify(typed.body));

  const afterType = await observe(harness.port, harness.token, ident);
  ident.pageGeneration = afterType.pageGeneration;
  const name4 = nodeNamed(afterType, "Name");
  const replace = await post(
    harness.port,
    "/act",
    {
      ...ident,
      snapshotId: afterType.snapshotId,
      actionId: "set-replace",
      kind: "set_value",
      elementRef: name4.elementRef,
      text: "ab",
    },
    harness.token,
  );
  assert.equal(replace.status, 200, JSON.stringify(replace.body));
  const afterReplace = await observe(harness.port, harness.token, ident);
  ident.pageGeneration = afterReplace.pageGeneration;
  const name5 = nodeNamed(afterReplace, "Name");
  const appendAgain = await post(
    harness.port,
    "/act",
    {
      ...ident,
      snapshotId: afterReplace.snapshotId,
      actionId: "type-cd-2",
      kind: "type_text",
      elementRef: name5.elementRef,
      text: "cd",
    },
    harness.token,
  );
  assert.equal(appendAgain.status, 200, JSON.stringify(appendAgain.body));

  const beforeSelect = await observe(harness.port, harness.token, ident);
  ident.pageGeneration = beforeSelect.pageGeneration;
  const color = nodeNamed(beforeSelect, "color");
  const nameOnSelect = nodeNamed(beforeSelect, "Name");
  const badSelect = await post(
    harness.port,
    "/act",
    {
      ...ident,
      snapshotId: beforeSelect.snapshotId,
      actionId: "select-on-input",
      kind: "select",
      elementRef: nameOnSelect.elementRef,
      value: "blue",
    },
    harness.token,
  );
  assert.equal(badSelect.status, 400);
  assert.equal(badSelect.body.error.completion, "not_started");
  assert.match(String(badSelect.body.error.code), /select|invalid/);

  const goodSelect = await post(
    harness.port,
    "/act",
    {
      ...ident,
      snapshotId: beforeSelect.snapshotId,
      actionId: "select-color",
      kind: "select",
      elementRef: color.elementRef,
      value: "blue",
    },
    harness.token,
  );
  assert.equal(goodSelect.status, 200, JSON.stringify(goodSelect.body));

  const beforeSubmit = await observe(harness.port, harness.token, ident);
  ident.pageGeneration = beforeSubmit.pageGeneration;
  const submit2 = nodeNamed(beforeSubmit, "Submit", "button");
  const submitted = await post(
    harness.port,
    "/act",
    {
      ...ident,
      snapshotId: beforeSubmit.snapshotId,
      actionId: "submit-form",
      kind: "click",
      elementRef: submit2.elementRef,
    },
    harness.token,
  );
  assert.equal(submitted.status, 200, JSON.stringify(submitted.body));
  await new Promise((resolvePromise) => setTimeout(resolvePromise, 80));
  const finalOracle = await getOracle(harness.pagePort, owner);
  assert.equal(finalOracle?.result?.count, 1);
  assert.equal(
    finalOracle?.result?.name,
    "abcd",
    `set_value replaces then type_text appends; got ${JSON.stringify(finalOracle)}`,
  );
});

test("upload pins its observed input through mutation and replays without another file event", async (t) => {
  const harness = await startHarness(t);
  const profile = "upload-handles"; const owner = "upload-owner";
  const opened = await post(harness.port, "/open", { profile, owner }, harness.token);
  assert.equal(opened.status, 200);
  const ident = { profile, owner, pageId: opened.body.pageId, pageGeneration: opened.body.pageGeneration };
  const landed = await post(harness.port, "/goto", {
    ...ident, url: harness.pageUrl(owner), actionId: "upload-goto",
  }, harness.token);
  assert.equal(landed.status, 200);
  ident.pageGeneration = landed.body.pageGeneration;
  const observed = await observe(harness.port, harness.token, ident);
  ident.pageGeneration = observed.pageGeneration;
  const file = nodeNamed(observed, "Upload fixture");
  const staging = join(harness.tempRoot, "staging", "computer-use-staging", owner);
  mkdirSync(staging, { recursive: true });
  const path = join(staging, "owned-upload.txt");
  writeFileSync(path, "test-body", "utf8");
  const request = { ...ident, snapshotId: observed.snapshotId, actionId: "upload-once",
    elementRef: file.elementRef, path };
  const uploaded = await post(harness.port, "/upload", request, harness.token);
  assert.equal(uploaded.status, 200, JSON.stringify(uploaded.body));
  const until = performance.now() + 2000;
  while (!harness.oracle.get(owner)?.uploaded && performance.now() < until) {
    await new Promise(resolve => setTimeout(resolve, 10));
  }
  assert.deepEqual(harness.oracle.get(owner)?.uploaded, { name: "owned-upload.txt", size: 9 });
  assert.equal(harness.oracle.get(owner)?.uploadEvents, 1);
  const replay = await post(harness.port, "/upload", request, harness.token);
  assert.equal(replay.status, 200);
  assert.equal(replay.body.replayed, true);
  assert.equal(harness.oracle.get(owner)?.uploadEvents, 1);
  const stale = await post(harness.port, "/upload", { ...request, actionId: "upload-stale" }, harness.token);
  assert.equal(stale.status, 409);
});

test("typed act scroll and drag use opaque refs, not selectors", async (t) => {
  const harness = await startHarness(t);
  const profile = "typed-space";
  const owner = "space-owner";
  const opened = await post(harness.port, "/open", { profile, owner }, harness.token);
  assert.equal(opened.status, 200);
  const ident = {
    profile,
    owner,
    pageId: opened.body.pageId,
    pageGeneration: opened.body.pageGeneration,
  };
  const landed = await post(
    harness.port,
    "/goto",
    { ...ident, url: harness.pageUrl(owner), actionId: "space-goto" },
    harness.token,
  );
  assert.equal(landed.status, 200);
  ident.pageId = landed.body.pageId;
  ident.pageGeneration = landed.body.pageGeneration;
  const first = await observe(harness.port, harness.token, ident);
  ident.pageGeneration = first.pageGeneration;
  const scroller = nodeNamed(first, "scroller");
  const drag = nodeNamed(first, "Drag");
  const drop = nodeNamed(first, "Drop");

  const selectorDrag = await post(
    harness.port,
    "/act",
    {
      ...ident,
      snapshotId: first.snapshotId,
      actionId: "drag-selector",
      kind: "drag",
      elementRef: drag.elementRef,
      toSelector: "#drop",
    },
    harness.token,
  );
  assert.equal(selectorDrag.status, 400);
  assert.equal(selectorDrag.body.error.completion, "not_started");
  assert.equal(await getOracle(harness.pagePort, owner), null);

  const scrolled = await post(
    harness.port,
    "/act",
    {
      ...ident,
      snapshotId: first.snapshotId,
      actionId: "scroll-box",
      kind: "scroll",
      elementRef: scroller.elementRef,
      delta: 180,
    },
    harness.token,
  );
  assert.equal(scrolled.status, 200, JSON.stringify(scrolled.body));
  await new Promise((resolvePromise) => setTimeout(resolvePromise, 80));
  const afterScroll = await getOracle(harness.pagePort, owner);
  assert.ok(Number(afterScroll?.scrollTop) >= 100, JSON.stringify(afterScroll));

  const afterScrollObs = await observe(harness.port, harness.token, ident);
  ident.pageGeneration = afterScrollObs.pageGeneration;
  const drag2 = nodeNamed(afterScrollObs, "Drag");
  const drop2 = nodeNamed(afterScrollObs, "Drop");
  const dragged = await post(
    harness.port,
    "/act",
    {
      ...ident,
      snapshotId: afterScrollObs.snapshotId,
      actionId: "drag-drop",
      kind: "drag",
      elementRef: drag2.elementRef,
      destElementRef: drop2.elementRef,
    },
    harness.token,
  );
  assert.equal(dragged.status, 200, JSON.stringify(dragged.body));
  await new Promise((resolvePromise) => setTimeout(resolvePromise, 80));
  const afterDrag = await getOracle(harness.pagePort, owner);
  assert.equal(afterDrag?.dropped, 1, JSON.stringify(afterDrag));
});

test("real pixel drag validates screenshot authority, reaches the explicit point, and never replays", async (t) => {
  const h = await startHarness(t);
  const owner = "pixel-drag-owner";
  const profile = "pixel-drag";
  const opened = await post(h.port, "/open", { profile, owner }, h.token);
  assert.equal(opened.status, 200, JSON.stringify(opened.body));
  const landed = await post(h.port, "/goto", { profile, owner, pageId: opened.body.pageId,
    pageGeneration: opened.body.pageGeneration, actionId: "pixel-land",
    url: `http://127.0.0.1:${h.pagePort}/drag-fixture?run=${owner}` }, h.token);
  assert.equal(landed.status, 200, JSON.stringify(landed.body));
  const ident = { profile, owner, pageId: landed.body.pageId, pageGeneration: landed.body.pageGeneration };
  const text = await post(h.port, "/observe", { ...ident, screenshot: false }, h.token);
  assert.equal(text.status, 200, JSON.stringify(text.body));
  const noImage = await post(h.port, "/act", { ...ident, actionId: "pixel-no-image", kind: "drag",
    snapshotId: text.body.snapshotId, elementRef: nodeNamed(text.body, "Source").elementRef,
    toX: 220, toY: 190 }, h.token);
  assert.equal(noImage.status, 400, JSON.stringify(noImage.body));
  assert.equal(noImage.body.error.code, "visual_observation_required");
  assert.equal(noImage.body.error.completion, "not_started");
  assert.equal(await getOracle(h.pagePort, owner), null);
  const visual = await observe(h.port, h.token, ident);
  assert.equal(visual.textOnly, false);
  const request = { ...ident, kind: "drag", actionId: "pixel-explicit",
    snapshotId: visual.snapshotId, elementRef: nodeNamed(visual, "Source").elementRef,
    toX: 220, toY: 190 };
  for (const [index, extra] of [
    { toX: null }, { toX: "220" }, { toX: visual.image.width },
    { toY: visual.image.height }, { destElementRef: nodeNamed(visual, "Destination").elementRef },
  ].entries()) {
    const rejected = await post(h.port, "/act", { ...request, ...extra, actionId: `pixel-invalid-${index}` }, h.token);
    assert.equal(rejected.status, 400, JSON.stringify(rejected.body));
    assert.equal(rejected.body.error.completion, "not_started");
    assert.equal(await getOracle(h.pagePort, owner), null);
  }
  const applied = await post(h.port, "/act", request, h.token);
  assert.equal(applied.status, 200, JSON.stringify(applied.body));
  const deadline = performance.now() + 2000;
  while ((await getOracle(h.pagePort, owner))?.drops !== 1 && performance.now() < deadline) {
    await new Promise(resolve => setTimeout(resolve, 10));
  }
  assert.equal((await getOracle(h.pagePort, owner))?.drops, 1);
  const duplicate = await post(h.port, "/act", request, h.token);
  assert.equal(duplicate.status, 200, JSON.stringify(duplicate.body));
  assert.equal(duplicate.body.replayed, true);
  assert.equal((await getOracle(h.pagePort, owner))?.drops, 1);
  const stale = await post(h.port, "/act", { ...request, actionId: "pixel-stale" }, h.token);
  assert.equal(stale.status, 409);
  assert.equal(stale.body.error.completion, "not_started");
  assert.equal((await getOracle(h.pagePort, owner))?.drops, 1);
});

test("real wait follows the original DOM element and rejects retirement, navigation and cancel", async (t) => {
  const harness = await startHarness(t);
  for (const operation of ["name", "replace", "hide", "snapshot", "navigate", "cancel"]) {
    await t.test(operation, async () => {
      const owner = `wait-race-${operation}`;
      const profile = owner;
      const opened = await post(harness.port, "/open", { profile, owner }, harness.token);
      assert.equal(opened.status, 200);
      const landed = await post(harness.port, "/goto", {
        owner, profile, pageId: opened.body.pageId, pageGeneration: opened.body.pageGeneration,
        url: `http://127.0.0.1:${harness.pagePort}/wait-fixture?run=${owner}`, actionId: "land",
      }, harness.token);
      if (landed.status !== 200) harness.diagnoseNavigationFailure(landed);
      assert.equal(landed.status, 200, `initial navigation for ${operation}: ${JSON.stringify({
        status: landed.status, code: landed.body?.error?.code, completion: landed.body?.error?.completion })}`);
      const ident = { owner, profile, pageId: landed.body.pageId, pageGeneration: landed.body.pageGeneration };
      const observation = await observe(harness.port, harness.token, ident);
      const watched = nodeNamed(observation, "Waiting", "button");
      const body = { ...ident, kind: "wait", actionId: "wait-original", snapshotId: observation.snapshotId,
        elementRef: watched.elementRef, nameEquals: "Ready", timeoutMs: 4000 };
      const pending = post(harness.port, "/act", body, harness.token);
      const duplicate = await post(harness.port, "/act", body, harness.token);
      assert.equal(duplicate.status, 409, JSON.stringify(duplicate.body));
      assert.equal(duplicate.body.error.code, "action_in_flight");
      // A distinct element already has the desired name. Only this retained
      // element may satisfy the condition; no name/selector lookup is allowed.
      const competing = await post(harness.port, "/act", {
        ...body, kind: "click", actionId: "during-wait",
      }, harness.token);
      assert.equal(competing.status, 409);
      if (operation === "snapshot") {
        await observe(harness.port, harness.token, ident);
        await harness.releaseWaitControl(owner, "name");
      } else if (operation === "cancel") {
        const stopped = await post(harness.port, "/cancel-run", { owner }, harness.token, { host: true });
        assert.equal(stopped.status, 200, JSON.stringify(stopped.body));
      } else {
        await harness.releaseWaitControl(owner, operation);
      }
      const result = await pending;
      if (operation === "name") {
        assert.equal(result.status, 200, JSON.stringify(result.body));
        const replay = await post(harness.port, "/act", body, harness.token);
        assert.equal(replay.status, 200);
        assert.equal(replay.body.replayed, true);
      } else {
        assert.ok(result.status >= 400, JSON.stringify(result.body));
        if (operation !== "cancel") {
          assert.ok(["stale_element_ref", "wait_target_hidden", "observation_required"].includes(result.body.error.code),
            JSON.stringify(result.body));
        }
      }
      if (operation !== "cancel") {
        const deadline = performance.now() + 2000;
        while (!harness.oracle.has(owner) && performance.now() < deadline) {
          await new Promise((resolve) => setTimeout(resolve, 10));
        }
        assert.equal(harness.oracle.get(owner)?.count, 0, "wait must never click");
        const closed = await post(harness.port, "/close", { owner, profile }, harness.token, { host: true });
        assert.equal(closed.status, 200, `original fixture profile must close: ${JSON.stringify(closed.body)}`);
        assert.equal(closed.body.closed, profile);
      }
    });
  }
});

test("pause drains real wait without closing profile; resume fences late old requests", async (t) => {
  const h = await startHarness(t);
  const owner = "pause-wait-owner", profile = "pause-wait";
  const opened = await post(h.port, "/open", { owner, profile }, h.token);
  assert.equal(opened.status, 200);
  const landed = await post(h.port, "/goto", {
    owner, profile, pageId: opened.body.pageId, pageGeneration: opened.body.pageGeneration,
    actionId: "land", url: h.pageUrl(owner),
  }, h.token);
  assert.equal(landed.status, 200);
  const ident = { owner, profile, pageId: landed.body.pageId, pageGeneration: landed.body.pageGeneration, runRevision: 1 };
  const observation = await observe(h.port, h.token, ident);
  const plus = nodeNamed(observation, "+1", "button");
  const wait = { ...ident, kind: "wait", actionId: "wait", snapshotId: observation.snapshotId,
    elementRef: plus.elementRef, nameEquals: "not-ready", timeoutMs: 4000 };
  const pending = post(h.port, "/act", wait, h.token);
  const duplicate = await post(h.port, "/act", wait, h.token);
  assert.equal(duplicate.body.error.code, "action_in_flight", "real Wait entered before fence");
  for (const path of ["/pause-run", "/resume-run", "/run-status"]) {
    const unauthorized = await post(h.port, path, { owner, runRevision: 1, nextRunRevision: 2 }, h.token);
    assert.equal(unauthorized.status, 403, `${path} requires the Host control marker as well as Bearer`);
  }
  const paused = await post(h.port, "/pause-run", { owner, runRevision: 1 }, h.token, { host: true });
  assert.equal(paused.status, 200, JSON.stringify(paused.body));
  assert.equal(paused.body.phase, "paused");
  assert.ok((await pending).status >= 400, "cancelled Wait cannot succeed");
  const idle = await post(h.port, "/run-status", { owner, runRevision: 1 }, h.token, { host: true });
  assert.equal(idle.status, 200);
  assert.equal(idle.body.idle, true);
  assert.equal(idle.body.activeOperations, 0);
  const blocked = await post(h.port, "/act", { ...wait, kind: "click", actionId: "paused-click" }, h.token);
  assert.equal(blocked.status, 409);
  const resumed = await post(h.port, "/resume-run", { owner, runRevision: 1, nextRunRevision: 2 }, h.token, { host: true });
  assert.equal(resumed.status, 200, JSON.stringify(resumed.body));
  const late = await post(h.port, "/act", { ...wait, kind: "click", actionId: "late-click" }, h.token);
  assert.equal(late.status, 409);
  assert.equal(late.body.error.code, "stale_run_revision");
  const missing = await post(h.port, "/observe", { ...ident, runRevision: undefined }, h.token);
  assert.equal(missing.status, 409, "omitting the revision cannot bypass the fence");
  const oldPause = await post(h.port, "/pause-run", { owner, runRevision: 1 }, h.token, { host: true });
  assert.equal(oldPause.status, 409, "old cleanup cannot pause the new epoch");
  assert.equal(await getOracle(h.pagePort, owner), null, "old/paused requests dispatched no input");
  ident.runRevision = 2;
  const fresh = await observe(h.port, h.token, ident);
  assert.equal(fresh.pageId, ident.pageId, "pause kept the actual tab open");
  const clicked = await post(h.port, "/act", { ...ident, kind: "click", actionId: "new-click",
    snapshotId: fresh.snapshotId, elementRef: nodeNamed(fresh, "+1", "button").elementRef }, h.token);
  assert.equal(clicked.status, 200, JSON.stringify(clicked.body));
  const deadline = performance.now() + 2000;
  while (!(await getOracle(h.pagePort, owner)) && performance.now() < deadline) {
    await new Promise(resolve => setTimeout(resolve, 10));
  }
  assert.equal((await getOracle(h.pagePort, owner))?.count, 1, "fresh action executed exactly once");
});

test("typed act wait is read-only and reload drops dummy refs", async (t) => {
  const harness = await startHarness(t);
  const profile = "typed-wait";
  const owner = "wait-owner";
  const opened = await post(harness.port, "/open", { profile, owner }, harness.token);
  assert.equal(opened.status, 200);
  const ident = {
    profile,
    owner,
    pageId: opened.body.pageId,
    pageGeneration: opened.body.pageGeneration,
  };
  const landed = await post(
    harness.port,
    "/goto",
    { ...ident, url: harness.pageUrl(owner), actionId: "wait-goto" },
    harness.token,
  );
  assert.equal(landed.status, 200);
  ident.pageId = landed.body.pageId;
  ident.pageGeneration = landed.body.pageGeneration;
  const first = await observe(harness.port, harness.token, ident);
  ident.pageGeneration = first.pageGeneration;
  const plus = nodeNamed(first, "+1", "button");

  const waited = await post(
    harness.port,
    "/act",
    {
      ...ident,
      snapshotId: first.snapshotId,
      elementRef: plus.elementRef,
      kind: "wait",
      actionId: "wait-match",
      nameEquals: "+1",
      timeoutMs: 1000,
    },
    harness.token,
  );
  assert.equal(waited.status, 200, JSON.stringify(waited.body));
  assert.equal(waited.body.pageGeneration, first.pageGeneration);

  const still = await post(
    harness.port,
    "/act",
    {
      ...ident,
      snapshotId: first.snapshotId,
      actionId: "click-after-wait",
      kind: "click",
      elementRef: plus.elementRef,
    },
    harness.token,
  );
  assert.equal(still.status, 200, JSON.stringify(still.body));
  ident.pageGeneration = still.body.pageGeneration || ident.pageGeneration;
  await new Promise((resolvePromise) => setTimeout(resolvePromise, 50));
  assert.equal((await getOracle(harness.pagePort, owner))?.count, 1);

  const afterClick = await observe(harness.port, harness.token, ident);
  ident.pageGeneration = afterClick.pageGeneration;
  const plus2 = nodeNamed(afterClick, "+1", "button");
  const timed = await post(
    harness.port,
    "/act",
    {
      ...ident,
      snapshotId: afterClick.snapshotId,
      elementRef: plus2.elementRef,
      kind: "wait",
      actionId: "wait-timeout",
      nameEquals: "never-visible",
      timeoutMs: 120,
    },
    harness.token,
  );
  assert.equal(timed.status, 400);
  assert.equal(timed.body.error.code, "wait_timeout");
  assert.equal(timed.body.error.completion, "not_started");

  const dummyReload = await post(
    harness.port,
    "/act",
    {
      ...ident,
      actionId: "reload-dummy",
      kind: "reload",
      elementRef: "dummy",
    },
    harness.token,
  );
  assert.equal(dummyReload.status, 400);

  const reloaded = await post(
    harness.port,
    "/act",
    { ...ident, actionId: "reload-ok", kind: "reload" },
    harness.token,
  );
  assert.equal(reloaded.status, 200, JSON.stringify(reloaded.body));
  assert.ok(reloaded.body.pageGeneration > ident.pageGeneration);
  const stale = await post(harness.port, "/observe", ident, harness.token);
  assert.equal(stale.status, 409);
  assert.equal(stale.body.error.currentPageGeneration, reloaded.body.pageGeneration);
});
