#!/usr/bin/env node
/** Private Cua Driver probe. Does not add Cua to the root runtime. */
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const ROOT = dirname(fileURLToPath(import.meta.url));
const dest = join(ROOT, "..", "computer-use-cua");
const outDir = join(ROOT, ".run", "cua");
mkdirSync(outDir, { recursive: true });
const pin = "c5a15f3df3b29ffbe774de9f33d632fe75afec75";
const pathHit = spawnSync("where", ["cua-driver"], { encoding: "utf8" });
const which = (pathHit.stdout || "").trim();
let clone = { status: null, stderr: "", stdout: "" };
if (!existsSync(join(dest, ".git"))) {
  clone = spawnSync(
    "git",
    [
      "clone",
      "--depth",
      "1",
      "--filter=blob:none",
      "--sparse",
      "https://github.com/trycua/cua.git",
      dest,
    ],
    { encoding: "utf8", timeout: 120_000 },
  );
}
const sparse = existsSync(dest)
  ? spawnSync("git", ["-C", dest, "sparse-checkout", "set", "libs/cua-driver"], {
      encoding: "utf8",
      timeout: 60_000,
    })
  : { status: 1, stderr: "clone missing", stdout: "" };
const report = {
  pin,
  pathCli: which || null,
  cloneStatus: clone.status,
  cloneStderr: (clone.stderr || "").slice(0, 2000),
  sparseStatus: sparse.status,
  destExists: existsSync(dest),
  driverReadme: existsSync(join(dest, "libs", "cua-driver", "README.md")),
};
writeFileSync(join(outDir, "probe.json"), JSON.stringify(report, null, 2));
process.stdout.write(JSON.stringify(report, null, 2) + "\n");
if (!which && !report.driverReadme) process.exitCode = 2;
