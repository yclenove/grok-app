import { test } from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { randomUUID } from "node:crypto";
import { observeDocument } from "./observe-document.mjs";
import { actDocument, cancelDocumentOperation } from "./act-document.mjs";
import { fenceDocument } from "./document-execution.mjs";
import { withObservationClock } from "../computer-use-probe/dom-fixture-clock.mjs";
const require = createRequire(new URL("../../package.json", import.meta.url));
const { JSDOM, VirtualConsole } = require("jsdom");

function fixture(html = "<input aria-label='Field'><button>Run</button>") {
  const virtualConsole = new VirtualConsole();
  virtualConsole.on("jsdomError", error => { throw error; });
  const dom = new JSDOM(html, { url: "https://fixture.invalid/", pretendToBeVisual: true, runScripts: "outside-only", virtualConsole });
  const win = dom.window;
  const closeWindow = win.close.bind(win);
  // jsdom.close() skips pagehide. Retire the fixture before destroying its realm.
  win.close = () => { win.__grokComputerUseSnapshot?.retire(); closeWindow(); };
  win.Element.prototype.getBoundingClientRect = () => ({ width: 100, height: 30, top: 10, left: 10, right: 110, bottom: 40 });
  win.document.elementFromPoint = () => win.document.querySelector("button,a");
  const call = (fn, ...args) => {
    const invoke = () => win.eval(`(${fn.toString()})(...${JSON.stringify(args)})`);
    return fn === observeDocument ? withObservationClock(win, invoke) : invoke();
  };
  let sequence = 1;
  call(fenceDocument, { epoch: 1, sequence });
  const read = (retain = true) => call(observeDocument, randomUUID(), retain, { epoch: 1, sequence: ++sequence });
  const make = (observation, action, parameters, index = 0) => ({ operationId: randomUUID(),
    snapshotId: observation.snapshotId, elementRef: observation.nodes[index].elementRef,
    action, parameters, deadlineMs: Date.now() + 10000 });
  return { dom, win, call, read, make, act: command => call(actDocument, command) };
}

test("SetValue and TypeText preserve CJK/emoji, selection, native setters and input events without returning values", async () => {
  const f = fixture();
  try {
    const field = f.win.document.querySelector("input"); const events = [];
    for (const name of ["beforeinput", "input", "change"]) field.addEventListener(name, e => events.push([e.type, e.inputType]));
    // A framework's instance setter must not swallow the native field update.
    Object.defineProperty(field, "value", { configurable: true, get() {
      return Object.getOwnPropertyDescriptor(f.win.HTMLInputElement.prototype, "value").get.call(this);
    }, set() { throw new Error("instance setter must not be used"); } });
    let result = await f.act(f.make(f.read(), "set_value", { text: "你好🌍" }));
    assert.equal(result.status, "verified"); assert.equal(field.value, "你好🌍");
    assert.deepEqual(events.map(e => e[0]), ["beforeinput", "input", "change"]);
    assert(!JSON.stringify(result).includes("你好"));
    field.setSelectionRange(2, 4); events.length = 0;
    result = await f.act(f.make(f.read(), "type_text", { text: "世界🚀" }));
    assert.equal(result.status, "verified"); assert.equal(field.value, "你好世界🚀");
    assert.deepEqual(events.map(e => e[0]), ["beforeinput", "input"]);
    assert.equal(field.selectionStart, field.value.length);
    assert(!JSON.stringify(f.read()).includes("你好世界"));
  } finally { f.dom.window.close(); }
});

test("a semantic click dispatches once, is not self-verified, and consumes the model snapshot", async () => {
  const f = fixture("<button>Run</button>");
  try {
    let clicks = 0; f.win.document.querySelector("button").addEventListener("click", () => clicks++);
    const command = f.make(f.read(), "click", {});
    const first = await f.act(command);
    assert.equal(first.status, "applied"); assert.equal(clicks, 1);
    assert.equal((await f.act(command)).status, "rejected"); assert.equal(clicks, 1);
  } finally { f.dom.window.close(); }
});

