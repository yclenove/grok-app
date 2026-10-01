#!/usr/bin/env node
/**
 * Bounded grok-4.6 observe→act→verify against the shipped Host IPC + self-built window.
 * Isolated GROK_HOME (auth.json copy only). Does not rewrite ~/.grok.
 * Oracle is %TEMP%/grok-cu-fixture-clicks.txt, never tool text.
 */
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { homedir } from "node:os";
import { fileURLToPath } from "node:url";
import { spawn, spawnSync } from "node:child_process";

const ROOT = dirname(fileURLToPath(import.meta.url));
const REPO = join(ROOT, "..", "..");
const OUT = join(ROOT, ".run", "p8");
mkdirSync(OUT, { recursive: true });
const home = join(OUT, "grok-home");
mkdirSync(home, { recursive: true });
const srcAuth = join(homedir(), ".grok", "auth.json");
if (existsSync(srcAuth)) {
  copyFileSync(srcAuth, join(home, "auth.json"));
}

const holdFile = join(OUT, "hold.json");
const envHold = {
  ...process.env,
  GROK_CU_HOLD_IPC: "1",
  GROK_CU_HOLD_SECS: "150",
  GROK_CU_HOLD_FILE: holdFile,
  CARGO_TARGET_DIR: join(REPO, "src-tauri", "target-cu"),
};
const holder = spawn(
  "cargo",
  ["run", "--offline", "--bin", "cu_probe"],
  {
    cwd: join(REPO, "src-tauri"),
    env: envHold,
    stdio: ["ignore", "pipe", "pipe"],
  },
);

let holdBuf = "";
const hold = await new Promise((resolve, reject) => {
  const t = setTimeout(() => reject(new Error("hold ipc timeout")), 90_000);
  const tryParse = () => {
    const line = holdBuf.split(/\r?\n/).find((l) => l.startsWith("{"));
    if (!line) return;
    try {
      const v = JSON.parse(line);
      if (v.url && v.token && v.title) {
        clearTimeout(t);
        resolve(v);
      }
    } catch {
      /* keep reading */
    }
  };
  holder.stdout.on("data", (c) => {
    holdBuf += c.toString("utf8");
    tryParse();
  });
  holder.stderr.on("data", (c) => {
    holdBuf += c.toString("utf8");
  });
  holder.on("error", reject);
  holder.on("exit", (code) => {
    if (!holdBuf.includes('"url"')) {
      clearTimeout(t);
      reject(new Error(`holder exited ${code}: ${holdBuf.slice(0, 800)}`));
    }
  });
}).catch((e) => ({ error: String(e.message || e) }));

if (hold.error) {
  writeFileSync(
    join(OUT, "p8-desktop-model.json"),
    JSON.stringify({ ok: false, reason: "hold_ipc", error: hold.error }, null, 2),
  );
  process.stderr.write(`hold failed: ${hold.error}\n`);
  holder.kill();
  process.exit(2);
}

const mcp = join(REPO, "tools", "computer-use-mcp", "server.mjs").replace(/\\/g, "/");
const mcpLog = join(OUT, "mcp-server.log");
writeFileSync(
  join(home, "config.toml"),
  `[mcp_servers.cu_host]
command = "node"
args = ["${mcp}"]
startup_timeout_sec = 15

[mcp_servers.cu_host.env]
GROK_APP_CU_IPC = "${hold.url}"
GROK_APP_CU_TOKEN = "${hold.token}"
GROK_APP_CU_SESSION = "p8-model"
GROK_APP_CU_MCP_LOG = "${mcpLog.replace(/\\/g, "/")}"
`,
);

const prompt = [
  `Call cu_host__computer_list_targets now.`,
  `Then call cu_host__computer_open_target with title "${hold.title}".`,
  `Then call cu_host__computer_observe and look at the image.`,
  `Then call cu_host__computer_act with action click and image-pixel x,y on the Count button once.`,
  `Do not click ChatGPT, WeChat, or any other window. Stop after one click.`,
].join(" ");

const debugFile = join(OUT, "grok-debug.log");
const t0 = Date.now();
const r = spawnSync(
  "grok",
  [
    "-p",
    prompt,
    "--model",
    "grok-4.6",
    "--effort",
    "low",
    "--always-approve",
    "--debug-file",
    debugFile,
  ],
  {
    encoding: "utf8",
    timeout: 120_000,
    env: {
      ...process.env,
      GROK_HOME: home,
      GROK_CLAUDE_MCPS_ENABLED: "false",
      GROK_CURSOR_MCPS_ENABLED: "false",
      GROK_APP_CU_IPC: hold.url,
      GROK_APP_CU_TOKEN: hold.token,
      GROK_APP_CU_SESSION: "p8-model",
      GROK_APP_CU_MCP_LOG: mcpLog,
    },
  },
);
const ms = Date.now() - t0;
holder.kill();

const clicksPath = hold.clicksPath || join(process.env.TEMP || "/tmp", "grok-cu-fixture-clicks.txt");
const file = existsSync(clicksPath) ? readFileSync(clicksPath, "utf8").trim() : "";
const clicks = Number(file) || 0;
const stdout = (r.stdout || "").trim();
const stderr = (r.stderr || "").slice(0, 4000);
const leaked = /grok-cu-fixture-clicks/.test(stdout);
const report = {
  ok: clicks >= 1 && r.status === 0 && !leaked,
  exitCode: r.status,
  ms,
  clicks,
  file,
  leakedOraclePath: leaked,
  title: hold.title,
  stdout: stdout.slice(0, 4000),
  stderr,
};
writeFileSync(join(OUT, "p8-desktop-model.json"), JSON.stringify(report, null, 2));
process.stdout.write(`clicks=${clicks} exit=${r.status} leaked=${leaked} ms=${ms}\n`);
if (!report.ok) process.exitCode = clicks >= 1 ? 0 : 2;
