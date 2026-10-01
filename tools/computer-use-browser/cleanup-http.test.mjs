import test from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { randomBytes } from "node:crypto";
import { mkdtemp, realpath, rm } from "node:fs/promises";
import { request } from "node:http";
import { tmpdir } from "node:os";
import { join, relative } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { childEnv } from "./child-env.mjs";
import { finishOwnedCleanup } from "./fixtures/cleanup-verified.mjs";

const ROOT = fileURLToPath(new URL(".", import.meta.url));

function post(port, token, path, body = {}) {
  return new Promise((resolve, reject) => {
    const data = Buffer.from(JSON.stringify(body));
    const req = request({ hostname: "127.0.0.1", port, method: "POST", path,
      headers: { authorization: `Bearer ${token}`, "x-grok-cu-host": "1",
        "content-type": "application/json", "content-length": data.length } }, res => {
      let content = "";
      res.setEncoding("utf8");
      res.on("data", chunk => { content += chunk; });
      res.on("end", () => {
        try { resolve({ status: res.statusCode, body: JSON.parse(content) }); }
        catch { reject(new Error("invalid cleanup response")); }
      });
    });
    req.setTimeout(8000, () => req.destroy(new Error(`${path} response deadline exceeded`)));
    req.on("error", reject);
    req.end(data);
  });
}

async function within(pending, label, ms = 10000) {
  let timer;
  try {
    return await Promise.race([pending, new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error(`${label} deadline exceeded`)), ms);
    })]);
  } finally { clearTimeout(timer); }
}

function assertPending(reply) {
  assert.equal(reply.status, 409);
  assert.deepEqual(reply.body.error, { status: 409, code: "run_cleanup_pending",
    completion: "unknown", message: "browser cleanup is still pending" });
}

