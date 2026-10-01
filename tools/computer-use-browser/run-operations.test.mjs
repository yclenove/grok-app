import test from "node:test";
import assert from "node:assert/strict";
import { RunOperations } from "./run-operations.mjs";

test("physical shutdown keeps waiting after an HTTP observer's idle deadline", async () => {
  const runs = new RunOperations();
  const operation = runs.begin("late-owner", 1);
  runs.stopAll();
  let settled = false;
  const cleanup = runs.whenAllIdle().then(() => { settled = true; });
  assert.equal(await runs.waitAllIdle(10), false);
  assert.equal(settled, false);
  assert.equal(runs.status("late-owner", 1).idle, false);
  operation.finish();
  await cleanup;
  assert.equal(settled, true);
  assert.equal(runs.status("late-owner", 1).idle, true);
});

test("late run cleanup needs both request settlement and native resource retirement", async () => {
  const runs = new RunOperations();
  const request = runs.begin("late-owner", 1);
  const resource = runs.holdResourceCleanup("late-owner", {});
  runs.stop("late-owner");
  let settled = false;
  const cleanup = runs.whenIdle("late-owner").then(() => { settled = true; });
  request.finish();
  await Promise.resolve();
  assert.equal(settled, false);
  resource.finish();
  await cleanup;
  assert.equal(runs.status("late-owner", 1).activeOperations, 0);
});

test("whole-worker late cleanup includes every run admitted before the shutdown fence", async () => {
  const runs = new RunOperations();
  const first = runs.begin("first", 1);
  const second = runs.begin("second", 1);
  runs.stopAll();
  let settled = false;
  const cleanup = runs.whenAllIdle().then(() => { settled = true; });
  first.finish();
  await Promise.resolve();
  assert.equal(settled, false);
  assert.throws(() => runs.begin("replacement", 1));
  second.finish();
  await cleanup;
  assert.equal(settled, true);
});

function rejects(code, operation) {
  assert.throws(operation, error => error.workerCode === code);
}

test("physical idle completion handles an empty worker and already-settled owners", async () => {
  const empty = new RunOperations();
  empty.stopAll();
  await empty.whenAllIdle();
  const runs = new RunOperations();
  runs.begin("finished", 1).finish();
  runs.stop("finished");
  await runs.whenIdle("finished");
  await runs.whenAllIdle();
  assert.equal(runs.status("finished", 1).idle, true);
  rejects("run_unknown", () => runs.whenIdle("unknown"));
});

test("expired readback observers cannot cancel physical cleanup observers", async () => {
  const runs = new RunOperations();
  const operation = runs.begin("owner", 1);
  runs.stop("owner");
  let completions = 0;
  const retained = Array.from({ length: 8 }, () => runs.whenIdle("owner").then(() => { completions += 1; }));
  const readbacks = await Promise.all(Array.from({ length: 8 }, () => runs.waitIdle("owner", 5)));
  assert.ok(readbacks.every(value => value === false));
  assert.equal(completions, 0);
  operation.finish();
  await Promise.all(retained);
  operation.finish();
  assert.equal(completions, 8);
  assert.equal(runs.status("owner", 1).activeOperations, 0);
});

