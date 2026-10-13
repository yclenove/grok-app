import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { mkdirSync, mkdtempSync, realpathSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import test from "node:test";
import { taskFixtureArtifactDirectory, taskFixtureRoot, taskFixtureTemporaryRoot } from "./fixtures/task-fixture-root.mjs";

function fixture(t) {
  const parent = mkdtempSync(join(tmpdir(), "grok-cu-temp-contract-"));
  t.after(() => rmSync(parent, { recursive: true, force: true }));
  const rustTemp = join(parent, "rust-tmp");
  const nodeTemp = join(parent, "node-temp");
  mkdirSync(rustTemp);
  mkdirSync(nodeTemp);
  const root = join(rustTemp, `grok-cu-managed-task-${randomUUID()}`);
  mkdirSync(root);
  writeFileSync(join(root, "owned-task-fixture"), "synthetic ownership marker");
  return { root, rustTemp, nodeTemp, env: { TMP: rustTemp, TEMP: nodeTemp } };
}

test("Windows fixture keeps Rust TMP priority when Node TEMP differs", t => {
  const f = fixture(t);
  assert.equal(taskFixtureTemporaryRoot(f.env, "win32"), realpathSync.native(f.rustTemp));
  assert.equal(taskFixtureRoot(f.root, f.env, "win32"), realpathSync.native(f.root));
  assert.throws(() => taskFixtureRoot(f.root, { TMP: f.nodeTemp, TEMP: f.rustTemp }, "win32"),
    /explicit isolated/);
});

test("Windows TEMP fallback remains canonical when TMP is missing or empty", t => {
  const f = fixture(t);
  for (const TMP of [undefined, ""]) {
    assert.equal(taskFixtureRoot(f.root, { TMP, TEMP: f.rustTemp }, "win32"), realpathSync.native(f.root));
  }
});

test("fixture ownership marker and exact root name remain required", t => {
  const f = fixture(t);
  rmSync(join(f.root, "owned-task-fixture"));
  assert.throws(() => taskFixtureRoot(f.root, f.env, "win32"), /explicit isolated/);
  const other = join(f.rustTemp, "arbitrary-directory");
  mkdirSync(other);
  writeFileSync(join(other, "owned-task-fixture"), "synthetic marker");
  assert.throws(() => taskFixtureRoot(other, f.env, "win32"), /explicit isolated/);
});

test("Windows user-profile fallback matches the OS when both temp variables are absent", t => {
  const f = fixture(t);
  const env = { USERPROFILE: f.rustTemp, SystemRoot: f.nodeTemp };
  assert.equal(taskFixtureRoot(f.root, env, "win32"), realpathSync.native(f.root));
  assert.equal(taskFixtureTemporaryRoot({ SystemRoot: f.nodeTemp }, "win32"), realpathSync.native(f.nodeTemp));
});

test("fixture rejects a linked root even when it resolves to the marked directory", t => {
  const f = fixture(t);
  const link = join(f.rustTemp, `grok-cu-managed-task-${randomUUID()}`);
  symlinkSync(f.root, link, process.platform === "win32" ? "junction" : "dir");
  assert.throws(() => taskFixtureRoot(link, f.env, "win32"), /explicit isolated/);
  // Unlink the fixture junction itself before the ordinary directory cleanup.
  rmSync(link);
});

test("non-Windows fixture ignores Windows TMP/TEMP priority", t => {
  const f = fixture(t);
  assert.equal(taskFixtureTemporaryRoot(f.env, "linux"), realpathSync.native(tmpdir()));
  assert.throws(() => taskFixtureRoot(f.root, f.env, "linux"), /explicit isolated/);
});

test("artifact observer uses Node's physical temp directory and preserves missing-path behavior", t => {
  const f = fixture(t);
  const artifact = join(f.nodeTemp, "playwright-artifacts-owned");
  assert.equal(taskFixtureArtifactDirectory(artifact, f.nodeTemp), true);
  assert.equal(taskFixtureArtifactDirectory(artifact, f.rustTemp), false);
  assert.equal(taskFixtureArtifactDirectory(join(f.nodeTemp, "other-directory"), f.nodeTemp), false);
  assert.equal(taskFixtureArtifactDirectory(join(f.nodeTemp, "missing", "playwright-artifacts-owned"), f.nodeTemp), false);
  assert.equal(taskFixtureArtifactDirectory(new URL("file:///not-a-string"), f.nodeTemp), false);
  const alias = join(f.rustTemp, "node-temp-alias");
  symlinkSync(f.nodeTemp, alias, process.platform === "win32" ? "junction" : "dir");
  assert.equal(taskFixtureArtifactDirectory(join(alias, "playwright-artifacts-owned"), f.nodeTemp), true);
  rmSync(alias);
});
