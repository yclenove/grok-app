import { test } from "node:test";
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { observeDocument } from "./observe-document.mjs";
import { fenceDocument } from "./document-execution.mjs";
import { withObservationClock } from "../computer-use-probe/dom-fixture-clock.mjs";
const require = createRequire(new URL("../../package.json", import.meta.url));
const { JSDOM, VirtualConsole } = require("jsdom");
const snapshot = "00000000-0000-4000-8000-000000000001";
function fixture(html) {
  const virtualConsole = new VirtualConsole();
  virtualConsole.on("jsdomError", error => { throw error; });
  const dom = new JSDOM(html, { url: "https://fixture.invalid/", pretendToBeVisual: true, runScripts: "outside-only", virtualConsole });
  const closeWindow = dom.window.close.bind(dom.window);
  // jsdom.close() skips pagehide. Retire the fixture before destroying its realm.
  dom.window.close = () => { dom.window.__grokComputerUseSnapshot?.retire(); closeWindow(); };
  dom.window.Element.prototype.getBoundingClientRect = () => ({ x: 10, y: 10, width: 100, height: 30, top: 10, left: 10, right: 110, bottom: 40 });
  let sequence = 1;
  dom.window.eval(`(${fenceDocument.toString()})({epoch:1,sequence:1})`);
  return { dom, read: (retain = true, timed = false) => {
    const invoke = () => dom.window.eval(`(${observeDocument.toString()})(${JSON.stringify(snapshot)},${retain},{epoch:1,sequence:${++sequence}})`);
    return timed ? invoke() : withObservationClock(dom.window, invoke);
  } };
}
test("visible main-frame observation excludes passwords, field values, hidden descendants and opaque ancestors", () => {
  const f = fixture(`<title>Fixture</title><main><p>Visible text</p><button>Run fixture</button>
    <input value="VALUE_SECRET"><input type="password" aria-label="PASSWORD_SECRET" value="PASSWORD_VALUE">
    <input hidden value="HIDDEN_VALUE"><div hidden>HIDDEN_TEXT</div><div aria-hidden="true">ARIA_SECRET</div>
    <div style="opacity:0"><span>OPACITY_SECRET</span></div><div style="display:none">DISPLAY_SECRET</div>
    <textarea>TEXTAREA_SECRET</textarea><script>const SCRIPT_SECRET=1;</script>
    <iframe srcdoc="<p>FRAME_SECRET</p>"></iframe></main>`);
  try {
    const result = f.read(); const serialized = JSON.stringify(result);
    assert(result.text.includes("Visible text")); assert(result.nodes.some(n => n.name === "Run fixture"));
    for (const secret of ["VALUE_SECRET", "PASSWORD_SECRET", "PASSWORD_VALUE", "HIDDEN_VALUE", "HIDDEN_TEXT", "ARIA_SECRET",
      "OPACITY_SECRET", "DISPLAY_SECRET", "TEXTAREA_SECRET", "SCRIPT_SECRET", "FRAME_SECRET"]) assert(!serialized.includes(secret), secret);
    assert.equal(result.snapshotId, snapshot);
    assert(result.nodes.every(n => n.elementRef.startsWith(snapshot)));
  } finally { f.dom.window.close(); }
});
test("observation bounds node refs and visible text", () => {
  const f = fixture(`<main>${"<button>Action</button>".repeat(100)}<p>${"a".repeat(40000)}</p></main>`);
  try { const result = f.read(); assert.equal(result.nodes.length, 64); assert(result.text.length <= 32000); assert(result.truncated); }
  finally { f.dom.window.close(); }
});

test("exhausted observation time returns a truncated snapshot without walking the rest of the page", () => {
  const f = fixture(`<main>${"<button>Action</button>".repeat(1000)}</main>`);
  let ticks = 0;
  Object.defineProperty(f.dom.window.performance, "now", { value: () => (ticks += 50) });
  try {
    const result = f.read(true, true);
    assert(result.truncated);
    assert(result.nodes.length < 10, "elapsed-time budget must stop scanning before the node cap");
    assert(ticks > 0, "observer must actually consult the monotonic clock");
  } finally { f.dom.window.close(); }
});

test("a deeply nested hidden ancestor never leaks text even when the ancestor budget is exhausted", () => {
  const f = fixture(`<div hidden>${"<div>".repeat(160)}<button>DEEP_SECRET</button>${"</div>".repeat(160)}</div><button>Visible sibling</button>`);
  try {
    const result = f.read();
    assert(!JSON.stringify(result).includes("DEEP_SECRET"));
    assert(result.nodes.some(n => n.name === "Visible sibling"));
  } finally { f.dom.window.close(); }
});

test("UI preview does not replace the model's element references", () => {
  const f = fixture("<button>Current model control</button>");
  try {
    f.read();
    const current = f.dom.window.__grokComputerUseSnapshot;
    f.read(false);
    assert.equal(f.dom.window.__grokComputerUseSnapshot, current);
    assert(current.refs.size > 0);
  } finally { f.dom.window.close(); }
});

test("a removed and reinserted original element cannot regain observation authority", () => {
  const f = fixture("<button id='target'>Original</button>");
  try {
    const observed = f.read();
    const original = f.dom.window.document.querySelector("button");
    original.remove(); f.dom.window.document.body.append(original);
    const state = f.dom.window.__grokComputerUseSnapshot;
    assert.equal(typeof state.resolve, "function", "model refs need document-bound validation");
    assert.throws(() => state.resolve(observed.nodes[0].elementRef), /stale/);
  } finally { f.dom.window.close(); }
});

test("changing an element identity away and back invalidates its original reference", () => {
  const f = fixture("<a href='/first'>Original</a>");
  try {
    const observed = f.read(); const original = f.dom.window.document.querySelector("a");
    original.setAttribute("href", "/other"); original.setAttribute("href", "/first");
    const state = f.dom.window.__grokComputerUseSnapshot;
    assert.equal(typeof state.resolve, "function", "signature equality alone cannot detect ABA changes");
    assert.throws(() => state.resolve(observed.nodes[0].elementRef), /stale/);
  } finally { f.dom.window.close(); }
});
