// Owned manual-acceptance pages for the App WebView Computer Use surface.
// The fixture binds loopback only and keeps all effects in this process.
import { Buffer } from "node:buffer";
import { lookup } from "node:dns/promises";
import http from "node:http";
import process from "node:process";
import { pathToFileURL, URL } from "node:url";

const MAX_BODY_BYTES = 1024;
const MAX_EVENTS = 80;

export function parseFixturePort(value = "0") {
  if (!/^\d{1,5}$/.test(value)) throw new Error("GROK_CU_WEBVIEW_FIXTURE_PORT must be a valid TCP port");
  const port = Number(value);
  if (!Number.isSafeInteger(port) || port > 65535) throw new Error("GROK_CU_WEBVIEW_FIXTURE_PORT must be a valid TCP port");
  return port;
}

function scriptString(value) {
  return JSON.stringify(value).replaceAll("<", "\\u003c");
}

function renderPage(id, state) {
  return `<!doctype html><html lang="zh-CN"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>内嵌页 ${state.label} - Computer Use 验收</title>
<style>
body{font:17px system-ui,sans-serif;max-width:680px;margin:48px auto;padding:24px;color:#17202a;background:#f3f5f7}
main{background:#fff;border:1px solid #d7dde3;border-radius:12px;padding:30px}
h1{font-size:25px;margin-top:0}label{display:block;margin:20px 0 8px}
input,button{font:inherit;padding:11px 13px;border:1px solid #7d8b99;border-radius:7px}
input{box-sizing:border-box;width:100%}button{background:#edf1f4;color:#17202a;cursor:pointer;margin:12px 12px 12px 0}
output{display:block;padding:15px;background:#f3f5f7;border-radius:7px}small{color:#586775}
</style></head><body><main><h1>内嵌页面 ${state.label}</h1>
<p>保存本页的中文备注，并通过按钮调整本页独立计数。</p>
<label for="note">页面 ${state.label} 备注</label><input id="note" maxlength="120" autocomplete="off">
<button id="save" type="button">保存页面 ${state.label} 备注</button>
<button id="increment" type="button">页面 ${state.label} 计数加一</button>
<output id="result" aria-live="polite"></output>
<p><small>本机验收页 ${state.label}，数据仅存于当前测试进程。</small></p></main>
<script>
let note=${scriptString(state.note)},count=${state.count};
const input=document.getElementById("note"),result=document.getElementById("result");
input.value=note;
function show(){result.textContent="已保存备注："+note+"；计数："+count;}show();
async function report(action){show();await fetch("/effect/${id}",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({action,note,count})});}
document.getElementById("save").addEventListener("click",()=>{note=input.value;void report("save");});
document.getElementById("increment").addEventListener("click",()=>{count++;void report("increment");});
</script></body></html>`;
}

function createHandler(pages) {
  return async (request, response) => {
    const url = new URL(request.url || "/", "http://127.0.0.1");
    response.setHeader("Cache-Control", "no-store");
    const pageMatch = /^\/(a|b)$/.exec(url.pathname);
    if (request.method === "GET" && pageMatch) {
      response.setHeader("Content-Type", "text/html; charset=utf-8");
      response.end(renderPage(pageMatch[1], pages.get(pageMatch[1])));
      return;
    }
    if (request.method === "GET" && url.pathname === "/state") {
      response.setHeader("Content-Type", "application/json; charset=utf-8");
      response.end(JSON.stringify(Object.fromEntries(pages)));
      return;
    }
    const effectMatch = /^\/effect\/(a|b)$/.exec(url.pathname);
    if (request.method === "POST" && effectMatch) {
      let body = "";
      for await (const part of request) {
        body += part;
        if (Buffer.byteLength(body) > MAX_BODY_BYTES) {
          response.writeHead(413).end();
          return;
        }
      }
      try {
        const effect = JSON.parse(body);
        const state = pages.get(effectMatch[1]);
        if (!state || !["save", "increment"].includes(effect.action)
          || typeof effect.note !== "string" || effect.note.length > 120
          || !Number.isSafeInteger(effect.count) || effect.count < 0 || effect.count > 50
          || state.events.length >= MAX_EVENTS) throw new Error("invalid effect");
        state.note = effect.note;
        state.count = effect.count;
        state.events.push({ ...effect, receivedUtc: new Date().toISOString() });
        response.writeHead(204).end();
      } catch {
        response.writeHead(400).end();
      }
      return;
    }
    response.writeHead(404).end();
  };
}

function listen(server, port, host) {
  return new Promise((resolve, reject) => {
    const failed = (error) => reject(error);
    server.once("error", failed);
    server.listen(port, host, () => {
      server.off("error", failed);
      resolve();
    });
  });
}

function closeServer(server) {
  if (!server?.listening) return Promise.resolve();
  return new Promise((resolve, reject) => {
    server.close((error) => error ? reject(error) : resolve());
    server.closeIdleConnections?.();
  });
}

export async function startAppWebviewFixture({ port = 0 } = {}) {
  if (!Number.isSafeInteger(port) || port < 0 || port > 65535) throw new Error("fixture port must be valid");
  const pages = new Map([
    ["a", { label: "A", note: "未保存", count: 0, events: [] }],
    ["b", { label: "B", note: "未保存", count: 0, events: [] }],
  ]);
  const handler = createHandler(pages);
  const ipv4Server = http.createServer(handler);
  await listen(ipv4Server, port, "127.0.0.1");
  const assignedPort = ipv4Server.address().port;
  let ipv6Server = null;
  const localhostAddresses = await lookup("localhost", { all: true, verbatim: true }).catch(() => []);
  if (localhostAddresses.some(({ family }) => family === 6 || family === "IPv6")) {
    ipv6Server = http.createServer(handler);
    try {
      await listen(ipv6Server, assignedPort, "::1");
    } catch (error) {
      await closeServer(ipv4Server);
      throw new Error("localhost resolves to IPv6 but the fixture could not bind ::1", { cause: error });
    }
  }
  const localhostOrigin = `http://localhost:${assignedPort}`;
  const ipv4Origin = `http://127.0.0.1:${assignedPort}`;
  let closed = false;
  return {
    pid: process.pid,
    port: assignedPort,
    pageA: `${localhostOrigin}/a`,
    pageB: `${localhostOrigin}/b`,
    state: `${localhostOrigin}/state`,
    ipv4PageA: `${ipv4Origin}/a`,
    ipv4PageB: `${ipv4Origin}/b`,
    ipv4State: `${ipv4Origin}/state`,
    close: async () => {
      if (closed) return;
      closed = true;
      await Promise.all([closeServer(ipv4Server), closeServer(ipv6Server)]);
    },
  };
}

async function main() {
  const fixture = await startAppWebviewFixture({ port: parseFixturePort(process.env.GROK_CU_WEBVIEW_FIXTURE_PORT) });
  const { close, ...description } = fixture;
  process.stdout.write(`${JSON.stringify(description)}\n`);
  let closing = false;
  const shutdown = async (exitCode) => {
    if (closing) return;
    closing = true;
    await close();
    process.exit(exitCode);
  };
  process.stdin.setEncoding("utf8");
  process.stdin.on("data", (input) => {
    if (input.trim() === "quit") void shutdown(0);
  });
  process.stdin.once("end", () => { void shutdown(0); });
  process.once("SIGINT", () => void shutdown(130));
  process.once("SIGTERM", () => void shutdown(143));
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    process.stderr.write(`${error instanceof Error ? error.stack : String(error)}\n`);
    process.exitCode = 1;
  });
}
