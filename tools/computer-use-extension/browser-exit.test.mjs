import { test } from "node:test";
import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import {
  assertCleanupHasNoSecrets, decideBrowserExit, durableCleanupFromJournal, durableCleanupRecord,
} from "./browser-exit.mjs";

const previousId = "00000000-0000-4000-8000-000000000021";
const currentId = "00000000-0000-4000-8000-000000000022";
const requestId = "00000000-0000-4000-8000-000000000023";
const documentId = "ab".repeat(16);

function matched(phase) {
  return {
    writeSucceeded: true,
    paired: true,
    currentId,
    previousId,
    physical: phase,
    record: { browserId: previousId, requestId, documentId, phase },
    pending: { browserId: previousId, requestId, documentId },
  };
}

test("durable cleanup record keeps no secret fields", () => {
  const completionKey = "c".repeat(64);
  const journal = {
    endpoint: "http://127.0.0.1:9/?token=secret",
    phase: "physicallySettled",
    proof: {
      completionKey,
      binding: { requestId, documentId },
    },
  };
  const record = durableCleanupFromJournal(journal, previousId);
  assertCleanupHasNoSecrets(record);
  const encoded = JSON.stringify(record);
  assert.equal(encoded.includes(completionKey), false);
  assert.equal(encoded.includes("token"), false);
  assert.equal(encoded.includes("endpoint"), false);
  assert.deepEqual(Object.keys(record).sort(), ["browserId", "documentId", "phase", "requestId", "version"]);
  assert.throws(() => durableCleanupRecord({ ...record, completionKey }), /browserExitUnavailable/);
});

test("write failure and mismatched identity or proof do not release the owner", () => {
  assert.deepEqual(decideBrowserExit({ ...matched("physicallySettled"), writeSucceeded: false }), {
    release: false, result: null, actions: 0,
  });
  assert.equal(decideBrowserExit({ ...matched("prepared"), previousId: currentId }).release, false);
  assert.equal(decideBrowserExit({
    ...matched("prepared"),
    pending: { browserId: previousId, requestId: randomUUID(), documentId },
  }).release, false);
  assert.equal(decideBrowserExit({
    ...matched("prepared"),
    record: { browserId: currentId, requestId, documentId, phase: "prepared" },
  }).release, false);
  for (const flag of ["newShare", "documentMissing", "timedOut", "storageLost", "hostAbsent"]) {
    assert.equal(decideBrowserExit({ ...matched("physicallySettled"), [flag]: true }).release, false, flag);
  }
});

test("a shipped versioned cleanup record still releases on an exact match", () => {
  const record = durableCleanupRecord({
    browserId: previousId, requestId, documentId, phase: "physicallySettled",
  });
  assert.equal(record.version, 1);
  assert.deepEqual(decideBrowserExit({ ...matched("physicallySettled"), record }), {
    release: true, result: "cleanup", actions: 0,
  });
});

test("a matching exit records only cleanup or unknown and does not replay", () => {
  const cleanup = decideBrowserExit(matched("physicallySettled"));
  const unknown = decideBrowserExit(matched("prepared"));
  assert.deepEqual(cleanup, { release: true, result: "cleanup", actions: 0 });
  assert.deepEqual(unknown, { release: true, result: "unknown", actions: 0 });
  assert.equal(cleanup.result === "applied" || cleanup.result === "verified", false);
  assert.equal(unknown.actions, 0);
});
