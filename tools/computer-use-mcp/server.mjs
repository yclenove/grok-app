#!/usr/bin/env node
/** Session MCP transport. Auth is supplied by the Host, never read from user accounts. */
import { request } from "node:http";
import { createInterface } from "node:readline";
import { TOOLS } from "./protocol.mjs";

const IPC = process.env.GROK_APP_CU_IPC || "";
const TOKEN = process.env.GROK_APP_CU_TOKEN || "";
const SESSION = process.env.GROK_APP_CU_SESSION || "";
let endpoint;
try {
  endpoint = new URL(IPC);
  if (endpoint.protocol !== "http:" || endpoint.hostname !== "127.0.0.1" ||
      !endpoint.port || endpoint.username || endpoint.password || !TOKEN || !SESSION) throw new Error();
} catch {
  process.stderr.write("Computer Use requires a Host-issued local session connection.\n");
  process.exit(1);
}

function send(message) { process.stdout.write(JSON.stringify(message) + "\n"); }
function failure(message) { return { isError: true, content: [{ type: "text", text: message }] }; }

function hostCall(name, args = {}) {
  return new Promise((resolve, reject) => {
    const data = Buffer.from(JSON.stringify({ session: SESSION, name, arguments: args }));
    const req = request({
      hostname: endpoint.hostname, port: endpoint.port, path: "/cu/tool", method: "POST",
      headers: { "content-type": "application/json", "content-length": data.length, authorization: `Bearer ${TOKEN}` },
    }, (res) => {
      let size = 0;
      const chunks = [];
      res.on("data", (chunk) => {
        size += chunk.length;
        if (size > 16 * 1024 * 1024) { req.destroy(new Error("observation exceeds transport limit")); return; }
        chunks.push(chunk);
      });
      res.on("error", reject);
      res.on("end", () => {
        try { resolve(JSON.parse(Buffer.concat(chunks).toString("utf8"))); }
        catch { reject(new Error(`Host response could not be decoded (HTTP ${res.statusCode})`)); }
      });
    });
    req.setTimeout(15_000, () => req.destroy(new Error("Host call timed out; action outcome unknown")));
    req.on("error", reject);
    req.end(data);
  });
}

async function handle(message) {
  if (message.method === "initialize") {
    return send({ jsonrpc: "2.0", id: message.id, result: {
      protocolVersion: message.params?.protocolVersion || "2024-11-05",
      capabilities: { tools: { listChanged: false } },
      serverInfo: { name: "grok-computer-use", version: "1" },
      instructions: "Use only the target the user selected. Observe before acting, then verify. Copy observation IDs. Screenshots are image content; without an image, do not use pixel coordinates. Never infer success from transport OK. An unknown result requires a new observation; do not retry the same side-effecting actionId. For login, permissions or a new target, request handoff. computer_stop ends the run; only the user can resume.",
    }});
  }
  if (message.method === "notifications/initialized") return;
  if (message.method === "notifications/cancelled") {
    await hostCall("computer_request_handoff");
    return;
  }
  if (message.method === "ping") return send({ jsonrpc: "2.0", id: message.id, result: {} });
  if (message.method === "tools/list") return send({ jsonrpc: "2.0", id: message.id, result: { tools: TOOLS } });
  if (message.method === "tools/call") {
    const name = message.params?.name;
    const result = TOOLS.some((t) => t.name === name)
      ? await hostCall(name, message.params?.arguments || {}).catch((e) => failure(e.message))
      : failure("Tool is not available in this session");
    return send({ jsonrpc: "2.0", id: message.id, result });
  }
  if (message.id != null) send({ jsonrpc: "2.0", id: message.id, error: { code: -32601, message: "method not found" } });
}

const input = createInterface({ input: process.stdin, crlfDelay: Infinity });
input.on("line", (line) => {
  if (!line.trim()) return;
  if (Buffer.byteLength(line) > 64 * 1024) {
    send({ jsonrpc: "2.0", id: null, error: { code: -32600, message: "request too large" } });
    return;
  }
  let message;
  try { message = JSON.parse(line); }
  catch { send({ jsonrpc: "2.0", id: null, error: { code: -32700, message: "invalid JSON" } }); return; }
  void handle(message).catch(() => {
    if (message.id != null) send({ jsonrpc: "2.0", id: message.id, result: failure("Computer Use connection failed") });
  });
});
input.on("close", () => { void hostCall("computer_request_handoff").catch(() => {}); });
