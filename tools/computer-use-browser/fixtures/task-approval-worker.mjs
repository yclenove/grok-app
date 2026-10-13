// Test-only source worker entry. Never bundled into a product runtime.
// Hold the ORIGINAL native launch/close; gates do not replay input or close.
import fs, { appendFileSync, existsSync } from "node:fs";
import { basename, join, toNamespacedPath } from "node:path";
import childProcess from "node:child_process";
import http from "node:http";
import { setTimeout as delay } from "node:timers/promises";
import { taskFixtureArtifactDirectory, taskFixtureRoot } from "./task-fixture-root.mjs";

const supplied = process.env.GROK_CU_BROWSER_PROFILE_ROOT || "";
const root = taskFixtureRoot(supplied);
const evidence = join(root, "fixture-events.ndjson");
const start = performance.now();
const record = (phase, details = {}) => appendFileSync(evidence,
  JSON.stringify({ phase, elapsedMs: Math.round(performance.now() - start), ...details }) + "\n");
const gate = async phase => {
  if (!existsSync(join(root, `hold-${phase}`))) return;
  const deadline = performance.now() + 30000;
  while (!existsSync(join(root, `release-${phase}`))) {
    if (performance.now() >= deadline) throw new Error(`owned ${phase} gate was not released`);
    await delay(10);
  }
};

// Fixed route counters only: never record headers, body, page URL or token.
const routes = new Set(["/open", "/scope-identity", "/observe", "/act", "/goto",
  "/tabs", "/new-tab", "/upload", "/download", "/cancel-run"]);
const createServer = http.createServer;
let routeSequence = 0;
http.createServer = function (...args) {
  const server = createServer.apply(this, args);
  server.prependListener("request", (req, res) => {
    const path = new URL(req.url || "/", "http://127.0.0.1").pathname;
    if (routes.has(path)) {
      const request = ++routeSequence;
      record("route", { path, request });
      res.once("finish", () => record("route-reply", { route: path, request, status: res.statusCode }));
    }
  });
  return server;
};

// Observe only this fixture's writable Chromium child and Playwright's
// artifact-directory teardown. Forward the original arguments/results; never
// record command lines, process IDs, paths, headers or exception messages.
const originalSpawn = childProcess.spawn;
const fixtureChrome = toNamespacedPath(join(root, ".chrome-exec", "chrome-win", "chrome.exe"));
let nativeSequence = 0;
childProcess.spawn = function (command, ...args) {
  const child = originalSpawn.call(this, command, ...args);
  if (typeof command === "string" && toNamespacedPath(command) === fixtureChrome) {
    const nativeOwner = ++nativeSequence;
    record("native-process-start", { nativeOwner });
    child.once("exit", code => record("native-process-exit", {
      nativeOwner, status: Number.isInteger(code) ? code : null,
    }));
  }
  return child;
};
const originalRemove = fs.promises.rm;
let cleanupSequence = 0;
fs.promises.rm = async function (directory, ...args) {
  if (!taskFixtureArtifactDirectory(directory)) {
    return originalRemove.call(this, directory, ...args);
  }
  const cleanup = ++cleanupSequence;
  const started = performance.now();
  record("native-artifact-cleanup-start", { cleanup });
  let fulfilled = false;
  try {
    const result = await originalRemove.call(this, directory, ...args);
    fulfilled = true;
    return result;
  } finally {
    record("native-artifact-cleanup-end", {
      cleanup, fulfilled, durationMs: Math.round(performance.now() - started),
    });
  }
};

// Observe the exact dependency teardown; do not replace taskkill, alter its
// arguments/deadline or log PID/command/stdout/env/token/page information.
const originalSpawnSync = childProcess.spawnSync;
childProcess.spawnSync = function (command, ...args) {
  if (typeof command !== "string" || !/^taskkill \/pid \d+ \/T \/F$/.test(command)) {
    return originalSpawnSync.call(this, command, ...args);
  }
  const started = performance.now();
  record("native-force-kill-start");
  let result;
  try { result = originalSpawnSync.call(this, command, ...args); return result; }
  finally { record("native-force-kill-end", { durationMs: Math.round(performance.now() - started),
    status: Number.isInteger(result?.status) ? result.status : null }); }
};

// Load after observers: Playwright's CJS launcher captures spawn/spawnSync at
// module evaluation. A static import would silently miss those native events.
const { default: playwright } = await import("playwright-core");
const launch = playwright.chromium.launchPersistentContext.bind(playwright.chromium);
playwright.chromium.launchPersistentContext = async (directory, options) => {
  const context = await launch(directory, options);
  const profile = basename(directory);
  record("launch-native-ready", { profile });
  const close = context.close.bind(context);
  let closeCalls = 0;
  context.once("close", () => record("close-event", { profile }));
  context.close = async (...args) => {
    record("close-dispatched", { profile, calls: ++closeCalls });
    await gate("close");
    const started = performance.now();
    try { await close(...args); }
    catch (error) { record("close-rejected", { profile }); throw error; }
    record("close-native-settled", { profile, durationMs: Math.round(performance.now() - started) });
  };
  await gate("launch");
  record("launch-returned", { profile });
  return context;
};
record("fixture-ready");
await import("../server.mjs");
