// Fixed synthetic MCP data. No network, shell, desktop, or user file access.
import { appendFileSync } from "node:fs";
import { createInterface } from "node:readline";

const log = process.argv[2];
if (!log) throw new Error("owned fixture log required");
const tool = "consent_fixed_data";
for await (const line of createInterface({ input: process.stdin })) {
  let message;
  try { message = JSON.parse(line); } catch { continue; }
  if (message.id == null) continue;
  let result;
  if (message.method === "initialize") {
    result = { protocolVersion: "2024-11-05", capabilities: { tools: {} }, serverInfo: { name: "consent-fixture", version: "1" } };
  } else if (message.method === "tools/list") {
    result = { tools: [{ name: tool, description: "Return fixed synthetic test data. No real side effects.", inputSchema: { type: "object", properties: { sequence: { type: "integer", enum: [1, 2] } }, required: ["sequence"], additionalProperties: false } }] };
  } else if (message.method === "tools/call" && message.params?.name === tool && [1, 2].includes(message.params?.arguments?.sequence)) {
    const sequence = message.params.arguments.sequence;
    appendFileSync(log, JSON.stringify({ sequence, pid: process.pid, decisionClass: "synthetic-test-data" }) + "\n");
    result = { content: [{ type: "text", text: `synthetic fixed data ${sequence}` }] };
  } else if (message.method === "ping") {
    result = {};
  } else {
    process.stdout.write(JSON.stringify({ jsonrpc: "2.0", id: message.id, error: { code: -32601, message: "fixture method unavailable" } }) + "\n");
    continue;
  }
  process.stdout.write(JSON.stringify({ jsonrpc: "2.0", id: message.id, result }) + "\n");
}
