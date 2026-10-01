// Real session MCP child -> App IPC -> Broker adapter -> MV3 -> owned fixture.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath } from "node:url";

export function sessionClient(binding) {
  const child = spawn(process.execPath, [fileURLToPath(new URL("../computer-use-mcp/server.mjs", import.meta.url))], {
    windowsHide: true, stdio: ["pipe", "pipe", "pipe"],
    env: { ...process.env, GROK_APP_CU_IPC: binding.endpoint,
      GROK_APP_CU_TOKEN: binding.token, GROK_APP_CU_SESSION: binding.session },
  });
  child.stderr.on("data", () => {}); // Never forward possible credential-bearing diagnostics.
  const pending = new Map(); let sequence = 0;
  const lines = createInterface({ input: child.stdout });
  const fail = () => { for (const waiter of pending.values()) waiter.reject(new Error("fixture MCP exited")); pending.clear(); };
  child.once("error", fail); child.once("exit", fail);
  lines.on("line", line => {
    const reply = JSON.parse(line);
    const waiter = pending.get(reply.id);
    if (!waiter) return;
    pending.delete(reply.id);
    if (reply.error) waiter.reject(new Error("fixture MCP protocol error"));
    else waiter.resolve(reply.result);
  });
  async function rpc(method, params = {}) {
    const id = ++sequence; let timer;
    const result = new Promise((resolve, reject) => {
      pending.set(id, { resolve, reject });
      timer = setTimeout(() => { pending.delete(id); reject(new Error("fixture MCP timeout")); }, 16000);
    });
    child.stdin.write(JSON.stringify({ jsonrpc: "2.0", id, method, params }) + "\n");
    try { return await result; } finally { clearTimeout(timer); }
  }
  return {
    rpc, call: (name, args = {}) => rpc("tools/call", { name, arguments: args }),
    async close() {
      child.stdin.end();
      if (child.exitCode === null && child.signalCode === null) {
        const timer = setTimeout(() => child.kill(), 3000);
        await new Promise(resolve => child.once("exit", resolve)); clearTimeout(timer);
      }
      lines.close();
    },
  };
}
function decoded(reply) {
  assert.equal(reply.isError, false, "MCP tool must succeed");
  return JSON.parse(reply.content.find(item => item.type === "text").text);
}

// Both requests must cross the production MCP child and /cu/tool transport. The
// independent fixture counter, not an MCP success envelope, proves the effect.
export async function verifyMcpClickOnce({ client, args, effectCount, stage, passed }) {
  stage("mcp-act-through-existing-adapter");
  assert.equal(await effectCount(), 0, "fixture must start without an action effect");
  const clicked = decoded(await client.call("computer_act", args));
  assert.equal(clicked.kind, "applied");
  assert.equal(clicked.executed, true);
  assert.equal(await effectCount(), 1, "the MCP action must change the owned page once");
  passed("mcp-act-through-existing-adapter");

  stage("mcp-act-action-id-dedupe");
  const duplicate = decoded(await client.call("computer_act", args));
  assert.deepEqual(duplicate, clicked);
  assert.equal(await effectCount(), 1, "retrieving the action result must not click again");
  passed("mcp-act-action-id-dedupe");
}

// A rejected read-only observation can wait for the Host's explicit admission
// deadline once. Never infer backoff from arbitrary text, and never retry an
// action or an unknown/started operation. This is probe load scheduling only.
export async function observeWithAdmissionBackoff(client) {
  const reply = await client.call("computer_observe", { screenshot: false });
  if (reply.isError === true && reply.code === "rate_limited" &&
      reply.completion === "not_started" && Number.isInteger(reply.retryAfterMs) &&
      reply.retryAfterMs > 0 && reply.retryAfterMs <= 2000) {
    await delay(reply.retryAfterMs);
    return client.call("computer_observe", { screenshot: false });
  }
  return reply;
}

