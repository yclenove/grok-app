// Native test preload only; never copied into the runtime pack. The production
// server and ContextClose run unchanged. IPC gates the original Playwright
// close call so tests can inspect a genuinely pending owned browser context.
import { basename } from "node:path";
import { createRequire } from "node:module";
import { pathToFileURL } from "node:url";
import childProcess from "node:child_process";

if (!process.send) throw new Error("cleanup fixture requires a private parent IPC channel");
// Diagnostics only: retain the exact dependency call and its original result.
// No command, PID, output, environment or credential is sent to the log.
const spawnSync = childProcess.spawnSync;
childProcess.spawnSync = function (command, ...args) {
  if (typeof command !== "string" || !/^taskkill \/pid \d+ \/T \/F$/.test(command)) {
    return spawnSync.call(this, command, ...args);
  }
  const started = performance.now();
  process.send({ kind: "native-force-kill-start" });
  let result;
  let failure;
  try { result = spawnSync.call(this, command, ...args); return result; }
  catch (error) { failure = error; throw error; }
  finally {
    // Measure inside the worker: parent IPC delivery can itself be delayed by
    // a blocked event loop. Preserve the dependency's return/throw unchanged.
    const code = result?.error?.code ?? failure?.code;
    process.send({ kind: "native-force-kill-end", durationMs: Math.round(performance.now() - started),
      status: Number.isInteger(result?.status) ? result.status : null,
      signal: typeof result?.signal === "string" && /^SIG[A-Z0-9]+$/.test(result.signal) ? result.signal : null,
      errorCode: typeof code === "string" && /^[A-Z0-9_]+$/.test(code) ? code : null });
  }
};
// Patch the dependency of the explicitly selected worker, not an unrelated
// source installation when the test is exercising a generated runtime pack.
const requireWorker = createRequire(pathToFileURL(process.argv[1]));
const { default: playwright } = await import(pathToFileURL(requireWorker.resolve("playwright-core")).href);
const { chromium } = playwright;
const gates = new Map();
const operations = new Map();
const { RunOperations } = await import(new URL("./run-operations.mjs", pathToFileURL(process.argv[1])).href);
const begin = RunOperations.prototype.begin;
RunOperations.prototype.begin = function (owner, ...args) {
  const operation = begin.call(this, owner, ...args);
  if (owner !== "pending-shutdown") return operation;
  const finish = operation.finish;
  operation.finish = () => {
    operations.set(owner, finish);
    process.send({ kind: "cleanup-operation-held", profile: owner });
  };
  return operation;
};
const launch = chromium.launchPersistentContext.bind(chromium);
chromium.launchPersistentContext = async (dir, options) => {
  const context = await launch(dir, options);
  const close = context.close.bind(context);
  let release;
  const pending = new Promise(resolve => { release = resolve; });
  const profile = basename(dir);
  gates.set(profile, release);
  const closed = new Promise(resolve => context.once("close", resolve));
  let calls = 0;
  context.close = async (...args) => {
    process.send({ kind: "cleanup-dispatched", profile, calls: ++calls });
    await pending;
    const physical = close(...args);
    // The failure fixture uses a REAL native close event, never a fabricated
    // EventEmitter event. Keep observing the original physical promise too.
    const observed = physical.then(() => {
      gates.delete(profile);
      process.send({ kind: "cleanup-settled", profile });
    });
    if (profile === "rejected-close") {
      void observed.catch(() => process.send({ kind: "cleanup-physical-failed", profile }));
      await closed;
      process.send({ kind: "cleanup-rejected", profile });
      throw new Error("fixture physical cleanup rejection");
    }
    await observed;
  };
  context.once("close", () => {
    process.send({ kind: "cleanup-closed", profile });
  });
  return context;
};
process.on("message", message => {
  if (message?.kind === "release-operation") {
    for (const [owner, finish] of operations) {
      if (message.profile === "all" || message.profile === owner) {
        operations.delete(owner);
        finish();
      }
    }
    return;
  }
  if (message?.kind !== "release-cleanup") return;
  if (message.profile === "all") for (const release of gates.values()) release();
  else gates.get(message.profile)?.();
});
