#!/usr/bin/env node
/**
 * P0.4: bind a Cua window target, then cancel the daemon.
 * Does not click ChatGPT / WeChat. Private named pipe, overlay off.
 */
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn, spawnSync } from "node:child_process";

const ROOT = dirname(fileURLToPath(import.meta.url));
const REPO = join(ROOT, "..", "..");
const OUT = join(ROOT, ".run", "cua");
mkdirSync(OUT, { recursive: true });
const bin = join(
  REPO,
  "tools",
  "computer-use-cua",
  "libs",
  "cua-driver",
  "rust",
  "target",
  "debug",
  "cua-driver.exe",
);
const socket = `\\\\.\\pipe\\grok-cu-p04-${process.pid}`;

function fail(reason, extra) {
  const report = { ok: false, reason, socket, bin, ...extra };
  writeFileSync(join(OUT, "bind-cancel.json"), JSON.stringify(report, null, 2));
  process.stdout.write(JSON.stringify(report, null, 2) + "\n");
  process.exitCode = 2;
}

if (!existsSync(bin)) {
  fail("binary_missing");
  process.exit(2);
}

const fixtureTitle = `GrokCuFixture-cua-${process.pid}`;
const fixture = spawn(
  "powershell",
  ["-NoProfile", "-STA", "-File", join(ROOT, "win-form.ps1")],
  {
    stdio: ["ignore", "ignore", "ignore"],
    env: { ...process.env, GROK_CU_FIXTURE_TITLE: fixtureTitle },
    windowsHide: false,
  },
);

const serve = spawn(
  bin,
  ["serve", "--socket", socket, "--no-overlay", "--no-permissions-gate"],
  { stdio: ["ignore", "pipe", "pipe"] },
);
let serveLog = "";
serve.stdout.on("data", (c) => {
  serveLog += c.toString("utf8");
});
serve.stderr.on("data", (c) => {
  serveLog += c.toString("utf8");
});

const waitReady = async () => {
  const t0 = Date.now();
  while (Date.now() - t0 < 25_000) {
    const st = spawnSync(bin, ["status", "--socket", socket], {
      encoding: "utf8",
      timeout: 8_000,
    });
    const text = `${st.stdout || ""}\n${st.stderr || ""}`;
    if (st.status === 0 && !/not running/i.test(text)) return st;
    await new Promise((r) => setTimeout(r, 400));
  }
  return null;
};

const status1 = await waitReady();
if (!status1) {
  serve.kill();
  fail("daemon_not_ready", { serveLog: serveLog.slice(0, 4000) });
  process.exit(2);
}

function call(tool, args) {
  const argv = ["call", tool, "--socket", socket];
  if (args) argv.push(JSON.stringify(args));
  return spawnSync(bin, argv, { encoding: "utf8", timeout: 30_000 });
}

function parseWindows(stdout) {
  try {
    const parsed = JSON.parse((stdout || "").trim() || "null");
    const arr =
      parsed?.structuredContent?.windows ||
      parsed?._legacy_windows ||
      parsed?.windows ||
      parsed?.result?.windows ||
      [];
    return Array.isArray(arr) ? arr : [];
  } catch {
    return [];
  }
}

let listed = { status: 1, stdout: "", stderr: "" };
let windows = [];
const tList = Date.now();
while (Date.now() - tList < 12_000) {
  listed = call("list_windows", {});
  windows = parseWindows(listed.stdout);
  if (windows.some((w) => String(w.title || "").includes("GrokCuFixture-cua"))) break;
  await new Promise((r) => setTimeout(r, 400));
}
const listOut = `${listed.stdout || ""}\n${listed.stderr || ""}`;

const blocked = /chatgpt|微信|wechat|^grok$/i;
const bound =
  windows.find((w) =>
    String(w.title || "").includes("GrokCuFixture-cua"),
  ) ||
  windows.find((w) => String(w.title || "").includes("GrokCuFixture")) ||
  null;
if (bound && blocked.test(String(bound.title || ""))) {
  serve.kill();
  try {
    fixture.kill();
  } catch {
    /* ignore */
  }
  fail("refused_sensitive_target", { title: bound.title, listOut: listOut.slice(0, 1500) });
  process.exit(2);
}

let state = null;
if (bound && (bound.pid != null || bound.window_id != null)) {
  const args = {};
  if (bound.pid != null) args.pid = bound.pid;
  if (bound.window_id != null) args.window_id = bound.window_id;
  const st = call("get_window_state", args);
  state = {
    exit: st.status,
    stdout: (st.stdout || "").slice(0, 2000),
    stderr: (st.stderr || "").slice(0, 1000),
  };
}

const stop = spawnSync(bin, ["stop", "--socket", socket], {
  encoding: "utf8",
  timeout: 10_000,
});
try {
  serve.kill();
} catch {
  /* ignore */
}
try {
  fixture.kill();
} catch {
  /* ignore */
}

const after = spawnSync(bin, ["status", "--socket", socket], {
  encoding: "utf8",
  timeout: 8_000,
});
const afterText = `${after.stdout || ""}\n${after.stderr || ""}`;
const cancelled = after.status !== 0 || /not running/i.test(afterText);

const report = {
  ok: Boolean(listed.status === 0 && bound && cancelled && !blocked.test(String(bound.title || ""))),
  fixtureTitle,
  socket,
  listExit: listed.status,
  windowCount: windows.length,
  bound: bound
    ? {
        pid: bound.pid ?? null,
        window_id: bound.window_id ?? null,
        title: bound.title ?? bound.name ?? null,
      }
    : null,
  state,
  stopExit: stop.status,
  cancelled,
  listStdout: (listed.stdout || "").slice(0, 2500),
  listStderr: (listed.stderr || "").slice(0, 1500),
  statusReady: (status1.stdout || "").slice(0, 800),
  statusAfter: afterText.slice(0, 800),
  serveLog: serveLog.slice(0, 2500),
};
writeFileSync(join(OUT, "bind-cancel.json"), JSON.stringify(report, null, 2));
process.stdout.write(JSON.stringify(report, null, 2) + "\n");
if (!report.ok) process.exitCode = 2;