test("production cleanup HTTP replies stay bounded without releasing native contexts", { timeout: 150000 }, async t => {
  assert.ok(process.env.GROK_CU_CHROME, "provide an explicit isolated-test Chrome executable");
  const base = await realpath(tmpdir());
  const root = await mkdtemp(join(base, "grok-cu-cleanup-http-"));
  const token = randomBytes(32).toString("hex");
  const child = spawn(process.execPath, ["--import", pathToFileURL(join(ROOT, "fixtures/cleanup-delay.preload.mjs")).href,
    process.env.GROK_CU_TEST_WORKER || join(ROOT, "server.mjs")], { cwd: ROOT, windowsHide: true, stdio: ["ignore", "pipe", "pipe", "ipc"],
    env: { ...childEnv({ GROK_CU_BROWSER_TOKEN: token, GROK_CU_BROWSER_PORT: "0",
      GROK_CU_BROWSER_PROFILE_ROOT: join(root, "profiles"), GROK_CU_BROWSER_STAGING_ROOT: join(root, "staging") }),
      DEBUG: "pw:browser" } });
  const exited = new Promise(resolve => child.once("exit", (code, signal) => resolve({ code, signal })));
  const closeCalls = new Map();
  const closed = new Set();
  const settled = new Set();
  const closeWaiters = new Map();
  const started = performance.now();
  const diagnostic = (phase, durationMs) => process.stdout.write(
    `# cleanup phase=${phase} elapsedMs=${Math.round(performance.now() - started)}` +
    (Number.isFinite(durationMs) && durationMs >= 0 ? ` nativeDurationMs=${durationMs}` : "") + "\n");
  child.on("message", message => {
    if (message?.kind === "cleanup-dispatched") closeCalls.set(message.profile, message.calls);
    if (message?.kind === "cleanup-closed") closed.add(message.profile);
    if (message?.kind === "cleanup-settled") {
      settled.add(message.profile);
      closeWaiters.get(message.profile)?.();
    }
    if (["cleanup-dispatched", "cleanup-closed", "cleanup-settled", "native-force-kill-start", "native-force-kill-end"].includes(message?.kind)) diagnostic(message.kind, message.durationMs);
    if (message?.kind === "native-force-kill-end") {
      process.stdout.write(`# native-force-kill-result ${JSON.stringify({ status: message.status,
        signal: message.signal, errorCode: message.errorCode })}\n`);
    }
  });
  let startupError = "unknown";
  child.stderr.on("data", chunk => {
    // Capture only a Node error classification, never browser/page/token data.
    const code = chunk.toString("utf8").match(/\bERR_[A-Z_]+\b/);
    if (code) startupError = code[0];
    for (const phase of ["gracefully close start", "gracefully close end", "process did exit", "will force kill"]) {
      if (chunk.toString("utf8").includes(`<${phase}`)) diagnostic(phase.replaceAll(" ", "-"));
    }
  });
  let port;
  let clean = false;
  t.after(async () => {
    try { await finishOwnedCleanup({
      shutdown: async () => {
        if (clean) return;
        if (child.connected) {
          for (const kind of ["release-cleanup", "release-operation"]) {
            await new Promise((resolve, reject) => child.send({ kind, profile: "all" },
              error => error ? reject(error) : resolve()));
          }
        }
        assert.ok(port, `worker never became ready; retain isolated root ${root}`);
        let reply = await post(port, token, "/shutdown");
        if (reply.status === 409 && reply.body.error?.code === "run_cleanup_pending") {
          await Promise.all([...closeCalls.keys()].map(async profile => {
            if (!settled.has(profile)) await within(new Promise(resolve => closeWaiters.set(profile, resolve)),
              "retained physical cleanup", 30000);
          }));
          reply = await post(port, token, "/shutdown");
        }
        assert.equal(reply.status, 200, `shutdown unconfirmed; retain isolated root ${root}`);
      },
      workerExit: async () => {
        try {
          assert.deepEqual(await within(exited, "owned worker cleanup"), { code: 0, signal: null });
        } catch (error) {
          // Only the fixture's original child handle. Forced teardown is still
          // failure; it is not browser-exit evidence or permission to erase it.
          if (child.exitCode == null && child.signalCode == null) {
            diagnostic("forced-owned-worker-cleanup");
            child.kill();
          }
          await within(exited, "forced owned worker cleanup", 5000).catch(() => {});
          throw error;
        }
      },
      fixtureClose: async () => {},
      browserExit: async () => {
        for (const profile of closeCalls.keys()) {
          assert.ok(closed.has(profile) && settled.has(profile),
            `physical browser cleanup remains unconfirmed; retain ${root}`);
        }
      },
      removeProfile: async () => {
        const rel = relative(base, root);
        assert.ok(rel.startsWith("grok-cu-cleanup-http-") && !rel.includes("/") && !rel.includes("\\"));
        await rm(root, { recursive: true, force: true });
      },
    }); } catch (error) {
      // Node can show only the primary test error when after hooks also fail.
      // Preserve this owned root without dumping dependency output/secrets.
      process.stdout.write(`# cleanup-retained ${JSON.stringify({ root,
        failures: error instanceof AggregateError ? error.errors.length : 1 })}\n`);
      throw error;
    }
  });
  port = await within(new Promise((resolve, reject) => {
    let output = "";
    child.once("error", reject);
    child.once("exit", () => reject(new Error(`cleanup worker exited before ready: ${startupError}`)));
    child.stdout.on("data", chunk => {
      output += chunk.toString("utf8");
      for (const line of output.split(/\r?\n/)) {
        try { const value = JSON.parse(line); if (Number.isInteger(value.port)) resolve(value.port); }
        catch { /* Readiness output may be split across chunks. */ }
      }
    });
  }), "owned worker startup");
  const send = (path, body) => post(port, token, path, body);
  const release = async profile => {
    // A close event is insufficient: Playwright may still await process exit or
    // artifact cleanup. Only its original promise settlement confirms the gate.
    const done = settled.has(profile) ? Promise.resolve() : new Promise(resolve => closeWaiters.set(profile, resolve));
    child.send({ kind: "release-cleanup", profile });
    // This is a late-reconciliation test budget, NOT a product response budget
    // or a claim of fast native exit. The phase timings retain slow exits.
    await within(done, "physical context close", 30000);
  };

  for (const path of ["/close", "/cancel-run", "/shutdown"]) {
    const profile = `pending-${path.slice(1)}`;
    const body = { owner: profile, profile, runRevision: 1 };
    assert.equal((await send("/open", body)).status, 200);
    const start = performance.now();
    const responses = path === "/cancel-run"
      ? await Promise.all([send(path, body), send(path, body)]) : [await send(path, body)];
    for (const response of responses) assertPending(response);
    assert.ok(performance.now() - start < 7500, "route replied before the caller's transport timeout");
    assert.equal(closeCalls.get(profile), 1, "concurrent observers cannot redispatch native close");
    assert.equal(closed.has(profile), false);
    const status = await send("/run-status", body);
    assert.equal(status.status, 200);
    assert.equal(status.body.idle, false);
    assert.equal(status.body.activeOperations, path === "/shutdown" ? 2 : 1);
    assert.ok((await send("/health")).body.openProfiles.includes(profile));
    assert.equal((await send("/open", { ...body, owner: "different-owner" })).status, 409);
    await release(profile);
    if (path === "/shutdown") {
      // The native browser has closed, but an admitted operation has not yet
      // physically settled. Expiring another HTTP observer must not abandon
      // the original cleanup or require a caller to keep it alive.
      const stillPending = await send("/shutdown", body);
      assert.equal(stillPending.status, 409);
      assert.equal(stillPending.body.error?.code, "run_cleanup_pending");
      assert.equal(stillPending.body.error?.completion, "unknown");
      assert.equal((await send("/run-status", body)).body.activeOperations, 1);
      child.send({ kind: "release-operation", profile });
      const deadline = performance.now() + 5000;
      let health;
      do {
        health = await send("/health");
        if (!health.body.openProfiles.includes(profile)) break;
        await new Promise(resolve => setTimeout(resolve, 25));
      } while (performance.now() < deadline);
      assert.equal(health.body.openProfiles.includes(profile), false,
        "original shutdown must complete when the late operation settles, without another shutdown request");
    }
    // This explicit lifecycle retry observes the same physical close; it is
    // not a retry of an input/action. Shutdown's late completion keeps HTTP up.
    const reconciled = path === "/close" ? await send("/cancel-run", body) : await send(path, body);
    assert.equal(reconciled.status, 200, JSON.stringify(reconciled.body.error));
    assert.equal(closeCalls.get(profile), 1);
    if (path !== "/shutdown") {
      const settled = await send("/run-status", body);
      assert.equal(settled.body.idle, true);
      assert.equal(settled.body.activeOperations, 0);
      assert.ok(!(await send("/health")).body.openProfiles.includes(profile));
    }
    t.diagnostic(`${path}: pending=unknown, occupied=1, native-close=1, reconciled=confirmed`);
  }
  assert.deepEqual(await within(exited, "confirmed worker exit"), { code: 0, signal: null });
  clean = true;
});