test("a delayed observation cannot recreate the same retired snapshot and reactivate its old action", async () => {
  const f = fixture("<button>Run</button>");
  try {
    let clicks = 0; f.win.document.querySelector("button").addEventListener("click", () => clicks++);
    const identity = { epoch: 1, sequence: 2 };
    const observed = f.call(observeDocument, randomUUID(), true, identity);
    const command = f.make(observed, "click", {});
    await f.call(cancelDocumentOperation, command.snapshotId, command.operationId);
    assert.throws(() => f.call(observeDocument, command.snapshotId, true, identity), /execution|stale/);
    assert.equal((await f.act(command)).status, "rejected"); assert.equal(clicks, 0);
  } finally { f.dom.window.close(); }
});

test("same-ID replacement, disable/enable and read-only changes reject without input", async () => {
  for (const mutation of [
    field => { field.outerHTML = "<input id='field' aria-label='Field'>"; },
    field => { field.disabled = true; field.disabled = false; },
    field => { field.readOnly = true; },
    field => { field.type = "password"; field.type = "text"; },
  ]) {
    const f = fixture("<input id='field' aria-label='Field'>");
    try {
      const command = f.make(f.read(), "set_value", { text: "forbidden" });
      mutation(f.win.document.querySelector("input"));
      assert.equal((await f.act(command)).status, "rejected");
      assert.equal(f.win.document.querySelector("input").value, "");
    } finally { f.dom.window.close(); }
  }
});

test("prevented beforeinput and reentrant value changes do not get overwritten or reported verified", async () => {
  for (const change of [e => e.preventDefault(), e => { e.target.value = "handler-owned"; }]) {
    const f = fixture();
    try {
      const field = f.win.document.querySelector("input"); field.addEventListener("beforeinput", change);
      const result = await f.act(f.make(f.read(), "set_value", { text: "agent-input" }));
      assert.equal(result.status, "unknown"); assert.notEqual(field.value, "agent-input");
    } finally { f.dom.window.close(); }
  }
});

test("a ref made stale by focus or input handlers cannot be filled or declared verified", async () => {
  for (const event of ["focus", "input"]) {
    const f = fixture();
    try {
      const field = f.win.document.querySelector("input");
      field.addEventListener(event, () => { field.remove(); f.win.document.body.prepend(field); });
      const result = await f.act(f.make(f.read(), "set_value", { text: "attempt" }));
      assert.equal(result.status, "unknown");
      if (event === "focus") assert.equal(field.value, "");
    } finally { f.dom.window.close(); }
  }
});

test("Wait follows the original name, is single flight, preserves refs and cannot repeat an operation ID", async () => {
  const f = fixture("<button>Pending</button>");
  try {
    const observed = f.read(); const state = f.win.__grokComputerUseSnapshot;
    const command = f.make(observed, "wait", { nameEquals: "Ready", timeoutMs: 1000 });
    const waiting = f.act(command);
    assert.equal((await f.act(f.make(observed, "click", {}))).detail, "document_busy");
    assert.throws(() => f.read(), /busy/);
    f.read(false);
    assert.equal(f.win.__grokComputerUseSnapshot, state);
    f.win.document.querySelector("button").textContent = "Ready";
    assert.equal((await waiting).status, "verified");
    assert.equal(f.win.__grokComputerUseSnapshot, state);
    assert.equal(state.retired, false); assert.equal(state.operation, null);
    assert.equal((await f.act(command)).detail, "duplicate_operation");
    assert.equal((await f.act(f.make(observed, "click", {}))).status, "rejected", "changed-name ref cannot click without observe");
  } finally { f.dom.window.close(); }
});

test("Wait timeout and explicit cancel settle their owned timer before completion", async () => {
  const f = fixture("<button>Pending</button>");
  try {
    const timed = f.make(f.read(), "wait", { nameEquals: "Ready", timeoutMs: 5 });
    assert.equal((await f.act(timed)).status, "rejected");
    assert.equal(f.win.__grokComputerUseSnapshot.operation, null);
    const command = f.make(f.read(), "wait", { nameEquals: "Ready", timeoutMs: 10000 });
    const waiting = f.act(command);
    const cancel = await f.call(cancelDocumentOperation, command.snapshotId, command.operationId);
    assert.equal(cancel.status, "settled"); assert.equal((await waiting).detail, "cancelled");
    assert.equal(f.win.__grokComputerUseSnapshot.operation, null);
    assert.equal(f.win.__grokComputerUseSnapshot.retired, true);
  } finally { f.dom.window.close(); }
});

