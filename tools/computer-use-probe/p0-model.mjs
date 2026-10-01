#!/usr/bin/env node
/** Bounded vision probe. Uses isolated GROK_HOME. Does not rewrite ~/.grok. */
import { copyFileSync, mkdirSync, writeFileSync, existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { homedir } from "node:os";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const ROOT = dirname(fileURLToPath(import.meta.url));
const home = join(ROOT, ".run", "grok-home");
mkdirSync(home, { recursive: true });
const srcAuth = join(homedir(), ".grok", "auth.json");
if (existsSync(srcAuth)) {
  copyFileSync(srcAuth, join(home, "auth.json"));
}
const mcp = join(ROOT, "synthetic-mcp.mjs").replace(/\\/g, "/");
writeFileSync(
  join(home, "config.toml"),
  `[mcp_servers.cu_syn]
command = "node"
args = ["${mcp}"]
`,
);
const oracleDir = join(ROOT, ".run", "p0", "synthetic");
const env = {
  ...process.env,
  GROK_HOME: home,
  GROK_CU_ORACLE_DIR: oracleDir,
  GROK_CU_SEED: "42",
};
const r = spawnSync(
  "grok",
  [
    "-p",
    "Call computer_observe. Look at the image. What 4-digit number is painted in the red box? Reply with only those four digits.",
    "--model",
    "grok-4.6",
    "--effort",
    "low",
  ],
  { encoding: "utf8", timeout: 120_000, env },
);
const oracle = existsSync(join(oracleDir, "oracle.json"))
  ? JSON.parse(readFileSync(join(oracleDir, "oracle.json"), "utf8"))
  : { code: null };
const out = (r.stdout || "").trim();
const hit = oracle.code && out.includes(oracle.code);
writeFileSync(
  join(ROOT, ".run", "p0", "p8-model.json"),
  JSON.stringify(
    {
      exitCode: r.status,
      ms: null,
      stderr: (r.stderr || "").slice(0, 4000),
      stdout: out.slice(0, 4000),
      matchedOracle: Boolean(hit),
      oraclePresentInStdout: hit,
    },
    null,
    2,
  ),
);
process.stdout.write(`matched=${hit} exit=${r.status}\n`);
if (!hit) process.exitCode = 2;
