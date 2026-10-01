#!/usr/bin/env node
/**
 * P0 probe runner. Writes JSON evidence; does not enable the product flag.
 */
import { spawn, spawnSync } from "node:child_process";
import { createWriteStream, mkdirSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import os from "node:os";

const ROOT = dirname(fileURLToPath(import.meta.url));
const REPO = join(ROOT, "..", "..");
const OUT = process.env.GROK_CU_P0_OUT || join(ROOT, ".run", "p0");
mkdirSync(OUT, { recursive: true });

function now() {
  return new Date().toISOString();
}

function run(cmd, args, opts = {}) {
  const started = Date.now();
  const r = spawnSync(cmd, args, {
    encoding: "utf8",
    timeout: opts.timeout ?? 30_000,
    env: { ...process.env, ...(opts.env || {}) },
    cwd: opts.cwd || REPO,
    shell: false,
  });
  return {
    cmd: [cmd, ...args].join(" "),
    exitCode: r.status,
    stdout: (r.stdout || "").slice(0, 20_000),
    stderr: (r.stderr || "").slice(0, 20_000),
    ms: Date.now() - started,
  };
}

function mcpCall(child, msg) {
  const json = JSON.stringify(msg);
  const buf = Buffer.from(json, "utf8");
  child.stdin.write(`Content-Length: ${buf.length}\r\n\r\n`);
  child.stdin.write(buf);
}

function readMcp(child, timeoutMs = 8000) {
  return new Promise((resolve, reject) => {
    let buf = Buffer.alloc(0);
    const t = setTimeout(() => reject(new Error("mcp timeout")), timeoutMs);
    const onData = (chunk) => {
      buf = Buffer.concat([buf, chunk]);
      const headerEnd = buf.indexOf("\r\n\r\n");
      if (headerEnd < 0) return;
      const header = buf.slice(0, headerEnd).toString("utf8");
      const m = /Content-Length:\s*(\d+)/i.exec(header);
      if (!m) return;
      const len = Number(m[1]);
      const start = headerEnd + 4;
      if (buf.length < start + len) return;
      clearTimeout(t);
      child.stdout.off("data", onData);
      resolve(JSON.parse(buf.slice(start, start + len).toString("utf8")));
    };
    child.stdout.on("data", onData);
  });
}

async function syntheticTwice() {
  const oracleDir = join(OUT, "synthetic");
  mkdirSync(oracleDir, { recursive: true });
  const results = [];
  for (const i of [1, 2]) {
    const child = spawn(process.execPath, [join(ROOT, "synthetic-mcp.mjs")], {
      env: { ...process.env, GROK_CU_ORACLE_DIR: oracleDir, GROK_CU_SEED: "42" },
      stdio: ["pipe", "pipe", "pipe"],
    });
    mcpCall(child, { jsonrpc: "2.0", id: 1, method: "initialize", params: { protocolVersion: "2024-11-05", capabilities: {}, clientInfo: { name: "p0", version: "0" } } });
    await readMcp(child);
    mcpCall(child, { jsonrpc: "2.0", method: "notifications/initialized" });
    mcpCall(child, { jsonrpc: "2.0", id: 2, method: "tools/list", params: {} });
    const listed = await readMcp(child);
    mcpCall(child, { jsonrpc: "2.0", id: 3, method: "tools/call", params: { name: "computer_observe", arguments: {} } });
    const called = await readMcp(child);
    child.kill();
    const oracle = JSON.parse(readFileSync(join(oracleDir, "oracle.json"), "utf8"));
    const text = called.result?.content?.find((c) => c.type === "text")?.text || "";
    const image = called.result?.content?.find((c) => c.type === "image");
    results.push({
      pass: i,
      tools: listed.result?.tools?.map((t) => t.name) || [],
      hasImage: Boolean(image?.data),
      mime: image?.mimeType || null,
      text,
      oracleLeaked: text.includes(oracle.code) || /oracle/i.test(text),
      imageBytes: image?.data ? Buffer.from(image.data, "base64").length : 0,
    });
  }
  const consistent =
    results[0].imageBytes === results[1].imageBytes &&
    results[0].text === results[1].text &&
    !results[0].oracleLeaked &&
    !results[1].oracleLeaked &&
    results[0].hasImage;
  return { consistent, results };
}

function fourTargetMatrix() {
  const host = `${os.platform()}-${os.arch()}`;
  const rows = [
    {
      target: "Windows x64",
      evidenceClass: os.platform() === "win32" && os.arch() === "x64" ? "native" : "not_run",
      note: os.platform() === "win32" ? "this host" : "requires a Windows x64 machine",
    },
    {
      target: "macOS arm64",
      evidenceClass: "not_run",
      note: "requires Apple Silicon macOS",
    },
    {
      target: "macOS x64",
      evidenceClass: "not_run",
      note: "requires Intel macOS",
    },
    {
      target: "Linux x64",
      evidenceClass: "not_run",
      note: "requires Ubuntu GNOME Wayland and X11 separately; browser/XWayland is not Wayland",
    },
  ];
  return { host, rows };
}

async function main() {
  const report = {
    startedAt: now(),
    host: { platform: os.platform(), arch: os.arch(), release: os.release() },
    p01_cli: {
      grokVersion: run("grok", ["--version"]),
      grokModels: run("grok", ["models"]),
    },
    p02_synthetic: await syntheticTwice(),
    p06_matrix: fourTargetMatrix(),
    tabDefault: "playwright-extension",
    tabDefaultReason:
      "Managed browser stays Playwright. Existing Chrome/Edge default is Playwright extension/CDP because BrowserSkill plugin-level session ownership is not Grok multi-chat authorization, and pairing must check real extension identity.",
  };
  const outFile = join(OUT, "p0-run.json");
  writeFileSync(outFile, JSON.stringify(report, null, 2));
  process.stdout.write(`${outFile}\n`);
  if (!report.p02_synthetic.consistent) process.exitCode = 2;
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
