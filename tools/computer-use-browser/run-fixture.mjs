#!/usr/bin/env node
/**
 * Drive the shipped Playwright worker against the self-built form.
 * Isolated profiles. Page postconditions come from the fixture oracle port,
 * not production /marker or /state.
 */
import { randomBytes } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { createServer, request } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn } from "node:child_process";
import { clientAllowed } from "./loopback.mjs";
import { childEnv } from "./child-env.mjs";

const ROOT = dirname(fileURLToPath(import.meta.url));
const OUT = join(ROOT, ".run");
mkdirSync(OUT, { recursive: true });
const html = readFileSync(join(ROOT, "fixtures", "form.html"), "utf8");
const CHROME = [
  process.env.GROK_CU_TEST_CHROME,
  "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
  "C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe",
].find((path) => path && existsSync(path));

process.env.OPENAI_API_KEY = "SHOULD_NOT_REACH_CHILD";
process.env.GROK_API_KEY = "SHOULD_NOT_REACH_CHILD";

const token = randomBytes(32).toString("hex");
const tempRoot = mkdtempSync(join(realpathSync(tmpdir()), "grok-cu-run-fixture-"));
const oracle = new Map();

const pageServer = createServer((req, res) => {
  const url = new URL(req.url || "/", "http://127.0.0.1");
  if (req.method === "POST" && url.pathname === "/oracle") {
    const chunks = [];
    req.on("data", (chunk) => chunks.push(chunk));
    req.on("end", () => {
      try {
        const body = JSON.parse(Buffer.concat(chunks).toString("utf8"));
        if (body.runId) oracle.set(String(body.runId), body);
      } catch {
        /* ignore malformed oracle */
      }
      res.writeHead(204);
      res.end();
    });
    return;
  }
  if (req.method === "GET" && url.pathname === "/oracle") {
    const row = oracle.get(url.searchParams.get("run") || "") || null;
    const raw = JSON.stringify(row);
    res.writeHead(200, { "content-type": "application/json" });
    res.end(raw);
    return;
  }
  if ((req.url || "/").startsWith("/file.bin")) {
    res.writeHead(200, {
      "content-type": "application/octet-stream",
      "content-disposition": 'attachment; filename="report.bin"',
    });
    res.end(Buffer.from("staged-by-run"));
    return;
  }
  res.writeHead(200, { "content-type": "text/html; charset=utf-8" });
  res.end(html);
});

function post(port, path, body, { host = false } = {}) {
  return new Promise((resolve, reject) => {
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
        res.on("data", (c) => chunks.push(c));
        res.on("end", () => {
          const raw = Buffer.concat(chunks).toString("utf8");
          try {
            resolve({ status: res.statusCode, body: JSON.parse(raw) });
          } catch {
            reject(new Error(`bad json ${raw.slice(0, 200)}`));
          }
        });
      },
    );
    req.on("error", reject);
    req.end(data);
  });
}

