import test from "node:test";
import assert from "node:assert/strict";
import {
  existsSync, lstatSync, mkdirSync, mkdtempSync, realpathSync, rmSync, symlinkSync, writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { isAbsolute, join, relative } from "node:path";
import { pinnedChromeLaunchPath, pinnedChromePack } from "./immutable-chrome.mjs";

function ownedRoot(t) {
  const base = realpathSync(tmpdir());
  const root = mkdtempSync(join(base, "cu-immutable-chrome-"));
  t.after(() => {
    const path = relative(base, root);
    assert(!isAbsolute(path) && path.startsWith("cu-immutable-chrome-") && !path.includes(".."));
    rmSync(root, { recursive: true, force: true });
  });
  return root;
}

function writePack(root) {
  const tree = join(root, "pack", "chromium", "chrome-win");
  mkdirSync(tree, { recursive: true });
  writeFileSync(join(root, "pack", "manifest.json"), "{}\n");
  writeFileSync(join(tree, "chrome.exe"), "MZ-pinned");
  writeFileSync(join(tree, "CREDITS.html"), "credits");
  return tree;
}

test("pinned pack chrome launches from a copy outside the locked tree", (t) => {
  const root = ownedRoot(t);
  const tree = writePack(root);
  const exe = join(tree, "chrome.exe");
  const writable = join(root, "exec");
  const launch = pinnedChromeLaunchPath(exe, writable);
  assert.equal(launch, join(writable, "chrome-win", "chrome.exe"));
  assert.equal(existsSync(launch), true);
  writeFileSync(join(writable, "chrome-win", "debug.log"), "log");
  mkdirSync(join(writable, "chrome-win", "Dictionaries"), { recursive: true });
  writeFileSync(join(writable, "chrome-win", "Dictionaries", "en-US-10-1.bdic"), "dict");
  assert.equal(existsSync(join(tree, "debug.log")), false);
  assert.equal(existsSync(join(tree, "Dictionaries")), false);
  assert.equal(pinnedChromeLaunchPath(exe, writable), launch);
});

test("a non-pack chrome path is not redirected", (t) => {
  const root = ownedRoot(t);
  const exe = join(root, "Application", "chrome.exe");
  mkdirSync(join(root, "Application"), { recursive: true });
  writeFileSync(exe, "MZ-system");
  assert.equal(pinnedChromePack(exe), null);
  assert.equal(pinnedChromeLaunchPath(exe, join(root, "exec")), realpathSync(exe));
});

test("execution root inside the pinned pack is rejected", (t) => {
  const root = ownedRoot(t);
  const tree = writePack(root);
  assert.throws(
    () => pinnedChromeLaunchPath(join(tree, "chrome.exe"), join(tree, "nested")),
    /must not be inside the pinned pack/,
  );
});

test("a symlink in the pinned tree is rejected", (t) => {
  const root = ownedRoot(t);
  const tree = writePack(root);
  try {
    symlinkSync(join(tree, "CREDITS.html"), join(tree, "linked.html"));
  } catch {
    t.skip("symlink not permitted");
    return;
  }
  const info = lstatSync(join(tree, "linked.html"));
  assert.equal(info.isSymbolicLink(), true);
  assert.throws(
    () => pinnedChromeLaunchPath(join(tree, "chrome.exe"), join(root, "exec")),
    /symlink/,
  );
});
