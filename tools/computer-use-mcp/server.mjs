#!/usr/bin/env node
/** Session MCP transport. Auth is supplied by the Host, never read from user accounts. */
import { request } from "node:http";
import { createInterface } from "node:readline";
import { TOOLS } from "./protocol.mjs";

const IPC = process.env.GROK_APP_CU_IPC || "";
const TOKEN = process.env.GROK_APP_CU_TOKEN || "";
const SESSION = process.env.GROK_APP_CU_SESSION || "";
const activeCalls = new Map();
let inputClosed = false;
let endpoint;
try {
  endpoint = new URL(IPC);
  if (endpoint.protocol !== "http:" || endpoint.hostname !== "127.0.0.1" ||
      !endpoint.port || endpoint.username || endpoint.password || !TOKEN || !SESSION) throw new Error();
} catch {
  process.stderr.write("Computer Use requires a Host-issued local session connection.\n");
  process.exit(1);
}

function send(message) { if (!inputClosed) process.stdout.write(JSON.stringify(message) + "\n"); }
function failure(message) { return { isError: true, content: [{ type: "text", text: message }] }; }

function hostCall(name, args = {}, signal) {
  return new Promise((resolve, reject) => {
    const data = Buffer.from(JSON.stringify({ session: SESSION, name, arguments: args }));
    const req = request({
      hostname: endpoint.hostname, port: endpoint.port,
      path: name === "computer_upload" ? "/cu/upload" : "/cu/tool", method: "POST", signal,
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
      res.on("aborted", () => reject(new Error("Host response interrupted; action outcome unknown")));
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
      instructions: "When the task needs computer access, call computer_request_access once with the purpose, necessary surfaces, capabilities and scope intent. Copy the returned taskId. A pending response grants no access: stop dispatching computer tools and end the turn while awaiting the Host decision. Do not poll for approval or repeat the user's task. Only a committed Host decision permits execution within its approved scope. For additional scope, request only the additional access; a denial grants no access to that scope. Observe before acting, then verify. Copy observation IDs. Screenshots are image content; without an image, do not use pixel coordinates. Never infer success from transport OK. An unknown result requires a fresh observation; do not replay the side-effecting action. For an observed App WebView file input, computer_upload waits for the user to choose a file in the original task. Never supply local paths or bytes, poll for the choice, or approve it yourself. Upload verification is field assignment, not website submission; observe and verify the website separately. For login or system permission UI, request handoff. After verifying the task result, call computer_complete_task with the original taskId before the final reply. This revokes all task scopes; its acknowledgement does not prove physical cleanup or business success. Waiting for approval, clarification or handoff is not task completion. computer_stop ends a run; only the user can resume.",
    }});
  }
  if (message.method === "notifications/initialized") return;
  if (message.method === "notifications/cancelled") {
    // Only the original HTTP call is ours to retire. A broad handoff could
    // pause a successor task, and closing a socket is not physical completion.
    activeCalls.get(message.params?.requestId)?.abort();
    return;
  }
  if (message.method === "ping") return send({ jsonrpc: "2.0", id: message.id, result: {} });
  if (message.method === "tools/list") return send({ jsonrpc: "2.0", id: message.id, result: { tools: TOOLS } });
  if (message.method === "tools/call") {
    if (message.id == null) return; // A notification cannot start a model action.
    if (!((typeof message.id === "string" && message.id.length <= 256) ||
      (typeof message.id === "number" && Number.isSafeInteger(message.id)))) {
      return send({ jsonrpc: "2.0", id: null, error: { code: -32600, message: "invalid request id" } });
    }
    if (activeCalls.has(message.id)) {
      return send({ jsonrpc: "2.0", id: message.id, error: { code: -32600, message: "request id is already pending" } });
    }
    const name = message.params?.name;
    const controller = new AbortController();
    activeCalls.set(message.id, controller);
    try {
      const result = TOOLS.some((t) => t.name === name)
        ? await hostCall(name, message.params?.arguments ?? {}, controller.signal)
          .catch((e) => failure(controller.signal.aborted
            ? "Original Computer Use request cancelled; action outcome unknown"
            : e.message))
        : failure("Tool is not available in this session");
      return send({ jsonrpc: "2.0", id: message.id, result });
    } finally {
      if (activeCalls.get(message.id) === controller) activeCalls.delete(message.id);
    }
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
  if (!message || typeof message !== "object" || Array.isArray(message)) {
    send({ jsonrpc: "2.0", id: null, error: { code: -32600, message: "request must be an object" } });
    return;
  }
  void handle(message).catch(() => {
    if (message.id != null) send({ jsonrpc: "2.0", id: message.id, result: failure("Computer Use connection failed") });
  });
});
input.on("close", () => {
  inputClosed = true;
  for (const controller of activeCalls.values()) controller.abort();
  // The Host owns task/session shutdown. EOF must not pause an unrelated
  // successor task or claim the original native owner has physically ended.
});
