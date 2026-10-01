import test from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { randomBytes } from "node:crypto";
import { mkdtemp, realpath, rm } from "node:fs/promises";
import { createServer, request } from "node:http";
import { tmpdir } from "node:os";
import { join, relative } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { childEnv } from "./child-env.mjs";
import { confirmWorkerShutdown } from "./fixtures/cleanup-reconcile.mjs";

const ROOT = fileURLToPath(new URL(".", import.meta.url));
const HTML = `<!doctype html><meta charset="utf-8"><title>Owned capture fixture</title>
<input id="value" aria-label="Value" value="before"><button id="change">Change</button><output id="count">0</output>
<script>document.getElementById('change').onclick=()=>{const n=Number(document.getElementById('count').textContent)+1;
document.getElementById('count').textContent=n;document.getElementById('value').value='changed '+n;};</script>`;

async function within(pending, label, ms = 15000) {
  let timer;
  try { return await Promise.race([pending, new Promise((_, reject) => {
    timer = setTimeout(() => reject(new Error(`${label} timed out`)), ms);
  })]); } finally { clearTimeout(timer); }
}

function post(port, token, path, body = {}, timeoutMs = 20000) {
  return new Promise((resolve, reject) => {
    const data = Buffer.from(JSON.stringify(body));
    const req = request({ hostname: "127.0.0.1", port, path, method: "POST", headers: {
      authorization: `Bearer ${token}`, "x-grok-cu-host": "1", "content-type": "application/json", "content-length": data.length,
    } }, res => {
      let text = "";
      res.setEncoding("utf8");
      res.on("data", chunk => { text += chunk; if (text.length > 512 * 1024) req.destroy(new Error("reply limit")); });
      res.on("error", reject);
      res.on("end", () => { try { resolve({ status: res.statusCode, body: JSON.parse(text) }); } catch { reject(new Error("invalid reply")); } });
    });
    req.setTimeout(timeoutMs, () => req.destroy(new Error(`${path} timeout`)));
    req.on("error", reject); req.end(data);
  });
}

