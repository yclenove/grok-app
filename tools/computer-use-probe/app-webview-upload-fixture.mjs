// Loopback-only manual upload oracle. Never reads a local path or forwards data.
import { Buffer } from "node:buffer";
import { createHash, randomBytes } from "node:crypto";
import http from "node:http";
import process from "node:process";
import { pathToFileURL, URL } from "node:url";
import { parseFixturePort } from "./app-webview-fixture.mjs";

export const MAX_FIXTURE_UPLOAD_BYTES = 64 * 1024;
const MAX_UPLOADS = 16;

function renderPage(uploadPath) {
  return `<!doctype html><html lang="zh-CN"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>本机文件上传 - Computer Use 验收</title>
<style>
body{font:17px system-ui,sans-serif;max-width:680px;margin:40px auto;padding:24px;background:#f3f5f7;color:#17202a}
main{background:#fff;border:1px solid #d7dde3;border-radius:12px;padding:28px}
label,output{display:block;margin:18px 0}button,input{font:inherit;padding:12px}
output{white-space:pre-wrap;overflow-wrap:anywhere}small{color:#586775}
</style></head><body><main><h1>本机文件上传验收</h1>
<p>只选择本次验收的公开测试文件。数据仅发送到当前电脑的回环接收器，不写磁盘、不转发。</p>
<label for="file">选择验收文件（最多 64 KiB）</label><input id="file" type="file">
<button id="submit" type="button">提交验收文件</button>
<output id="result" aria-live="polite">尚未提交</output>
<small>选择文件不会自动提交；接收次数可由独立 /state 接口核对。</small></main>
<script>
const file=document.getElementById("file"),button=document.getElementById("submit"),result=document.getElementById("result");
button.addEventListener("click",async()=>{
  const selected=file.files[0];
  if(!selected){result.textContent="请先选择验收文件";return;}
  if(selected.size>${MAX_FIXTURE_UPLOAD_BYTES}){result.textContent="文件超过验收页 64 KiB 上限";return;}
  button.disabled=true;result.textContent="正在提交";
  try{
    const response=await fetch(${JSON.stringify(uploadPath)},{method:"PUT",headers:{"Content-Type":selected.type||"application/octet-stream","X-Fixture-Filename":encodeURIComponent(selected.name)},body:selected});
    if(!response.ok)throw new Error("接收器拒绝："+response.status);
    const receipt=await response.json();
    result.textContent="已接收："+receipt.filename+"；字节："+receipt.bytes+"；次数："+receipt.sequence+"；SHA256："+receipt.sha256;
  }catch(error){result.textContent=String(error);}finally{button.disabled=false;}
});
</script></body></html>`;
}

function closeServer(server) {
  if (!server.listening) return Promise.resolve();
  return new Promise((resolve, reject) => {
    server.close(error => error ? reject(error) : resolve());
    server.closeIdleConnections();
  });
}

export async function startAppWebviewUploadFixture({ port = 0 } = {}) {
  if (!Number.isSafeInteger(port) || port < 0 || port > 65535) throw new Error("fixture port must be valid");
  const uploadPath = `/upload/${randomBytes(24).toString("hex")}`;
  const uploads = [];
  let origin;
  const server = http.createServer(async (request, response) => {
    response.setHeader("Cache-Control", "no-store");
    response.setHeader("X-Content-Type-Options", "nosniff");
    if (request.headers.host !== new URL(origin).host) {
      request.resume();
      response.writeHead(403).end();
      return;
    }
    if (request.method === "GET" && request.url === "/") {
      response.setHeader("Content-Type", "text/html; charset=utf-8");
      response.setHeader("Content-Security-Policy", "default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; connect-src 'self'; base-uri 'none'; frame-ancestors 'none'");
      response.end(renderPage(uploadPath));
      return;
    }
    if (request.method === "GET" && request.url === "/state") {
      response.setHeader("Content-Type", "application/json; charset=utf-8");
      response.end(JSON.stringify({ uploads }));
      return;
    }
    if (request.method !== "PUT" || request.url !== uploadPath) {
      request.resume();
      response.writeHead(404).end();
      return;
    }
    if (request.headers.origin !== origin || uploads.length >= MAX_UPLOADS) {
      request.resume();
      response.writeHead(403).end();
      return;
    }
    let filename;
    try {
      filename = decodeURIComponent(request.headers["x-fixture-filename"] || "");
      if (!filename || filename.length > 240 || /[\\/]/.test(filename)
        || [...filename].some(character => character.charCodeAt(0) < 32 || character.charCodeAt(0) === 127)) throw new Error("invalid name");
    } catch {
      request.resume();
      response.writeHead(400).end();
      return;
    }
    try {
      const chunks = [];
      let bytes = 0;
      for await (const chunk of request) {
        bytes += chunk.length;
        if (bytes > MAX_FIXTURE_UPLOAD_BYTES) {
          response.writeHead(413).end();
          return;
        }
        chunks.push(chunk);
      }
      // Recheck after the asynchronous body read: simultaneous submissions
      // must not overflow the retained evidence bound.
      if (uploads.length >= MAX_UPLOADS) {
        response.writeHead(403).end();
        return;
      }
      const receipt = {
        sequence: uploads.length + 1,
        filename,
        bytes,
        sha256: createHash("sha256").update(Buffer.concat(chunks)).digest("hex"),
        contentType: request.headers["content-type"] || "application/octet-stream",
        receivedUtc: new Date().toISOString(),
      };
      uploads.push(receipt);
      response.setHeader("Content-Type", "application/json; charset=utf-8");
      response.end(JSON.stringify(receipt));
    } catch {
      if (!response.headersSent) response.writeHead(400).end();
    }
  });
  server.requestTimeout = 10_000;
  server.headersTimeout = 10_000;
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(port, "127.0.0.1", () => {
      server.off("error", reject);
      origin = `http://127.0.0.1:${server.address().port}`;
      resolve();
    });
  });
  let closed = false;
  return {
    pid: process.pid,
    page: `${origin}/`,
    state: `${origin}/state`,
    maxBytes: MAX_FIXTURE_UPLOAD_BYTES,
    close: async () => {
      if (closed) return;
      closed = true;
      await closeServer(server);
    },
  };
}

async function main() {
  const fixture = await startAppWebviewUploadFixture({ port: parseFixturePort(process.env.GROK_CU_UPLOAD_FIXTURE_PORT) });
  const { close, ...description } = fixture;
  process.stdout.write(`${JSON.stringify(description)}\n`);
  let closing = false;
  const shutdown = async code => {
    if (closing) return;
    closing = true;
    await close();
    process.exit(code);
  };
  process.stdin.setEncoding("utf8");
  process.stdin.on("data", input => { if (input.trim() === "quit") void shutdown(0); });
  process.stdin.once("end", () => { void shutdown(0); });
  process.once("SIGINT", () => { void shutdown(130); });
  process.once("SIGTERM", () => { void shutdown(143); });
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch(error => {
    process.stderr.write(`${error instanceof Error ? error.stack : String(error)}\n`);
    process.exitCode = 1;
  });
}
