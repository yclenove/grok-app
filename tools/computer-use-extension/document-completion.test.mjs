import { test } from "node:test";
import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { runInNewContext } from "node:vm";
import { armDocumentCompletion } from "./document-completion.mjs";

function fixture() {
  const snapshotId = randomUUID(); const requestId = randomUUID(); const listeners = new Set(); const messages = [];
  let closed = false; let rejectSend = false;
  const terminal = {}; const token = "c".repeat(64);
  const state = { snapshotId, retired: false, operation: null,
    execution: { current: () => !closed, close: () => { closed = true; } }, retire() { this.retired = true; } };
  const world = { __grokComputerUseSnapshot: state, snapshotId, requestId, token,
    chrome: { storage: { session: { set: async value => { Object.assign(terminal, JSON.parse(JSON.stringify(value))); } } },
      runtime: { sendMessage: async message => { if (rejectSend) throw new Error("lost receipt"); messages.push(message); } } },
    addEventListener(type, handler) { assert.equal(type, "pagehide"); listeners.add(handler); },
    removeEventListener(type, handler) { assert.equal(type, "pagehide"); listeners.delete(handler); } };
  const arm = () => runInNewContext(`(${armDocumentCompletion.toString()})(snapshotId, requestId, token)`, world);
  const hide = (persisted = false, isTrusted = true) => { for (const listener of [...listeners]) listener({ persisted, isTrusted }); };
  return { world, arm, hide, listeners, state, requestId, snapshotId, messages, terminal, token, closed: () => closed,
    rejectSend: () => { rejectSend = true; } };
}
const turn = () => new Promise(resolve => setImmediate(resolve));

test("document guard ignores synthetic and cached pagehide; terminal receipt has no page data or cleanup key", async () => {
  const f = fixture(); assert.equal(f.arm().status, "armed");
  f.hide(false, false); f.hide(true); await turn();
  assert.equal(f.messages.length, 0); assert.equal(f.closed(), false); assert.equal(f.listeners.size, 1);
  assert.deepEqual(f.terminal, {});
  f.hide(); await turn();
  assert.equal(f.closed(), true); assert.equal(f.state.retired, true); assert.equal(f.listeners.size, 0);
  assert.deepEqual(JSON.parse(JSON.stringify(f.messages)), [{ type: "cu-document-finished", snapshotId: f.snapshotId, requestId: f.requestId }]);
  f.hide(); await turn(); assert.equal(f.messages.length, 1);
});

test("terminal guard waits for the actual original operation and refuses another operation's completion", async () => {
  const f = fixture(); f.arm(); let finish;
  f.state.operation = { id: f.requestId, finished: new Promise(resolve => { finish = resolve; }) };
  f.hide(); await turn(); assert.equal(f.state.retired, true); assert.equal(f.messages.length, 0);
  finish(); await turn(); assert.equal(f.messages.length, 1);
  const other = fixture(); other.arm(); other.state.operation = { id: randomUUID(), finished: Promise.resolve() };
  other.hide(); await turn(); assert.equal(other.messages.length, 0); assert.equal(other.closed(), false);
});

test("guard admission rejects stale/busy documents and replacements retain only one listener", () => {
  const f = fixture();
  f.state.retired = true; assert.throws(f.arm, /completionGuardUnavailable/); f.state.retired = false;
  f.state.operation = {}; assert.throws(f.arm, /completionGuardUnavailable/); f.state.operation = null;
  f.arm(); assert.equal(f.listeners.size, 1); f.arm(); assert.equal(f.listeners.size, 1);
  f.state.operation = {}; assert.throws(f.arm, /completionGuardUnavailable/); assert.equal(f.listeners.size, 1);
  f.state.operation = null; f.world.snapshotId = randomUUID(); assert.throws(f.arm, /completionGuardUnavailable/);
});

test("a lost terminal message retains only its one-use storage receipt without an unhandled rejection", async () => {
  const f = fixture(); f.arm(); f.rejectSend(); f.hide(); await turn();
  assert.equal(f.messages.length, 0); assert.equal(f.state.retired, true);
  assert.deepEqual(f.terminal, { ["cuDocumentFinished." + f.requestId]: { token: f.token } });
});
