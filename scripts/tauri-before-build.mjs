#!/usr/bin/env node
/**
 * Tauri beforeBuildCommand: every bundle must prepare/check its own Computer Use
 * runtime before UI build. Never silently ship a different OS/architecture.
 * Target comes from TAURI_ENV_TARGET_TRIPLE or GROK_CU_BUNDLE_TARGET — never guessed.
 */
import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { existsSync } from "node:fs";
import { computerUseTarget } from "./computer-use-targets.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = dirname(HERE);
const SEED_ROOT = join(ROOT, "src-tauri", "resources", "computer-use", "seed");
const gateOnly = process.argv.includes("--gate-only");
const triple =
  process.env.TAURI_ENV_TARGET_TRIPLE ||
  process.env.GROK_CU_BUNDLE_TARGET ||
  "";

if (!triple) {
  process.stderr.write(
    "TAURI_ENV_TARGET_TRIPLE or GROK_CU_BUNDLE_TARGET is required so Computer Use runtime is not bundled for the wrong OS.\n",
  );
  process.exit(1);
}

let target;
try { target = computerUseTarget(triple); }
catch (error) {
  process.stderr.write(`${error.message}\n`);
  process.exit(1);
}

// Resource globs point here. A CLI fixture override must never validate a different
// seed from the one Tauri will actually bundle.
const samePath = value => process.platform === "win32" ? value.toLowerCase() : value;
if (process.env.GROK_CU_SEED
  && samePath(resolve(ROOT, process.env.GROK_CU_SEED)) !== samePath(SEED_ROOT)) {
  process.stderr.write("GROK_CU_SEED override is not supported for bundles; the checked seed must match Tauri resources.\n");
  process.exit(1);
}

if (gateOnly) {
  process.stdout.write(`ok tauri-before-build gate target=${triple} runtime=${target.arch}\n`);
  process.exit(0);
}

function run(cmd, args) {
  const result = spawnSync(cmd, args, { cwd: ROOT, stdio: "inherit", shell: false });
  if (result.error) {
    process.stderr.write(`spawn ${cmd}: ${result.error.message}\n`);
    process.exit(1);
  }
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

{
  run(process.execPath, [
    join(ROOT, "scripts", "prepare-computer-use-runtime.mjs"),
    "--prepare",
    "--target",
    target.arch,
  ]);
  run(process.execPath, [
    join(ROOT, "scripts", "prepare-computer-use-runtime.mjs"),
    "--check",
    "--target",
    target.arch,
  ]);
  const seed = join(SEED_ROOT, "manifest.json");
  if (!existsSync(seed)) {
    process.stderr.write("Computer Use seed missing after prepare; refusing to bundle.\n");
    process.exit(1);
  }
  run(process.execPath, [
    join(ROOT, "scripts", "audit-computer-use-bundle.mjs"),
    dirname(seed), "--target", target.arch, "--seed-only",
  ]);
}

if (process.platform === "win32") {
  run("cmd.exe", ["/c", "pnpm", "build:ui"]);
} else {
  run("pnpm", ["build:ui"]);
}
