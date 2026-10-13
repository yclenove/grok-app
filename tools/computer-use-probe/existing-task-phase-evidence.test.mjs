import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdtemp, readFile, realpath, rmdir, unlink } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { JointPhaseEvidence } from "./existing-task-phase-evidence.mjs";

const failure = () => ({ stage: "real-unshare-reshare-original-picker", detail: "share-host-ready",
  errorClass: "Error", errorCode: "other", errorFamily: "postcondition-timeout",
  checks: ["first", "second", "third"], startupEvidence: { phase: "complete", elapsedMs: 617,
    initialWorkerCount: 0, workerEvents: 1, sourceWorkerMatched: true },
  connectionEvidence: { detached: true, workerStopped: true, pairingConfirmed: 1,
    disconnectRequests: 1, contextsCleared: 1,
    http: [{ route: "/cu/extension-status", status: 403, outcome: "response", elapsedMs: 619 }] } });

async function ownedDirectory(check) {
  const directory = await realpath(await mkdtemp(path.join(os.tmpdir(), "grok-cu-phase-evidence-")));
  let passed = false;
  try { await check(directory); passed = true; }
  finally {
    if (passed) {
      for (const name of ["joint-phases.ndjson", "joint-primary-failure.json"]) {
        try { await unlink(path.join(directory, name)); }
        catch (error) { if (error.code !== "ENOENT") throw error; }
      }
      await rmdir(directory);
    }
  }
}

test("primary failure survives the exact diagnostic child being killed before auxiliary work settles", { timeout: 15000 }, async () => {
  await ownedDirectory(async directory => {
    const moduleUrl = new URL("./existing-task-phase-evidence.mjs", import.meta.url).href;
    const child = spawn(process.execPath, ["--input-type=module", "-e", `
      import { JointPhaseEvidence } from ${JSON.stringify(moduleUrl)};
      const evidence = new JointPhaseEvidence(process.argv[1]);
      evidence.note("share-host-ready");
      evidence.primary(${JSON.stringify(failure())});
      process.stdout.write("persisted\\n");
      // Model the later auxiliary browser query that never settles.
      setInterval(() => {}, 1000);
      await new Promise(() => {});
    `, directory], { windowsHide: true, stdio: ["ignore", "pipe", "ignore"] });
    const exited = once(child, "exit");
    try {
      const [data] = await Promise.race([once(child.stdout, "data"), exited.then(() => {
        throw new Error("owned diagnostic child exited before primary persistence");
      })]);
      assert.equal(data.toString(), "persisted\n");
      assert.equal(child.exitCode, null);
      assert.equal(child.kill(), true);
      await exited;
      const primary = JSON.parse(await readFile(path.join(directory, "joint-primary-failure.json"), "utf8"));
      assert.equal(primary.detail, "share-host-ready");
      assert.equal(primary.checks, 3);
      assert.equal(primary.startup.sourceWorkerMatched, true);
      assert.equal(primary.connection.workerStopped, true);
      assert.equal(primary.connection.http[0].status, 403);
      const rows = (await readFile(path.join(directory, "joint-phases.ndjson"), "utf8")).trim().split("\n");
      assert.equal(JSON.parse(rows.at(-1)).phase, "share-host-ready");
    } finally {
      if (child.exitCode === null && child.signalCode === null) child.kill();
      await exited;
    }
  });
});

test("diagnostics stay bounded, exclude arbitrary payloads, and preserve the first failure", async () => {
  await ownedDirectory(async directory => {
    const evidence = new JointPhaseEvidence(directory);
    try {
      for (let index = 0; index < 150; index++) evidence.note("share-host-ready");
      evidence.note("private-payload-that-is-not-a-phase");
      const first = failure();
      first.unexpected = "private-payload-that-is-not-a-phase";
      first.connectionEvidence.http[0].headers = first.unexpected;
      first.connectionEvidence.unexpected = first.unexpected;
      first.nativeEvaluation = { kind: "exception", exceptionClass: "TypeError", description: first.unexpected };
      evidence.primary(first);
      const filename = path.join(directory, "joint-primary-failure.json");
      const original = await readFile(filename, "utf8");
      assert.throws(() => evidence.primary(failure()), { code: "EEXIST" });
      assert.equal(await readFile(filename, "utf8"), original);
      assert.equal(original.includes(first.unexpected), false);
      const primary = JSON.parse(original);
      assert.equal(primary.phaseOverflow, true);
      assert.equal(primary.lastPhase, "other");
      assert.deepEqual(primary.nativeEvaluation, { kind: "exception", exceptionClass: "TypeError" });
      const rows = (await readFile(path.join(directory, "joint-phases.ndjson"), "utf8")).trim().split("\n");
      assert.equal(rows.length, 128);
      assert.equal(rows.some(row => row.includes(first.unexpected)), false);
    } finally { evidence.close(); }
  });
});
