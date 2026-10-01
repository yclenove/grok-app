import test from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { randomBytes } from "node:crypto";
import { mkdtemp, realpath, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, relative } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { childEnv } from "./child-env.mjs";

const ROOT = fileURLToPath(new URL(".", import.meta.url));
async function within(pending, label, ms = 10000) {
  let timer;
  try {
    return await Promise.race([pending, new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error(`${label} deadline exceeded`)), ms);
    })]);
  } finally { clearTimeout(timer); }
}

test("a rejected close with a real native close event cannot release HTTP ownership", { timeout: 100000 }, async t => {
  assert.ok(process.env.GROK_CU_CHROME, "provide an explicit isolated-test Chrome executable");
  const base = await realpath(tmpdir());
  const root = await mkdtemp(join(base, "grok-cu-rejected-close-"));
  const token = randomBytes(32).toString("hex");
  const child = spawn(process.execPath, ["--import", pathToFileURL(join(ROOT, "fixtures/cleanup-delay.preload.mjs")).href,
    process.env.GROK_CU_TEST_WORKER || join(ROOT, "server.mjs")], {
    cwd: ROOT, windowsHide: true, stdio: ["ignore", "pipe", "pipe", "ipc"],
    env: childEnv({ GROK_CU_BROWSER_TOKEN: token, GROK_CU_BROWSER_PORT: "0",
      GROK_CU_BROWSER_PROFILE_ROOT: join(root, "profiles"), GROK_CU_BROWSER_STAGING_ROOT: join(root, "staging") }),
  });
  const exited = new Promise(resolve => child.once("exit", (code, signal) => resolve({ code, signal })));
  const seen = new Map();
  const waiters = new Map();
  const opened = new Set();
  let startedOpen = false;
  let unconfirmedLaunches = 0;
  let startupError = "unknown";
  child.stderr.on("data", chunk => {
    const code = chunk.toString("utf8").match(/\bERR_[A-Z_]+\b/);
    if (code) startupError = code[0];
  });
  child.on("message", message => {
    const key = `${message?.kind}:${message?.profile}`;
    seen.set(key, message);
    waiters.get(key)?.(message);
    waiters.delete(key);
  });
  const wait = (kind, profile) => {
    const key = `${kind}:${profile}`;
    return seen.has(key) ? Promise.resolve(seen.get(key)) : new Promise(resolve => waiters.set(key, resolve));
  };
  const release = profile => new Promise((resolve, reject) => child.send({ kind: "release-cleanup", profile },
    error => error ? reject(error) : resolve()));
  t.after(async () => {
    let nativeConfirmed = !startedOpen;
    let cleanupError;
    try {
      if (child.connected) await release("all");
      for (const profile of opened) {
        await within(wait("cleanup-settled", profile), "fixture original native close", 30000);
      }
      nativeConfirmed = unconfirmedLaunches === 0 && (!startedOpen || opened.size > 0);
    } catch (error) { cleanupError = error; }
    // This fixture deliberately rejects production cleanup. Terminate only its
    // original Node ChildProcess handle, never rediscover or kill by a PID.
    // This is TEST teardown, not evidence of product recovery or normal exit.
    try {
      if (child.exitCode == null && child.signalCode == null) child.kill();
      await within(exited, "owned fault-fixture worker exit");
    } catch (error) { cleanupError ??= error; }
    if (!nativeConfirmed || cleanupError) {
      process.stdout.write(`# rejected-close-retained ${JSON.stringify({ root, nativeConfirmed })}\n`);
      throw cleanupError ?? new Error("native launch/cleanup unconfirmed; isolated root retained");
    }
    const rel = relative(base, root);
    assert.ok(rel.startsWith("grok-cu-rejected-close-") && !rel.includes("/") && !rel.includes("\\"));
    await rm(root, { recursive: true, force: true });
  });
  const port = await within(new Promise((resolve, reject) => {
    let output = "";
    child.once("error", reject);
    child.once("exit", () => reject(new Error(`fault worker exited before ready: ${startupError}`)));
    child.stdout.on("data", chunk => {
      output += chunk.toString("utf8");
      for (const line of output.split(/\r?\n/)) {
        try { const value = JSON.parse(line); if (Number.isInteger(value.port)) resolve(value.port); }
        catch { /* Wait for complete readiness JSON. */ }
      }
    });
  }), "owned fault worker startup");
  const post = async (path, body = {}) => {
    const reply = await fetch(`http://127.0.0.1:${port}${path}`, { method: "POST",
      headers: { authorization: `Bearer ${token}`, "x-grok-cu-host": "1", "content-type": "application/json" },
      body: JSON.stringify(body), signal: AbortSignal.timeout(8000) });
    return { status: reply.status, body: await reply.json() };
  };
  const identity = profile => ({ owner: profile, profile, runRevision: 1 });
  const open = async profile => {
    startedOpen = true;
    unconfirmedLaunches += 1;
    const reply = await post("/open", identity(profile));
    assert.equal(reply.status, 200);
    opened.add(profile);
    unconfirmedLaunches -= 1;
  };
  const body = identity("rejected-close");
  await open(body.profile);
  const first = post("/close", body);
  await within(wait("cleanup-dispatched", body.profile), "close dispatch");
  await release(body.profile);
  const failure = await first;
  assert.equal(failure.status, 500);
  assert.equal(failure.body.error.completion, "unknown");
  await within(wait("cleanup-closed", body.profile), "real native close event");
  const occupied = async () => {
    const reply = await post("/run-status", body);
    assert.equal(reply.status, 200);
    assert.equal(reply.body.idle, false);
    assert.equal(reply.body.activeOperations, 1);
    assert.ok((await post("/health")).body.openProfiles.includes(body.profile));
  };
  await occupied();
  const retried = await post("/close", body);
  assert.equal(retried.status, 500, "a close event cannot redeem the original failure");
  assert.equal(retried.body.error.completion, "unknown");
  assert.equal((await post("/pause-run", body)).status, 200);
  const resumed = await post("/resume-run", { ...body, nextRunRevision: 2 });
  assert.equal(resumed.status, 409);
  assert.equal(resumed.body.error.code, "run_not_quiescent");
  const cancelled = await post("/cancel-run", body);
  assert.equal(cancelled.status, 500, "cancel cannot redeem the original failure");
  assert.equal(cancelled.body.error.completion, "unknown");
  await within(wait("cleanup-settled", body.profile), "original native close completion", 30000);
  await occupied();
  assert.equal(seen.get(`cleanup-dispatched:${body.profile}`).calls, 1);

  // Uncertainty is exact-slot ownership, not a global ban on independent runs.
  const other = identity("independent-close");
  await open(other.profile);
  const independent = post("/close", other);
  await within(wait("cleanup-dispatched", other.profile), "independent close dispatch");
  await release(other.profile);
  const independentReply = await independent;
  if (independentReply.status !== 200) {
    assert.equal(independentReply.status, 409);
    assert.equal(independentReply.body.error.code, "run_cleanup_pending");
    assert.equal(independentReply.body.error.completion, "unknown");
  }
  await within(wait("cleanup-settled", other.profile), "independent physical close", 30000);
  assert.equal((await post("/cancel-run", other)).status, 200);
  assert.equal((await post("/run-status", other)).body.idle, true);
  await occupied();
  const shutdown = await post("/shutdown");
  assert.equal(shutdown.status, 500);
  assert.equal(shutdown.body.error.completion, "unknown");
  t.diagnostic("real native close event + injected rejection: owned=1, redispatch=0, independent run closes normally; no product recovery claimed");
});
