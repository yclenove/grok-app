import test from "node:test";
import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { InputCleanup } from "./input-cleanup.mjs";
import { RunOperations } from "./run-operations.mjs";

test("uncertain input retains the original admission at full capacity until actual context close", () => {
  const operations = new RunOperations({ maxActive: 1 });
  const request = operations.begin("owner", 1);
  const context = new EventEmitter();
  const cleanup = new InputCleanup(context);
  cleanup.retain(request);
  assert.equal(operations.pause("owner", 1).idle, false);
  assert.throws(() => operations.resume("owner", 1, 2));
  assert.throws(() => cleanup.check(), error => error.workerCode === "input_quarantined" &&
    error.completion === "not_started");
  context.emit("close");
  assert.equal(operations.status("owner", 1).idle, true);
  assert.throws(() => cleanup.check(), "closed profile cannot regain input authority");
});

test("context closure before cleanup handoff does not strand occupancy", () => {
  const operations = new RunOperations();
  const request = operations.begin("owner", 1);
  const context = new EventEmitter();
  const cleanup = new InputCleanup(context);
  context.emit("close");
  cleanup.retain(request);
  assert.equal(operations.status("owner", 1).idle, true);
});