export async function verifyExistingMcp({ worker, fixture, fixtureId, selector, rpc, until, stage, passed }) {
  const binding = await rpc("mcp-open", { selector });
  const client = sessionClient(binding); let other;
  const snapshot = () => worker.evaluate(async tabId => {
    const [result] = await chrome.scripting.executeScript({ target: { tabId, frameIds: [0] }, world: "ISOLATED",
      func: () => ({ snapshotId: globalThis.__grokComputerUseSnapshot?.snapshotId,
        count: globalThis.__grokComputerUseSnapshot?.refs.size }) });
    return result.result;
  }, fixtureId);
  let checkpoint = "initialize";
  try {
    stage("mcp-observation-through-existing-adapter");
    assert.equal((await client.rpc("initialize")).serverInfo.name, "grok-computer-use");
    checkpoint = "catalog";
    const catalog = await client.rpc("tools/list");
    assert(!catalog.tools.some(tool => /authorize|reconnect|resume/.test(tool.name)));
    checkpoint = "status";
    assert.equal(decoded(await client.call("computer_status")).backend, "chromium-extension");
    checkpoint = "targets";
    const listed = await client.call("computer_list_targets");
    if (listed.isError) process.stderr.write("MCP existing target diagnostic: toolError=true\n");
    const targets = decoded(listed);
    if (targets.length !== 1 || targets[0]?.targetId !== `tab:${fixtureId}`) {
      process.stderr.write(`MCP existing target diagnostic: count=${targets.length}, fixtureMatch=${targets.some(row => row.targetId === `tab:${fixtureId}`)}\n`);
    }
    assert.equal(targets.length, 1); assert.equal(targets[0].targetId, `tab:${fixtureId}`);
    checkpoint = "observe-reply";
    const reply = await client.call("computer_observe", { screenshot: false });
    const observed = decoded(reply);
    checkpoint = "observe-content";
    assert.equal(observed.targetId, `tab:${fixtureId}`);
    assert(observed.text.includes("Visible fixture 你好"));
    assert(observed.nodes.some(node => node.name === "Fixture action"));
    assert(!reply.content.some(item => item.type === "image"));
    for (const secret of ["VALUE_SECRET", "PASSWORD_SECRET", "PASSWORD_VALUE", "HIDDEN_SECRET", "TEXTAREA_SECRET"]) {
      assert(!JSON.stringify(reply).includes(secret));
    }
    checkpoint = "observe-snapshot";
    assert.equal((await snapshot()).snapshotId, observed.snapshotId);
    passed("mcp-observation-through-existing-adapter");

    checkpoint = "act-click";
    await fixture.evaluate(() => {
      const button = document.querySelector("#fixture-action");
      if (!button) throw new Error("fixture action button missing");
      globalThis.__cuClickCount = 0;
      button.addEventListener("click", () => { globalThis.__cuClickCount += 1; });
    });
    const clickArgs = {
      version: 1,
      actionId: "mcp-existing-click-1",
      runId: binding.run,
      targetId: observed.targetId,
      targetGeneration: observed.targetGeneration,
      snapshotId: observed.snapshotId,
      geometryRevision: observed.geometryRevision,
      action: "click",
      target: { elementRef: observed.nodes.find(node => node.name === "Fixture action").ref },
      parameters: {},
    };
    await verifyMcpClickOnce({
      client, args: clickArgs,
      effectCount: () => fixture.evaluate(() => globalThis.__cuClickCount),
      stage: name => { checkpoint = name; stage(name); }, passed,
    });

    stage("mcp-act-rejects-coordinate-and-stale-snapshot");
    checkpoint = "act-rejections";
    const coordinate = {
      ...clickArgs,
      actionId: "mcp-existing-coordinate-rejected",
      target: { x: 1, y: 1 },
    };
    const coordinateReply = await client.call("computer_act", coordinate);
    assert.equal(coordinateReply.isError, true);
    const coordinateOutcome = JSON.parse(coordinateReply.content.find(item => item.type === "text").text);
    assert.equal(coordinateOutcome.kind, "rejected");
    assert.equal(coordinateOutcome.executed, false);
    const staleReply = await client.call("computer_act", {
      ...clickArgs,
      actionId: "mcp-existing-stale-rejected",
      snapshotId: "00000000-0000-4000-8000-000000000099",
    });
    assert.equal(staleReply.isError, true);
    const staleOutcome = JSON.parse(staleReply.content.find(item => item.type === "text").text);
    assert.equal(staleOutcome.kind, "rejected");
    assert.equal(staleOutcome.executed, false);
    assert.equal(await fixture.evaluate(() => globalThis.__cuClickCount), 1);
    passed("mcp-act-rejects-coordinate-and-stale-snapshot");

    stage("mcp-existing-session-isolation");
    checkpoint = "session-isolation";
    assert.equal((await rpc("mcp-open", { secondary: true })).issued, false);
    // A valid token from session A cannot be relabelled as session B in MCP env.
    other = sessionClient({ ...binding, session: "extension-mcp-other-owner" });
    assert.equal((await other.call("computer_list_targets")).isError, true);
    assert.equal((await other.call("computer_open_target", { targetId: observed.targetId })).isError, true);
    assert.equal((await other.call("computer_observe", { screenshot: false })).isError, true);
    assert.equal((await other.call("computer_authorize", { targetId: observed.targetId })).isError, true);
    assert.equal((await snapshot()).snapshotId, observed.snapshotId);
    await other.close(); other = null;
    passed("mcp-existing-session-isolation");

    stage("mcp-preview-keeps-model-refs");
    checkpoint = "preview-refs";
    const previewModel = decoded(await client.call("computer_observe", { screenshot: false }));
    const saved = await snapshot();
    assert(saved.count > 0);
    assert.equal(saved.snapshotId, previewModel.snapshotId);
    // Real UI preview requests an image; Chrome denies capture without a toolbar gesture.
    // Even this rejected preview must leave the existing model refs untouched.
    const preview = await rpc("mcp-preview");
    const afterPreview = await snapshot();
    assert.equal(preview.ok, false);
    assert.deepEqual(afterPreview, saved);
    assert.equal((await rpc("mcp-state")).snapshot, previewModel.snapshotId);
    passed("mcp-preview-keeps-model-refs");

    stage("mcp-stop-cancels-existing-observation");
    checkpoint = "stop-observation-dispatch";
    await worker.evaluate(() => { globalThis.__cuPauseObservation = true; });
    let earlyReply;
    const pending = observeWithAdmissionBackoff(client).then(reply => {
      earlyReply = reply;
      return reply;
    });
    await until(async () => {
      if (earlyReply) {
        const texts = earlyReply.content?.filter(item => item.type === "text").map(item => item.text) ?? [];
        // Classify the transport failure without logging page content or credentials.
        process.stderr.write(`MCP stop admission: settledBeforeBarrier=true, toolError=${earlyReply.isError === true}, rateLimited=${texts.includes("rate limited")}, slotsFull=${texts.includes("too many pending calls")}\n`);
        throw new Error("fixture observation settled before its dispatch barrier");
      }
      return worker.evaluate(() => typeof globalThis.__cuReleaseObservation === "function");
    });
    checkpoint = "stop-command";
    const started = Date.now();
    decoded(await client.call("computer_stop"));
    checkpoint = "stop-observation-joined";
    assert.equal((await pending).isError, true);
    assert(Date.now() - started < 1500, "stop must cancel before the extension 5-second or Host 10-second timeout");
    checkpoint = "stop-state";
    const state = await rpc("mcp-state");
    assert.equal(state.borrowed, 0); assert.equal(state.idle, true); assert.equal(state.stop, "stopped");
    await worker.evaluate(() => { globalThis.__cuReleaseObservation(); delete globalThis.__cuReleaseObservation; });
    assert.equal((await client.call("computer_observe", { screenshot: false })).isError, true);
    assert(!fixture.isClosed(), "stop must return the user's tab, never close it");
    process.stderr.write(`MCP stop: cancelledObservationMs=${Date.now() - started}, tabOpen=true\n`);
    passed("mcp-stop-cancels-existing-observation");
  } catch (error) {
    // Field-level location only: assertion values or MCP payloads may contain credentials/content.
    process.stderr.write(`MCP existing probe failed: checkpoint=${checkpoint}\n`);
    throw error;
  } finally {
    await worker.evaluate(() => { globalThis.__cuReleaseObservation?.(); delete globalThis.__cuReleaseObservation; }).catch(() => {});
    await other?.close(); await client.close();
  }
}
