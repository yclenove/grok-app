import test from "node:test";
import assert from "node:assert/strict";
import { randomBytes } from "node:crypto";
import { spawn } from "node:child_process";
import { existsSync, mkdtempSync, realpathSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, relative, resolve, sep } from "node:path";
import { request } from "node:http";
import { connect } from "node:net";
import { fileURLToPath } from "node:url";
import { childEnv } from "./child-env.mjs";

const ROOT = fileURLToPath(new URL(".", import.meta.url));

function post(port, path, body, { token, host = false, raw } = {}) {
  return new Promise((resolve, reject) => {
    const data = raw || Buffer.from(JSON.stringify(body || {}), "utf8");
    const headers = {
      "content-type": "application/json",
      "content-length": data.length,
    };
    if (token) headers.authorization = `Bearer ${token}`;
    if (host) headers["x-grok-cu-host"] = "1";
    const req = request(
      { hostname: "127.0.0.1", port, path, method: "POST", headers },
      (res) => {
        const chunks = [];
        res.on("data", (chunk) => chunks.push(chunk));
        res.on("end", () => {
          try {
            resolve({
              status: res.statusCode,
              body: JSON.parse(Buffer.concat(chunks).toString("utf8")),
            });
          } catch (error) {
            reject(error);
          }
        });
      },
    );
    req.on("error", error => reject(new Error(`POST ${path} failed: ${error.code || "transport error"}`)));
    req.setTimeout(5000, () => req.destroy(new Error("worker response timeout")));
    req.end(data);
  });
}

function assertContract(response, status, completion = "not_started") {
  assert.equal(response.status, status);
  assert.equal(response.body.ok, false);
  assert.equal(response.body.error.status, status);
  assert.match(response.body.error.code, /^[a-z0-9_]{1,64}$/);
  assert.equal(response.body.error.completion, completion);
  assert.equal(typeof response.body.error.message, "string");
  assert.ok(response.body.error.message.length > 0);
}

async function startWorker(t) {
  const token = randomBytes(32).toString("hex");
  const root = mkdtempSync(join(tmpdir(), "grok-cu-error-contract-"));
  const child = spawn(process.execPath, [process.env.GROK_CU_TEST_WORKER || join(ROOT, "server.mjs")], {
    cwd: ROOT,
    stdio: ["ignore", "pipe", "pipe"],
    env: childEnv({
      GROK_CU_BROWSER_TOKEN: token,
      GROK_CU_BROWSER_PORT: "0",
      GROK_CU_BROWSER_PROFILE_ROOT: join(root, "profiles"),
      GROK_CU_BROWSER_STAGING_ROOT: join(root, "staging"),
    }),
  });
  // Register cleanup before awaiting startup, and never remove a live worker's root.
  t.after(async () => {
    if (child.exitCode == null && child.signalCode == null) {
      await new Promise((resolveExit, reject) => {
        const timer = setTimeout(() => reject(new Error("worker cleanup timeout")), 5000);
        child.once("exit", () => { clearTimeout(timer); resolveExit(); });
        child.kill();
      });
    }
    const base = realpathSync(tmpdir());
    assert.ok(`${resolve(root)}${sep}`.toLowerCase().startsWith(`${base}${sep}`.toLowerCase()));
    assert.ok(relative(base, resolve(root)).startsWith("grok-cu-error-contract-"));
    rmSync(root, { recursive: true, force: true });
  });
  let output = "";
  const port = await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("worker port timeout")), 10_000);
    const inspect = (chunk) => {
      output += chunk.toString("utf8");
      for (const line of output.split(/\r?\n/)) {
        try {
          const parsed = JSON.parse(line);
          if (parsed.port) {
            clearTimeout(timer);
            resolve(parsed.port);
            return;
          }
        } catch {
          // Wait for a complete JSON line.
        }
      }
    };
    child.stdout.on("data", inspect);
    child.stderr.on("data", chunk => {
      if (chunk.toString("utf8").includes("triggerUncaughtException")) {
        t.diagnostic("worker stderr reported an uncaught exception");
      }
      inspect(chunk);
    });
    child.once("error", reject);
    child.once("exit", (code) => reject(new Error(`worker exited ${code}`)));
  });
  return { port, token, root, child };
}

function abortBody(port, token, path, body) {
  return new Promise((resolveClose, reject) => {
    const socket = connect({ host: "127.0.0.1", port });
    let continued = false;
    let response = "";
    const timer = setTimeout(() => {
      socket.destroy();
      reject(new Error("partial request timeout"));
    }, 5000);
    socket.once("connect", () => socket.write([
      `POST ${path} HTTP/1.1`, `Host: 127.0.0.1:${port}`,
      `Authorization: Bearer ${token}`, "x-grok-cu-host: 1",
      "Content-Type: application/json", "Content-Length: 4096",
      "Expect: 100-continue", "Connection: close", "", "",
    ].join("\r\n")));
    socket.on("data", chunk => {
      response += chunk.toString("ascii");
      if (!continued && response.includes("100 Continue\r\n\r\n")) {
        continued = true;
        // Valid JSON but incomplete HTTP body: no route may consume it.
        socket.end(JSON.stringify(body));
      }
    });
    socket.once("error", error => {
      // A reset is allowed only for the deliberately incomplete connection.
      // The following independent health/status requests must still succeed.
      if (!continued || error.code !== "ECONNRESET") reject(error);
    });
    socket.once("close", () => {
      clearTimeout(timer);
      if (!continued) reject(new Error("worker did not accept request headers"));
      else resolveClose();
    });
  });
}

test("aborted request bodies cannot terminate worker or admit browser/lifecycle work", async (t) => {
  const { port, token, root, child } = await startWorker(t);
  for (const path of ["/open", "/pause-run", "/cancel-run", "/shutdown"]) {
    await abortBody(port, token, path, { owner: "run-partial", profile: "partial-profile", runRevision: 1 });
    const health = await post(port, "/health", {}, { token });
    assert.equal(health.status, 200);
    assert.deepEqual(health.body.openProfiles, []);
    assert.equal(child.exitCode, null);
    const status = await post(port, "/run-status", { owner: "run-partial", runRevision: 1 }, { token, host: true });
    assertContract(status, 404);
    assert.equal(status.body.error.code, "run_unknown");
    assert.equal(existsSync(join(root, "profiles", "partial-profile")), false);
  }
});

test("server returns one typed error envelope for HTTP failures", async (t) => {
  const { port, token } = await startWorker(t);

  assertContract(await post(port, "/health", {}), 401);
  const malformed = await post(port, "/shutdown", {}, { token, raw: Buffer.from("{broken") });
  assertContract(malformed, 400);
  assert.equal(malformed.body.error.code, "invalid_json");
  assertContract(await post(port, "/missing", {}, { token }), 404);
  assertContract(await post(port, "/cancel-run", { owner: "run-a" }, { token }), 403);
  assertContract(await post(port, "/open", { profile: "..", owner: "run-a" }, { token }), 400);

  const cancelled = await post(
    port,
    "/cancel-run",
    { owner: "run-cancelled" },
    { token, host: true },
  );
  assert.equal(cancelled.status, 200);
  assertContract(
    await post(
      port,
      "/open",
      { profile: "cancelled-profile", owner: "run-cancelled" },
      { token },
    ),
    409,
  );

  assertContract(
    await post(port, "/health", {}, { token, raw: Buffer.alloc(1024 * 1024 + 1, 0x20) }),
    413,
  );
});