test("cancellation before entry fences a late script, but another operation cannot cancel an active Wait", async () => {
  const f = fixture("<button>Pending</button>");
  try {
    let command = f.make(f.read(), "click", {});
    assert.equal((await f.call(cancelDocumentOperation, command.snapshotId, command.operationId)).status, "settled");
    assert.equal((await f.act(command)).status, "rejected");
    command = f.make(f.read(), "wait", { nameEquals: "Ready", timeoutMs: 10000 });
    const waiting = f.act(command);
    assert.equal((await f.call(cancelDocumentOperation, command.snapshotId, randomUUID())).status, "operation_mismatch");
    await f.call(cancelDocumentOperation, command.snapshotId, command.operationId);
    await waiting;
  } finally { f.dom.window.close(); }
});

test("Wait rejects a replacement, removed/reinserted ref or view change even when its new name matches", async () => {
  for (const change of [
    f => { f.win.document.querySelector("button").outerHTML = "<button>Ready</button>"; },
    f => { const b = f.win.document.querySelector("button"); b.remove(); f.win.document.body.append(b); b.textContent = "Ready"; },
    f => { f.win.document.querySelector("button").textContent = "Ready"; f.win.dispatchEvent(new f.win.Event("resize")); },
  ]) {
    const f = fixture("<button>Pending</button>");
    try {
      const waiting = f.act(f.make(f.read(), "wait", { nameEquals: "Ready", timeoutMs: 1000 }));
      change(f); assert.equal((await waiting).status, "rejected");
    } finally { f.dom.window.close(); }
  }
});

test("unimplemented trusted keyboard, coordinates, clipboard and double-click stay rejected", async () => {
  const f = fixture("<button>Run</button>");
  try {
    let clicks = 0; f.win.document.querySelector("button").addEventListener("click", () => clicks++);
    const observation = f.read();
    for (const [action, parameters] of [["key", { key: "enter" }], ["click", { count: 2 }],
      ["click", { button: "right" }], ["type_text", { text: "hi", via: "clipboard" }], ["scroll", { delta: 2401 }]]) {
      assert.equal((await f.act(f.make(observation, action, parameters))).status, "rejected");
    }
    assert.equal((await f.act({ ...f.make(observation, "click", {}), x: 1 })).status, "rejected");
    assert.equal(clicks, 0);
  } finally { f.dom.window.close(); }
});

test("malformed, script, credential-bearing, download and new-window links never dispatch a click", async () => {
  for (const html of ["<a href='http://['>Run</a>", "<a href='javascript:void(0)'>Run</a>",
    "<a href='https://name:secret@fixture.invalid/'>Run</a>", "<a href='/file' download>Run</a>",
    "<a href='/other' target='_blank'>Run</a>", "<head><base target='_blank'></head><a href='/other'>Run</a>"]) {
    const f = fixture(html);
    try {
      let clicks = 0; f.win.document.querySelector("a").addEventListener("click", () => clicks++);
      const result = await f.act(f.make(f.read(), "click", {}));
      assert.equal(result.status, "rejected"); assert.equal(clicks, 0);
      assert(!JSON.stringify(result).includes("secret"));
    } finally { f.dom.window.close(); }
  }
});

test("associated form and base-target changes away and back retire the old reference", async () => {
  for (const mutation of [
    document => { const form = document.querySelector("form"); form.action = "/changed"; form.action = "/submit"; },
    document => { const base = document.querySelector("base"); base.target = "_blank"; base.target = "_self"; },
  ]) {
    const f = fixture("<head><base target='_self'></head><form id='form' action='/submit'></form><button form='form'>Submit</button>");
    try {
      const command = f.make(f.read(), "click", {});
      mutation(f.win.document);
      assert.equal((await f.act(command)).status, "rejected");
    } finally { f.dom.window.close(); }
  }
});

test("new model observation releases old refs; mutation storms retire instead of retaining unbounded work", async () => {
  const f = fixture("<button>Run</button>");
  try {
    f.read(); const old = f.win.__grokComputerUseSnapshot; f.read();
    assert(old.retired); assert.equal(old.refs.size, 0);
    const state = f.win.__grokComputerUseSnapshot; const button = f.win.document.querySelector("button");
    for (let i = 0; i < 5000; i++) button.setAttribute("id", String(i));
    await Promise.resolve(); assert(state.retired); assert.equal(state.refs.size, 0);
  } finally { f.dom.window.close(); }
});
