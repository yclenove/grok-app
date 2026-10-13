// Original Node MCP process; a local HTTP fixture, not native/user consent.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { once } from "node:events";
import { createInterface } from "node:readline";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

async function fixture(t, handler) {
  const requests = [];
  const server = createServer(async (request, response) => {
    let data = "";
    for await (const chunk of request) data += chunk;
    const body = JSON.parse(data);
    requests.push({ path: request.url, body });
    handler(body, response);
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const child = spawn(process.execPath, [fileURLToPath(new URL("./server.mjs", import.meta.url))], {
    windowsHide: true,
    stdio: ["pipe", "pipe", "pipe"],
    env: { ...process.env, GROK_APP_CU_IPC: `http://127.0.0.1:${server.address().port}`,
      GROK_APP_CU_TOKEN: "synthetic-local-transport", GROK_APP_CU_SESSION: "fixture-session" },
  });
  let stderr = "";
  child.stderr.on("data", chunk => { stderr += chunk; });
  const lines = createInterface({ input: child.stdout });
  const replies = [];
  lines.on("line", line => replies.push(JSON.parse(line)));
  const exited = once(child, "exit");
  t.after(async () => {
    child.stdin.end();
    server.closeAllConnections();
    await new Promise(resolve => server.close(resolve));
    const timeout = setTimeout(() => child.kill(), 5000);
    await exited;
    clearTimeout(timeout);
    assert.equal(stderr, "");
  });
  const send = value => child.stdin.write(`${JSON.stringify(value)}\n`);
  const wait = async (predicate, timeout = 5000) => {
    const deadline = Date.now() + timeout;
    while (!predicate()) {
      assert(Date.now() < deadline, "original MCP operation did not reach the expected state");
      await new Promise(resolve => setTimeout(resolve, 10));
    }
  };
  return { requests, replies, send, wait, child, exited };
}

const call = (id, name = "computer_upload") => ({ jsonrpc: "2.0", id, method: "tools/call",
  params: { name, arguments: { version: 1, runId: "original-run", targetId: "original-target" } } });
const cancel = requestId => ({ jsonrpc: "2.0", method: "notifications/cancelled", params: { requestId } });
const success = { isError: false, content: [{ type: "text", text: "original-result" }] };

test("upload uses its own route and survives the original 15 second idle budget", { timeout: 25000 }, async t => {
  const f = await fixture(t, (_, response) => {
    response.writeHead(200, { "content-type": "application/json" });
    response.write(" ");
    const heartbeat = setInterval(() => response.write(" "), 1000);
    const terminal = setTimeout(() => response.end(JSON.stringify(success)), 16000);
    response.on("close", () => { clearInterval(heartbeat); clearTimeout(terminal); });
  });
  const began = Date.now();
  f.send(call(1));
  await f.wait(() => f.replies.length === 1, 21000);
  assert(Date.now() - began >= 16000);
  assert.equal(f.requests[0].path, "/cu/upload");
  assert.deepEqual(f.replies[0].result, success);
  assert.equal(f.requests.length, 1, "no model polling or new conversation");
});

test("cancel ids are exact and cannot pause another request or overwrite an original", async t => {
  const responses = [];
  const f = await fixture(t, (_, response) => {
    responses.push(response);
    response.writeHead(200, { "content-type": "application/json" });
    response.write(" ");
  });
  f.send(call(7)); f.send(call("7"));
  await f.wait(() => f.requests.length === 2);
  f.send(call(7)); // A duplicate may not replace the first call's canceller.
  await f.wait(() => f.replies.some(reply => reply.error));
  f.send(cancel("missing"));
  f.send(cancel(7));
  await f.wait(() => f.replies.some(reply => reply.id === 7 && reply.result));
  assert.equal(f.replies.find(reply => reply.id === 7 && reply.result).result.isError, true);
  assert.equal(f.replies.some(reply => reply.id === "7"), false);
  responses[1].end(JSON.stringify(success));
  await f.wait(() => f.replies.some(reply => reply.id === "7"));
  assert.deepEqual(f.replies.find(reply => reply.id === "7").result, success);
  f.send(cancel(7)); f.send(cancel("7"));
  f.send({ jsonrpc: "2.0", id: 9, method: "ping" });
  await f.wait(() => f.replies.some(reply => reply.id === 9));
  assert.equal(f.requests.length, 2);
  assert(!f.requests.some(request => request.body.name === "computer_request_handoff"));
});

test("EOF retires original calls without a broad successor handoff", async t => {
  let closed = false;
  const f = await fixture(t, (_, response) => {
    response.writeHead(200, { "content-type": "application/json" });
    response.write(" ");
    response.on("close", () => { closed = true; });
  });
  f.send(call("original"));
  await f.wait(() => f.requests.length === 1);
  f.child.stdin.end();
  await f.wait(() => closed);
  await f.exited;
  assert.equal(f.requests.length, 1);
  assert.equal(f.replies.length, 0);
});

test("notifications and malformed ids cannot dispatch actions; normal tools keep the normal route", async t => {
  const f = await fixture(t, (_, response) => response.end(JSON.stringify(success)));
  f.send({ ...call(null), id: null });
  f.send(null);
  f.send([]);
  f.send(call({ unrelated: true }));
  f.send(call(12, "computer_observe"));
  await f.wait(() => f.replies.some(reply => reply.id === 12));
  assert.equal(f.requests.length, 1);
  assert.equal(f.requests[0].path, "/cu/tool");
  assert.equal(f.replies.find(reply => reply.id === null).error.code, -32600);
});