test("pause cancels authority immediately but keeps physical work counted until finish", async () => {
  const runs = new RunOperations();
  const first = runs.begin("owner", 1), second = runs.begin("owner", 1);
  const other = runs.begin("other", 1);
  assert.deepEqual(runs.pause("owner", 1), { runRevision: 1, phase: "paused", activeOperations: 2, idle: false });
  assert.equal(first.signal.aborted, true);
  assert.equal(second.signal.aborted, true);
  assert.equal(other.signal.aborted, false);
  rejects("run_cancelled", () => first.check());
  rejects("run_fenced", () => runs.begin("owner", 1));
  rejects("run_not_quiescent", () => runs.resume("owner", 1, 2));
  assert.equal(await runs.waitIdle("owner", 5), false, "timeout is not idle");
  first.finish(); first.finish();
  assert.equal(runs.status("owner", 1).activeOperations, 1, "idempotent finish");
  let idle = false;
  const waiting = runs.waitIdle("owner", 1000).then(value => { idle = value; });
  await Promise.resolve();
  assert.equal(idle, false);
  second.finish();
  await waiting;
  assert.equal(idle, true);
  assert.equal(runs.resume("owner", 1, 2).phase, "running");
  rejects("stale_run_revision", () => runs.begin("owner", 1));
  rejects("stale_run_revision", () => runs.begin("owner"));
  rejects("stale_run_revision", () => runs.pause("owner", 1));
  const fresh = runs.begin("owner", 2);
  first.finish();
  assert.equal(runs.status("owner", 2).activeOperations, 1, "late old finish cannot release new work");
  assert.equal(fresh.signal.aborted, false);
  fresh.finish(); other.finish();
});

test("fence arriving before open prevents late legacy and old explicit requests", () => {
  const runs = new RunOperations();
  assert.equal(runs.pause("owner", 1).idle, true);
  rejects("stale_run_revision", () => runs.begin("owner"));
  rejects("run_fenced", () => runs.begin("owner", 1));
  rejects("invalid_run_revision", () => runs.resume("owner", 1, 3));
  runs.resume("owner", 1, 2);
  rejects("stale_run_revision", () => runs.begin("owner", 1));
  rejects("stale_run_revision", () => runs.resume("owner", 1, 2));
  assert.equal(runs.begin("owner", 2).signal.aborted, false);
});

test("terminal owners cannot be resurrected and bounds never evict tombstones", () => {
  const runs = new RunOperations({ maxRuns: 2, maxActive: 1 });
  rejects("invalid_owner", () => runs.begin(42));
  rejects("invalid_owner", () => runs.begin("../unsafe"));
  rejects("stale_run_revision", () => runs.begin("invalid", 2));
  rejects("stale_run_revision", () => runs.pause("invalid", 2));
  const first = runs.begin("owner");
  rejects("operation_capacity", () => runs.begin("owner"));
  runs.stop("owner");
  assert.equal(first.signal.aborted, true);
  assert.equal(runs.status("owner", 1).idle, false);
  first.finish();
  rejects("run_fenced", () => runs.begin("owner", 1));
  rejects("run_fenced", () => runs.pause("owner", 1));
  rejects("run_not_quiescent", () => runs.resume("owner", 1, 2));
  runs.stop("second");
  rejects("run_capacity", () => runs.begin("third"));
  rejects("run_fenced", () => runs.begin("owner", 1));
});

test("cleanup remains counted during pause and shutdown fences future owners", async () => {
  const runs = new RunOperations();
  runs.begin("owner", 1).finish();
  runs.pause("owner", 1);
  const cleanup = runs.holdCleanup("owner");
  rejects("run_not_quiescent", () => runs.resume("owner", 1, 2));
  runs.stopAll();
  rejects("run_fenced", () => runs.begin("new-owner", 1));
  rejects("run_fenced", () => runs.resume("owner", 1, 2));
  assert.equal(await runs.waitAllIdle(5), false);
  cleanup.finish();
  assert.equal(await runs.waitAllIdle(5), true);
});

test("resource cleanup is identity-bound, deduplicated and independent of request saturation", async () => {
  const runs = new RunOperations({ maxActive: 1 });
  const request = runs.begin("owner", 1);
  const resource = {};
  const first = runs.holdResourceCleanup("owner", resource);
  assert.equal(runs.holdResourceCleanup("owner", resource), first);
  assert.equal(runs.status("owner", 1).activeOperations, 2);
  request.finish();
  assert.equal(await runs.waitIdle("owner", 1), false);
  first.finish();
  const second = runs.holdResourceCleanup("owner", resource);
  first.finish();
  assert.equal(runs.status("owner", 1).activeOperations, 1, "late cleanup cannot finish a newer lease");
  second.finish();
  assert.equal(await runs.waitIdle("owner", 1), true);
});
