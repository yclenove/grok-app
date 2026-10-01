import test from "node:test";
import assert from "node:assert/strict";
import { recordConnectionFailure, readConnectionDiagnostics } from "./connection-diagnostics.mjs";

test("connection diagnostics are bounded, copied and allowlisted without request data", () => {
  for (let i = 0; i < 40; i++) recordConnectionFailure("/cu/extension-status", "http", 403);
  const expected = readConnectionDiagnostics();
  assert.equal(expected.length, 32);
  for (const row of expected) {
    assert.deepEqual(Object.keys(row), ["elapsedMs", "route", "reason", "status"]);
    assert.equal(row.status, 403); assert(Number.isSafeInteger(row.elapsedMs));
  }
  recordConnectionFailure("/cu/extension-status?token=PRIVATE", "http", 403);
  recordConnectionFailure("/cu/extension-status", "PRIVATE error response", 403);
  assert.deepEqual(readConnectionDiagnostics(), expected);
  expected[0].route = "PRIVATE";
  assert(!JSON.stringify(readConnectionDiagnostics()).includes("PRIVATE"));
  recordConnectionFailure("/cu/extension-status", "network", "PRIVATE");
  assert.equal(readConnectionDiagnostics().at(-1).status, null);
});