function getOracle(pagePort, runId) {
  return new Promise((resolve, reject) => {
    const req = request(
      { hostname: "127.0.0.1", port: pagePort, path: `/oracle?run=${encodeURIComponent(runId)}` },
      (res) => {
        const chunks = [];
        res.on("data", (c) => chunks.push(c));
        res.on("end", () => {
          try {
            resolve(JSON.parse(Buffer.concat(chunks).toString("utf8")));
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

function actionId(prefix) {
  return `${prefix}-${randomBytes(8).toString("hex")}`;
}

function waitForExit(child, timeoutMs) {
  if (child.exitCode != null || child.signalCode != null) return Promise.resolve(child.exitCode);
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`worker ${child.pid} did not exit`)), timeoutMs);
    child.once("exit", (code) => {
      clearTimeout(timer);
      resolve(code);
    });
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
    await new Promise((resolve) => setTimeout(resolve, 50));
    live = pids.filter(processAlive);
  }
  return live;
}

async function observe(port, ident) {
  for (let attempt = 0; attempt < 4; attempt += 1) {
    const res = await post(port, "/observe", ident);
    if (res.status === 200 && res.body.ok === true) return res.body;
    const gen = res.body && res.body.error && res.body.error.currentPageGeneration;
    if (res.status === 409 && Number.isInteger(gen) && gen > 0) {
      ident.pageGeneration = gen;
      continue;
    }
    throw new Error(`observe ${JSON.stringify(res)}`);
  }
  throw new Error("observe did not settle on a generation");
}

function nodeNamed(observation, name, role) {
  const needle = String(name).replace(/\s+/g, " ").trim().toLowerCase();
  return observation.nodes.find((node) => {
    if (typeof node.elementRef !== "string") return false;
    if (role && node.role !== role) return false;
    const nodeName = String(node.name || "").replace(/\s+/g, " ").trim().toLowerCase();
    return nodeName === needle || nodeName.startsWith(`${needle} `);
  });
}

async function act(port, ident, kind, extra) {
  const seen = await observe(port, ident);
  const node = nodeNamed(seen, extra.name, extra.role);
  if (!node) {
    throw new Error(`no node role=${extra.role} name=${extra.name} in ${JSON.stringify(seen.nodes)}`);
  }
  const { role, name, ...rest } = extra;
  const body = {
    ...ident,
    pageId: seen.pageId,
    pageGeneration: seen.pageGeneration,
    snapshotId: seen.snapshotId,
    actionId: actionId(kind),
    kind: kind === "fill" ? "set_value" : kind,
    elementRef: node.elementRef,
    ...rest,
  };
  if (body.value != null && body.text == null) body.text = body.value;
  const res = await post(port, "/act", body);
  if (res.status !== 200 || res.body.ok !== true) {
    throw new Error(`act ${kind}: ${JSON.stringify(res)}`);
  }
  ident.pageId = res.body.pageId || ident.pageId;
  ident.pageGeneration = res.body.pageGeneration || ident.pageGeneration;
  await observe(port, ident);
  return res.body;
}

async function pass(workerPort, pagePort, profile, owner) {
  const open = await post(workerPort, "/open", { profile, owner });
  if (open.status !== 200 || !open.body.ok) {
    throw new Error(`open ${profile}: ${JSON.stringify(open)}`);
  }
  const ident = {
    profile,
    owner,
    pageId: open.body.pageId,
    pageGeneration: open.body.pageGeneration,
  };
  const pageUrl = `http://127.0.0.1:${pagePort}/?run=${encodeURIComponent(owner)}`;
  const go = await post(workerPort, "/goto", {
    ...ident,
    url: pageUrl,
    actionId: actionId("goto"),
  });
  if (go.status !== 200) throw new Error(`goto: ${JSON.stringify(go)}`);
  ident.pageId = go.body.pageId;
  ident.pageGeneration = go.body.pageGeneration;
  const before = await getOracle(pagePort, owner);
  await act(workerPort, ident, "fill", { role: "textbox", name: "Name", value: "fixture" });
  await act(workerPort, ident, "click", { role: "button", name: "+1" });
  await act(workerPort, ident, "click", { role: "button", name: "+1" });
  await act(workerPort, ident, "click", { role: "button", name: "Submit" });
  const after = await getOracle(pagePort, owner);
  await post(workerPort, "/close", { profile, owner });
  return { profile, owner, before, after, ident };
}

const envProbe = spawn(
  process.execPath,
  [
    "-e",
    "process.stdout.write(JSON.stringify({o:process.env.OPENAI_API_KEY||null,g:process.env.GROK_API_KEY||null}))",
  ],
  { env: childEnv({ GROK_CU_BROWSER_TOKEN: token }), stdio: ["ignore", "pipe", "ignore"] },
);
const envSeen = await new Promise((resolve, reject) => {
  const chunks = [];
  envProbe.stdout.on("data", (chunk) => chunks.push(chunk));
  envProbe.on("error", reject);
  envProbe.on("exit", (code) => {
    if (code !== 0) reject(new Error(`env probe ${code}`));
    else resolve(JSON.parse(Buffer.concat(chunks).toString("utf8")));
  });
});
if (envSeen.o != null || envSeen.g != null) {
  throw new Error("sentinel API keys leaked into child env");
}

await new Promise((resolve) => pageServer.listen(0, "127.0.0.1", resolve));
const pagePort = pageServer.address().port;

const workerEnv = childEnv({
  GROK_CU_BROWSER_TOKEN: token,
  GROK_CU_BROWSER_PORT: "0",
  GROK_CU_BROWSER_PROFILE_ROOT: join(tempRoot, "profiles"),
  GROK_CU_BROWSER_STAGING_ROOT: join(tempRoot, "staging"),
  ...(CHROME ? { GROK_CU_CHROME: CHROME } : {}),
});
const worker = spawn(process.execPath, [join(ROOT, "server.mjs")], {
  cwd: ROOT,
  stdio: ["ignore", "pipe", "pipe"],
  env: workerEnv,
});
let workerBuf = "";
const ownedPids = new Set([worker.pid]);
let workerPort;
let mainError;
let report;
try {
  workerPort = await new Promise((resolve, reject) => {
    const t = setTimeout(() => reject(new Error("worker port timeout")), 30_000);
    const tryParse = () => {
      const line = workerBuf.split(/\r?\n/).find((l) => l.startsWith("{"));
      if (!line) return;
      try {
        const v = JSON.parse(line);
        if (v.port) {
          clearTimeout(t);
          resolve(v.port);
        }
      } catch {
        /* keep */
      }
    };
    worker.stdout.on("data", (c) => {
      workerBuf += c.toString("utf8");
      tryParse();
    });
    worker.stderr.on("data", (c) => {
      workerBuf += c.toString("utf8");
    });
    worker.on("error", reject);
    worker.on("exit", (code) => {
      if (!String(workerBuf).includes('"port"')) {
        clearTimeout(t);
        reject(new Error(`worker exited ${code}: ${workerBuf.slice(0, 800)}`));
      }
    });
  });

  const lockA = await post(workerPort, "/open", { profile: "lock", owner: "run-a" });
  const lockB = await post(workerPort, "/open", { profile: "lock", owner: "run-b" });
  if (lockA.status !== 200 || lockB.status !== 409) {
    throw new Error(`exclusive profile expected 200 then 409, got ${lockA.status}/${lockB.status}`);
  }
  await post(workerPort, "/close", { profile: "lock", owner: "run-a" });

  const results = [];
  for (const n of [1, 2]) {
    results.push(await pass(workerPort, pagePort, `pass-${n}`, `owner-${n}`));
  }
  const consistent =
    results.length === 2 &&
    results.every(
      (r) =>
        r.after &&
        r.after.result &&
        r.after.result.count === 2 &&
        r.after.result.name === "fixture" &&
        r.before == null,
    );

  const dlOwner = "dl-owner";
  const dlProfile = "dl-profile";
  const opened = await post(workerPort, "/open", { profile: dlProfile, owner: dlOwner });
  if (opened.status !== 200) throw new Error(`download open ${JSON.stringify(opened)}`);
  const dlIdent = {
    profile: dlProfile,
    owner: dlOwner,
    pageId: opened.body.pageId,
    pageGeneration: opened.body.pageGeneration,
  };
  const landed = await post(workerPort, "/goto", {
    ...dlIdent,
    url: `http://127.0.0.1:${pagePort}/?run=${dlOwner}`,
    actionId: actionId("goto"),
  });
  dlIdent.pageId = landed.body.pageId;
  dlIdent.pageGeneration = landed.body.pageGeneration;
  const rejected = await post(workerPort, "/download", {
    ...dlIdent,
    path: "C:\\\\temp\\\\evil.bin",
    filename: "report.bin",
    actionId: actionId("dl-bad"),
  });
  if (rejected.status !== 400) {
    throw new Error(`model path must be 400, got ${rejected.status} ${JSON.stringify(rejected.body)}`);
  }
  const dlObs = await observe(workerPort, dlIdent);
  const dlNode = nodeNamed(dlObs, "download", "link");
  if (!dlNode) {
    throw new Error(`download link missing in ${JSON.stringify(dlObs.nodes)}`);
  }
  const saved = await post(workerPort, "/download", {
    ...dlIdent,
    pageId: dlObs.pageId,
    pageGeneration: dlObs.pageGeneration,
    snapshotId: dlObs.snapshotId,
    filename: "report.bin",
    elementRef: dlNode.elementRef,
    actionId: actionId("dl-ok"),
  });
  const { readFileSync: readStaged } = await import("node:fs");
  const staged =
    saved.status === 200 &&
    saved.body.path &&
    readStaged(saved.body.path, "utf8") === "staged-by-run" &&
    !String(saved.body.path).includes("evil");
  const pids = await post(workerPort, "/pids", {}, { host: true });
  await post(workerPort, "/close", { profile: dlProfile, owner: dlOwner });
  if (clientAllowed("8.8.8.8") || !clientAllowed("127.0.0.1") || !clientAllowed("::1")) {
    throw new Error("T15 loopback allowlist failed");
  }
  if (Number.isInteger(pids?.body?.workerPid)) ownedPids.add(Number(pids.body.workerPid));
  if (Array.isArray(pids?.body?.browserPids)) {
    for (const pid of pids.body.browserPids) ownedPids.add(Number(pid));
  }
  report = {
    ok: Boolean(consistent && staged),
    t15_loopback: true,
    t16_profile_exclusive: lockA.status === 200 && lockB.status === 409,
    t18_model_path_rejected: rejected.status === 400,
    t20_profile_owner: lockA.status === 200 && lockB.status === 409,
    env_sentinels_blocked: true,
    url: `http://127.0.0.1:${pagePort}/`,
    workerPort,
    exclusive: { first: lockA.status, second: lockB.status },
    results: results.map((r) => ({
      profile: r.profile,
      owner: r.owner,
      count: r.after && r.after.result && r.after.result.count,
      name: r.after && r.after.result && r.after.result.name,
    })),
    download: {
      rejected: rejected.status,
      saved: saved.status,
      staged,
    },
    pids: {
      workerPid: pids.body.workerPid,
      browserPids: pids.body.browserPids,
    },
  };
  if (!report.ok) mainError = new Error("fixture postconditions failed");
} catch (error) {
  mainError = error;
} finally {
  const cleanupErrors = [];
  try {
    if (workerPort) {
      const pids = await post(workerPort, "/pids", {}, { host: true }).catch(() => null);
      if (Number.isInteger(pids?.body?.workerPid)) ownedPids.add(Number(pids.body.workerPid));
      if (Array.isArray(pids?.body?.browserPids)) {
        for (const pid of pids.body.browserPids) ownedPids.add(Number(pid));
      }
      await post(workerPort, "/shutdown", {}, { host: true }).catch(() => {});
    }
  } catch (error) {
    cleanupErrors.push(String(error.message || error));
  }
  try {
    worker.kill();
  } catch {
    /* ignore */
  }
  try {
    await waitForExit(worker, 10_000);
  } catch (error) {
    cleanupErrors.push(String(error.message || error));
  }
  const live = await waitForPidsToExit([...ownedPids]);
  if (live.length) cleanupErrors.push(`pids still alive: ${live.join(",")}`);
  await new Promise((resolve) => pageServer.close(() => resolve()));
  try {
    rmSync(tempRoot, { recursive: true, force: true });
  } catch (error) {
    cleanupErrors.push(String(error.message || error));
  }
  if (report && report.pids) report.pids.liveAfterShutdown = live;
  if (mainError || cleanupErrors.length) {
    const failed = {
      ok: false,
      error: mainError ? String(mainError.message || mainError) : undefined,
      cleanupErrors,
      pids: report && report.pids,
      workerLog: workerBuf.slice(0, 2000),
    };
    writeFileSync(join(OUT, "playwright-form.json"), JSON.stringify(failed, null, 2));
    process.stderr.write(`${failed.error || cleanupErrors.join("; ")}\n`);
    process.exitCode = 2;
  } else if (report) {
    writeFileSync(join(OUT, "playwright-form.json"), JSON.stringify(report, null, 2));
    process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
  }
}