test("native managed captures cannot resurrect input authority across mutation or unknown completion", { timeout: 150000 }, async t => {
  assert.ok(process.env.GROK_CU_CHROME, "explicit isolated-test Chromium is required");
  const base = await realpath(tmpdir()), root = await mkdtemp(join(base, "grok-cu-capture-race-"));
  const responses = new Set(), requested = new Map();
  const pageServer = createServer((req, res) => {
    const path = new URL(req.url, "http://127.0.0.1").pathname;
    requested.set(path, (requested.get(path) || 0) + 1);
    if (path === "/pending" || path === "/timeout") {
      responses.add(res); res.once("close", () => responses.delete(res));
      if (path === "/timeout") {
        res.writeHead(200, { "content-type": "text/html" });
        res.write("<!doctype html><title>Pending owned fixture</title><script>");
      }
    } else { res.writeHead(200, { "content-type": "text/html; charset=utf-8" }); res.end(HTML); }
  });
  await new Promise(resolve => pageServer.listen(0, "127.0.0.1", resolve));
  const url = path => `http://127.0.0.1:${pageServer.address().port}${path}`;
  const token = randomBytes(32).toString("hex"), worker = process.env.GROK_CU_TEST_WORKER || join(ROOT, "server.mjs");
  const child = spawn(process.execPath, ["--import", pathToFileURL(join(ROOT, "fixtures/capture-barrier.preload.mjs")).href, worker], {
    cwd: ROOT, windowsHide: true, stdio: ["ignore", "pipe", "pipe", "ipc"],
    env: childEnv({ GROK_CU_BROWSER_TOKEN: token, GROK_CU_BROWSER_PORT: "0",
      GROK_CU_BROWSER_PROFILE_ROOT: join(root, "profiles"), GROK_CU_BROWSER_STAGING_ROOT: join(root, "staging") }),
  });
  const exited = new Promise(resolve => child.once("exit", (code, signal) => resolve({ code, signal })));
  const inbox = new Map(), waiters = new Map();
  let sequence = 0, stderr = "", port;
  child.stderr.on("data", chunk => { stderr = (stderr + chunk.toString()).slice(-4096); });
  child.on("message", message => {
    const key = `${message.kind}:${message.id}`;
    if (waiters.has(key)) { waiters.get(key)(message); waiters.delete(key); }
    else inbox.set(key, message);
  });
  const receive = (kind, id) => {
    const key = `${kind}:${id}`;
    if (inbox.has(key)) { const message = inbox.get(key); inbox.delete(key); return Promise.resolve(message); }
    return within(new Promise(resolve => waiters.set(key, resolve)), key);
  };
  const command = async (kind, page = 0) => {
    const id = ++sequence;
    child.send({ kind, id, profile: "capture-race", page });
    const reply = await receive("reply", id);
    assert.equal(reply.error, undefined);
    return { id, ...reply };
  };
  t.after(async () => {
    for (const response of responses) response.end("</script>" + HTML);
    let shutdown;
    try {
      if (child.connected) await command("release-all");
      const pids = port ? await post(port, token, "/pids") : null;
      assert.equal(pids?.status, 200);
      assert.equal(pids.body.workerPid, child.pid);
      assert.equal(pids.body.browserPids.length, 1, "one owned persistent context is expected");
      if (port) shutdown = await confirmWorkerShutdown((path, ms) => post(port, token, path, {}, ms));
      const terminal = await within(exited, "same owned worker exit", 10000);
      assert.equal(shutdown?.status, 200, `cleanup not confirmed; retain ${root}`);
      assert.deepEqual(terminal, { code: 0, signal: null });
      for (const pid of pids.body.browserPids) {
        assert.ok(Number.isSafeInteger(pid) && pid > 0);
        assert.throws(() => process.kill(pid, 0), error => error.code === "ESRCH", "original native browser still exists");
      }
      const rel = relative(base, root);
      assert.ok(rel.startsWith("grok-cu-capture-race-") && !rel.includes("/") && !rel.includes("\\"));
      await rm(root, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
      t.diagnostic("original worker shutdown 200 and exit 0; owned profiles removed");
    } catch (error) {
      t.diagnostic(`owned fixture retained: ${root}; ${error.message}`);
      if (child.exitCode == null && child.signalCode == null) child.kill();
      await within(exited, "failed fixture child exit", 5000).catch(() => {});
      throw error;
    } finally {
      pageServer.closeAllConnections();
      await new Promise(resolve => pageServer.close(resolve));
    }
  });
  port = await within(new Promise((resolve, reject) => {
    let output = "";
    child.once("error", reject);
    child.once("exit", () => reject(new Error(`worker exited before ready; ${stderr}`)));
    child.stdout.on("data", chunk => {
      output += chunk.toString();
      for (const line of output.split(/\r?\n/)) {
        try { const row = JSON.parse(line); if (Number.isInteger(row.port)) resolve(row.port); } catch { /* partial line */ }
      }
    });
  }), "worker ready");
  const send = (path, body) => post(port, token, path, body);
  const owner = { owner: "capture-race", profile: "capture-race", runRevision: 1 };
  const opened = await send("/open", owner);
  assert.equal(opened.status, 200);
  let primary = { ...owner, pageId: opened.body.pageId, pageGeneration: opened.body.pageGeneration };
  const goto = (identity, path) => send("/goto", { ...identity, actionId: `nav-${++sequence}`, url: url(path) });
  const landed = await goto(primary, "/first");
  assert.equal(landed.status, 200); primary.pageGeneration = landed.body.pageGeneration;
  const observe = async (identity, extra = {}) => {
    const reply = await send("/observe", { ...identity, screenshot: false, ...extra });
    assert.equal(reply.status, 200, JSON.stringify(reply.body)); return reply.body;
  };
  const click = (identity, observation) => send("/act", { ...identity, kind: "click", actionId: `click-${++sequence}`,
    snapshotId: observation.snapshotId, elementRef: observation.nodes.find(node => node.name === "Change")?.elementRef });
  let count = 0;
  for (const preview of [false, true]) {
    await t.test(`old native screenshot rejected after non-navigation click; preview=${preview}`, async () => {
      const old = await observe(primary), armed = await command("arm");
      const pending = send("/observe", { ...primary, screenshot: true, preview });
      const held = await receive("capture-held", armed.id);
      assert.ok(held.bytes > 100); assert.match(held.sha256, /^[0-9a-f]{64}$/);
      const action = await click(primary, old); assert.equal(action.status, 200, JSON.stringify(action.body));
      assert.deepEqual((await command("state")).state, { clicks: ++count, value: `changed ${count}` });
      await command("release");
      const late = await pending;
      assert.equal(late.status, 409); assert.equal(late.body.error.code, "observation_changed_during_capture");
      assert.equal(late.body.pngBase64, undefined); assert.equal(late.body.snapshotId, undefined);
      assert.equal((await click(primary, old)).status, 409, "old authority must stay retired");
      const fresh = await observe(primary); assert.notEqual(fresh.snapshotId, old.snapshotId);
    });
  }
  await t.test("a pending native navigation cannot be observed as an idle document", async () => {
    const pending = goto(primary, "/pending");
    await within((async () => { while (!requested.has("/pending")) await new Promise(resolve => setTimeout(resolve, 10)); })(), "native request");
    for (const preview of [false, true]) {
      const reply = await send("/observe", { ...primary, screenshot: false, preview });
      assert.equal(reply.status, 409); assert.equal(reply.body.error.code, "page_mutation_in_flight");
    }
    for (const response of responses) response.end(HTML);
    const complete = await pending; assert.equal(complete.status, 200); primary.pageGeneration = complete.body.pageGeneration;
    await observe(primary);
  });
  const second = await send("/new-tab", { ...owner, actionId: `tab-${++sequence}` });
  assert.equal(second.status, 200);
  const secondary = { ...owner, pageId: second.body.pageId, pageGeneration: second.body.pageGeneration };
  const secondLanded = await goto(secondary, "/second");
  assert.equal(secondLanded.status, 200); secondary.pageGeneration = secondLanded.body.pageGeneration;
  await t.test("capture on another page cannot clear a later unknown navigation outcome", async () => {
    const old = await observe(secondary);
    const armed = await command("arm", 1);
    const capture = send("/observe", { ...secondary, screenshot: true });
    await receive("capture-held", armed.id);
    const navigation = { ...primary, actionId: `unknown-${++sequence}`, url: url("/timeout") };
    const failed = await send("/goto", navigation);
    assert.equal(failed.status, 504); assert.equal(failed.body.error.completion, "unknown");
    for (const response of responses) response.end("</script>" + HTML);
    await command("release", 1);
    const untrusted = await capture;
    assert.equal(untrusted.status, 409); assert.equal(untrusted.body.error.code, "observation_changed_during_capture");
    const blocked = await click(secondary, old);
    assert.equal(blocked.status, 409); assert.equal(blocked.body.error.code, "unknown_quarantine");
    assert.deepEqual((await command("state", 1)).state, { clicks: 0, value: "before" });
    const fresh = await observe(secondary);
    const replay = await send("/goto", navigation);
    assert.equal(replay.status, 409); assert.equal(replay.body.error.completion, "unknown");
    assert.equal(requested.get("/timeout"), 1, "unknown navigation must never replay");
    assert.equal((await click(secondary, fresh)).status, 200);
    assert.deepEqual((await command("state", 1)).state, { clicks: 1, value: "changed 1" });
  });
});
