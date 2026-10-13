#!/usr/bin/env node
/**
 * Deterministic Computer Use runtime seed prepare/check or source-only audit.
 * Does not run npm, postinstall, or network latest.
 * Target must be explicit — never guess host equals target.
 */
import { spawnSync } from "node:child_process";
import process from "node:process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { runtimeCargoArgs, runtimeRequest, runtimeUsage } from "./computer-use-runtime-command.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = dirname(HERE);

let request;
try { request = runtimeRequest(process.argv.slice(2), process.env); }
catch (error) {
  process.stderr.write(`${error.message}\n${runtimeUsage}\n`);
  process.exit(2);
}
if (request.help) {
  process.stdout.write(`${runtimeUsage}\n--verify-sources does not prepare or publish a runtime seed.\n`);
  process.exit(0);
}

const cargo = spawnSync(
  "cargo",
  runtimeCargoArgs(request, ROOT),
  { cwd: join(ROOT, "src-tauri"), stdio: "inherit" },
);
if (cargo.error) process.stderr.write(`Unable to launch cargo: ${cargo.error.message}\n`);
process.exit(cargo.status ?? 1);
