#!/usr/bin/env node
/**
 * Deterministic Computer Use runtime seed prepare/check.
 * Does not run npm, postinstall, or network latest.
 * Target must be explicit — never guess host equals target.
 */
import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { computerUseTarget } from "./computer-use-targets.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = dirname(HERE);

const mode = process.argv.includes("--check")
  ? "check"
  : process.argv.includes("--prepare")
    ? "prepare"
    : null;
if (!mode) {
  process.stderr.write(
    "usage: prepare-computer-use-runtime.mjs --prepare|--check --target <target>\n",
  );
  process.exit(2);
}

const targetIdx = process.argv.indexOf("--target");
const target =
  targetIdx >= 0
    ? process.argv[targetIdx + 1]
    : process.env.GROK_CU_TARGET || "";
if (!target) {
  process.stderr.write(
    "--target is required (example: x86_64-windows). Do not guess host equals target.\n",
  );
  process.exit(2);
}
try { computerUseTarget(target); }
catch (error) {
  process.stderr.write(`${error.message}\n`);
  process.exit(2);
}

const extra = [];
const seed = process.env.GROK_CU_SEED;
if (seed) extra.push("--seed", seed);
const cache = process.env.GROK_CU_CACHE;
if (cache) extra.push("--cache", cache);

const cargo = spawnSync(
  "cargo",
  [
    "run",
    "-p",
    "grok-computer-use-core",
    "--locked",
    "--quiet",
    "--bin",
    "cu-prepare-runtime",
    "--target-dir",
    "target-cu-review",
    "--",
    `--${mode}`,
    "--target",
    target,
    "--repo",
    ROOT,
    ...extra,
  ],
  { cwd: join(ROOT, "src-tauri"), stdio: "inherit" },
);
process.exit(cargo.status ?? 1);
