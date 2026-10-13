// Source-test fixture ownership boundary, never bundled into a runtime.
import { existsSync, lstatSync, realpathSync } from "node:fs";
import { basename, dirname, join, toNamespacedPath } from "node:path";
import { tmpdir } from "node:os";

export function taskFixtureTemporaryRoot(env = process.env, platform = process.platform) {
  // Rust uses the Windows GetTempPath priority (TMP before TEMP); Node uses
  // TEMP before TMP. Compare against the original Rust fixture's directory.
  const temporary = platform === "win32"
    ? env.TMP || env.TEMP || env.USERPROFILE || env.SystemRoot || env.windir || tmpdir()
    : tmpdir();
  // The native resolver expands Windows short names and preserves the same
  // physical directory identity as Rust's canonicalize().
  return realpathSync.native(temporary);
}

export function taskFixtureRoot(supplied, env = process.env, platform = process.platform) {
  const root = realpathSync.native(supplied);
  if (toNamespacedPath(dirname(root)) !== toNamespacedPath(taskFixtureTemporaryRoot(env, platform)) ||
      !/^grok-cu-managed-task-[0-9a-f-]{36}$/.test(basename(root)) ||
      lstatSync(supplied).isSymbolicLink() || !existsSync(join(root, "owned-task-fixture"))) {
    throw new Error("explicit isolated task fixture root required");
  }
  return root;
}

export function taskFixtureArtifactDirectory(directory, temporary = tmpdir()) {
  if (typeof directory !== "string" || !basename(directory).startsWith("playwright-artifacts-")) {
    return false;
  }
  try {
    return toNamespacedPath(realpathSync.native(dirname(directory))) ===
      toNamespacedPath(realpathSync.native(temporary));
  } catch {
    // Diagnostic interception must not change rm(force)'s missing-path behavior.
    return false;
  }
}
