import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { withObservationClock } from "./dom-fixture-clock.mjs";

const require = createRequire(new URL("../../package.json", import.meta.url));
const { JSDOM, VirtualConsole } = require("jsdom");
const kernel = readFileSync(new URL("../../src-tauri/src/computer_use/webview/dom.js", import.meta.url), "utf8");
const downloadPolyfill = readFileSync(new URL("../../src-tauri/src/side_browser_blob/polyfill.js", import.meta.url), "utf8")
  .replace("__GROK_SBDL_LABEL__", JSON.stringify("resource-browser-download-fixture"));
const owner = "owned-native-binding";
const snapshot = n => `wvsnap:00000000-0000-4000-8000-${String(n).padStart(12, "0")}`;
function fixture(html) {
  const console = new VirtualConsole();
  console.on("jsdomError", error => { throw error; });
  const dom = new JSDOM(html, { url: "https://fixture.invalid/", pretendToBeVisual: true, runScripts: "outside-only", virtualConsole: console });
  const { window } = dom;
  window.Element.prototype.getBoundingClientRect = () => ({ x: 10, y: 10, width: 100, height: 30, top: 10, left: 10, right: 110, bottom: 40 });
  // jsdom has no layout engine. Geometry and contenteditable reflection are fixture data.
  Object.defineProperty(window.HTMLElement.prototype, "isContentEditable", { get() { return this.closest("[contenteditable='true']") !== null; } });
  const call = (data, timed = false) => {
    const invoke = () => JSON.parse(JSON.stringify(window.eval(`(${kernel})(${JSON.stringify({ version: 1, owner, snapshot: snapshot(1), ...data })})`)));
    return data.kind === "observe" && !timed ? withObservationClock(window, invoke) : invoke();
  };
  return {
    window, call, read: (forModel = true, n = 1, timed = false) => call({ kind: "observe", forModel, snapshot: snapshot(n) }, timed),
    act: (node, data = {}) => call({ kind: "act", elementRef: node.elementRef, action: "click", ...data }),
    prepareDownload: (node, data = {}) => call({ kind: "download_prepare", elementRef: node.elementRef, ...data }),
    dispatchDownload: (node, data = {}) => call({ kind: "download_dispatch", elementRef: node.elementRef, ...data }),
    close() { window.__grokAppWebViewDom?.retire(); window.close(); },
  };
}

test("typed native download bypasses only its exact manual polyfill click and still consumes the ref", () => {
  const f = fixture('<a download="owned.bin" href="/owned.bin">Owned</a><a download="manual.bin" href="/manual.bin">Manual</a>');
  try {
    const requests = [];
    // Keep manual transfers pending; no network/body/timing is a fixture oracle.
    f.window.fetch = url => { requests.push(String(url)); return new Promise(() => {}); };
    f.window.eval(downloadPolyfill);
    const links = Array.from(f.window.document.querySelectorAll("a"));
    let native = 0;
    links[0].addEventListener("click", event => {
      assert.equal(event.defaultPrevented, false, "manual capture must not swallow the native event");
      event.preventDefault(); native++;
      // A nested click on another link keeps the ordinary manual path.
      links[1].click();
    });
    const observed = f.read();
    const owned = observed.nodes.find(node => node.name === "Owned");
    assert.equal(f.prepareDownload(owned).ok, true);
    assert.equal(f.dispatchDownload(owned).ok, true);
    assert.equal(native, 1);
    assert.deepEqual(requests, ["https://fixture.invalid/manual.bin"]);
    assert.equal(f.dispatchDownload(owned).ok, false);
    assert.equal(native, 1);
    assert.deepEqual(requests, ["https://fixture.invalid/manual.bin"]);
    assert.equal(links[0].hasAttribute("data-grok-sbdl"), false);
    assert.equal(links[1].getAttribute("data-grok-sbdl"), "1");
    assert.equal(links[0].hasAttribute("data-grok-native-download"), false);
    assert.equal(links[1].hasAttribute("data-grok-native-download"), false);
  } finally { f.close(); }
});

test("manual download interception is restored after a native click and its page handler fails", () => {
  const f = fixture('<a download="owned.bin" href="/owned.bin">Owned</a>');
  try {
    const requests = [];
    f.window.fetch = url => { requests.push(String(url)); return new Promise(() => {}); };
    // A rejected native click tests the Host kernel's finally boundary,
    // without swallowing a DOM handler exception or invoking a page global.
    f.window.HTMLElement.prototype.click = () => { throw new Error("page click failed"); };
    f.window.eval(downloadPolyfill);
    const link = f.window.document.querySelector("a");
    const owned = f.read().nodes.find(node => node.name === "Owned");
    assert.throws(() => f.dispatchDownload(owned), /page click failed/);
    assert.equal(link.hasAttribute("data-grok-native-download"), false);
    assert.equal(f.dispatchDownload(owned).ok, false);
    link.dispatchEvent(new f.window.MouseEvent("click", { bubbles:true, cancelable:true }));
    assert.deepEqual(requests, ["https://fixture.invalid/owned.bin"]);
  } finally { f.close(); }
});

test("controls without ids and duplicate ids have distinct opaque references and exact effects", () => {
  const f = fixture("<button>First</button><button id='duplicate'>Second</button><button id='duplicate'>Third</button>");
  try {
    const buttons = Array.from(f.window.document.querySelectorAll("button"));
    const effects = [0, 0, 0]; buttons.forEach((button, i) => button.onclick = () => effects[i]++);
    const result = f.read();
    assert.equal(result.nodes.length, 3);
    assert.equal(new Set(result.nodes.map(n => n.elementRef)).size, 3);
    assert(result.nodes.every(n => /^wvsnap:[0-9a-f-]{36}:\d+$/.test(n.elementRef)));
    assert(result.nodes.every(n => !n.elementRef.includes("duplicate")));
    assert.equal(f.act(result.nodes[2]).ok, true);
    assert.deepEqual(effects, [0, 0, 1]);
    assert.equal(f.act(result.nodes[2]).ok, false);
    assert.deepEqual(effects, [0, 0, 1]);
  } finally { f.close(); }
});

test("preview never replaces or retires a model observation", () => {
  const f = fixture("<button>Target</button>");
  try {
    const model = f.read(); const retained = f.window.__grokAppWebViewDom;
    const preview = f.read(false, 2);
    assert.equal(f.window.__grokAppWebViewDom, retained);
    assert.notEqual(preview.snapshot, model.snapshot);
    assert.equal(f.act(preview.nodes[0], { snapshot: preview.snapshot }).ok, false);
    assert.equal(f.act(model.nodes[0]).ok, true);
  } finally { f.close(); }
});

for (const mutation of ["replace", "reinsert", "identity-aba", "name-aba", "shadow-host-reinsert", "label-aba"]) {
  test(`old references reject ${mutation} without redirecting an action`, () => {
    const f = fixture("<label id='label'>Original label</label><div id='host'></div><button id='target' aria-labelledby='label'>Original</button>");
    try {
      let target = f.window.document.querySelector("button");
      const host = f.window.document.getElementById("host");
      if (mutation.startsWith("shadow")) host.attachShadow({ mode: "open" }).append(target);
      let effects = 0; target.onclick = () => effects++;
      const observed = f.read(); const node = observed.nodes.find(n => n.actions.includes("click"));
      assert(node);
      if (mutation === "replace") { const clone = target.cloneNode(true); clone.onclick = () => effects++; target.replaceWith(clone); }
      if (mutation === "reinsert") { target.remove(); f.window.document.body.append(target); }
      if (mutation === "identity-aba") { target.setAttribute("name", "changed"); target.removeAttribute("name"); }
      if (mutation === "name-aba") { const text = target.firstChild; text.textContent = "Changed"; text.textContent = "Original"; }
      if (mutation === "shadow-host-reinsert") { host.remove(); f.window.document.body.append(host); }
      if (mutation === "label-aba") { const text = f.window.document.getElementById("label").firstChild; text.textContent = "Changed label"; text.textContent = "Original label"; }
      assert.equal(f.act(node).ok, false);
      assert.equal(effects, 0);
    } finally { f.close(); }
  });
}

test("scroll resolves the selected real overflow container, not a fixture id or another region", () => {
  const f = fixture("<div aria-label='First region' style='overflow-y:auto'>One</div><div aria-label='Second region' style='overflow-y:auto'>Two</div>");
  try {
    const containers = Array.from(f.window.document.querySelectorAll("div"));
    containers.forEach(element => { Object.defineProperty(element, "scrollHeight", { value: 900 }); Object.defineProperty(element, "clientHeight", { value: 200 }); });
    const observed = f.read(); const selected = observed.nodes.find(n => n.name === "Second region");
    assert.deepEqual(selected.actions, ["scroll", "wait"]);
    assert.equal(f.act(selected, { action: "scroll", dy: 120 }).ok, true);
    assert.deepEqual(containers.map(element => element.scrollTop), [0, 120]);
  } finally { f.close(); }
});

test("input content is never observed and fill data is not executable source", () => {
  const f = fixture(`<label>Safe label <input value='VALUE_SECRET'></label><input type='password' aria-label='PASSWORD_SECRET'>
    <textarea>TEXTAREA_SECRET</textarea><div contenteditable='true'>EDITABLE_SECRET</div><div hidden>HIDDEN_SECRET</div>
    <div aria-hidden='true'>ARIA_SECRET</div><div style='opacity:0'>OPACITY_SECRET</div><p>Visible text</p>`);
  try {
    const observed = f.read(); const serialized = JSON.stringify(observed);
    for (const secret of ["VALUE_SECRET", "PASSWORD_SECRET", "TEXTAREA_SECRET", "EDITABLE_SECRET", "HIDDEN_SECRET", "ARIA_SECRET", "OPACITY_SECRET"]) assert(!serialized.includes(secret), secret);
    assert(observed.text.includes("Visible text"));
    const input = observed.nodes.find(n => n.name === "Safe label");
    const content = "你好');globalThis.injected=true;//";
    assert.equal(f.act(input, { action: "set_value", text: content }).ok, true);
    assert.equal(f.window.document.querySelector("input").value, content);
    assert.equal(f.window.injected, undefined);
    const next = f.read(true, 2);
    assert.equal(f.act(next.nodes.find(n => n.name === "Safe label"), { snapshot: next.snapshot, action: "set_value", text: "" }).ok, true);
    assert.equal(f.window.document.querySelector("input").value, "");
  } finally { f.close(); }
});

test("type_text inserts a literal at the retained selection and verifies input events", () => {
  const f = fixture("<label>备注 <input value='值尾'></label><div contenteditable='true'>Rich</div>");
  try {
    const input = f.window.document.querySelector("input");
    input.focus(); input.setSelectionRange(1, 1);
    const events = [];
    for (const type of ["beforeinput", "input", "change"]) input.addEventListener(type, event => {
      events.push([event.type, event.inputType ?? null, event.data ?? null]);
    });
    const observed = f.read();
    const field = observed.nodes.find(node => node.name === "备注");
    const rich = observed.nodes.find(node => node.name === "");
    assert(field.actions.includes("type_text"));
    assert(rich.actions.includes("set_value"));
    assert(!rich.actions.includes("type_text"));
    const text = "中文🙂');globalThis.injected=true;//";
    assert.deepEqual(f.act(field, { action: "type_text", text }), { ok: true, verified: true });
    assert.equal(input.value, `值${text}尾`);
    assert.equal(input.selectionStart, 1 + text.length);
    assert.equal(input.selectionEnd, 1 + text.length);
    assert.deepEqual(events, [
      ["beforeinput", "insertText", text],
      ["input", "insertText", text],
    ]);
    assert.equal(f.window.injected, undefined);
    assert.equal(f.act(field, { action: "type_text", text: "again" }).ok, false);
  } finally { f.close(); }
});

test("type_text consumes a prevented insertion without changing or exposing the field", () => {
  const f = fixture("<input aria-label='Safe field' value='private'>");
  try {
    const input = f.window.document.querySelector("input");
    input.focus(); input.setSelectionRange(7, 7);
    input.addEventListener("beforeinput", event => event.preventDefault());
    const field = f.read().nodes[0];
    const rejected = f.act(field, { action: "type_text", text: "追加" });
    assert.equal(rejected.ok, false);
    assert(!JSON.stringify(rejected).includes("private"));
    assert.equal(input.value, "private");
    assert.equal(f.act(field, { action: "type_text", text: "retry" }).ok, false);
  } finally { f.close(); }
});

test("wrong owner, snapshot, forged reference and unsupported action cause no effect", () => {
  const f = fixture("<button>Target</button>");
  try {
    let effects = 0; f.window.document.querySelector("button").onclick = () => effects++;
    const node = f.read().nodes[0];
    for (const data of [{ owner: "other" }, { snapshot: snapshot(2) }, { elementRef: `${snapshot(1)}:128` }, { action: "set_value", text: "wrong" }]) assert.equal(f.act(node, data).ok, false);
    assert.equal(effects, 0);
    assert.equal(f.act(node).ok, true);
    assert.equal(effects, 1);
  } finally { f.close(); }
});

for (const mutation of ["hidden", "disabled", "moved", "scroll", "new-observation"]) {
  test(`dispatch rejects ${mutation} after observation`, () => {
    const f = fixture("<button>Target</button>");
    try {
      const element = f.window.document.querySelector("button"); let effects = 0; element.onclick = () => effects++;
      const node = f.read().nodes[0];
      if (mutation === "hidden") element.hidden = true;
      if (mutation === "disabled") element.disabled = true;
      if (mutation === "moved") element.getBoundingClientRect = () => ({ x: 50, y: 10, width: 100, height: 30, top: 10, left: 50, right: 150, bottom: 40 });
      if (mutation === "scroll") f.window.dispatchEvent(new f.window.Event("scroll"));
      if (mutation === "new-observation") f.read(true, 2);
      assert.equal(f.act(node).ok, false); assert.equal(effects, 0);
    } finally { f.close(); }
  });
}

test("observation and mutation work are bounded", () => {
  const f = fixture(`<main>${"<button>Control</button>".repeat(150)}<p>${"text ".repeat(9000)}</p></main>`);
  try {
    const observed = f.read(); assert(observed.truncated); assert(observed.nodes.length <= 128); assert(observed.text.length <= 32000);
    assert(observed.nodes.length > 0);
    let ticks = 0; Object.defineProperty(f.window.performance, "now", { value: () => (ticks += 50) });
    const bounded = f.read(true, 2, true); assert(bounded.truncated); assert(bounded.nodes.length < 10);
  } finally { f.close(); }
});

test("cross-origin frames fail closed", () => {
  const f = fixture("<iframe src='https://other.invalid/'></iframe>");
  try { const result = f.read(); assert.equal(result.ok, false); assert.equal(result.reason, "unsupported_cross_origin"); }
  finally { f.close(); }
});

test("only exact retained HTTP downloads are published and dispatched once", () => {
  const f = fixture(`<a id="safe" download="report.bin" href="/report.bin">Safe download</a>
    <a id="blob" download href="blob:https://fixture.invalid/id">Blob download</a>
    <a id="data" download href="data:text/plain,secret">Data download</a>
    <a id="ordinary" href="/next">Ordinary link</a>`);
  try {
    const safe = f.window.document.getElementById("safe");
    let downloads = 0;
    safe.addEventListener("click", event => { event.preventDefault(); downloads++; });
    const observed = f.read();
    const serialized = JSON.stringify(observed);
    const node = observed.nodes.find(entry => entry.name === "Safe download");
    assert(node);
    assert.deepEqual(node.actions, ["download", "wait"]);
    assert.equal(serialized.includes("report.bin"), false, "href/download metadata must stay private");
    assert.equal(observed.nodes.some(entry => entry.name === "Blob download"), false);
    assert.equal(observed.nodes.some(entry => entry.name === "Data download"), false);
    assert(observed.nodes.find(entry => entry.name === "Ordinary link").actions.includes("click"));
    assert.deepEqual(f.prepareDownload(node), { ok: true, url: "https://fixture.invalid/report.bin" });
    safe.setAttribute("href", "/changed.bin");
    assert.equal(f.dispatchDownload(node).ok, false);
    assert.equal(downloads, 0);

    const refreshed = f.read(true, 2);
    const current = refreshed.nodes.find(entry => entry.name === "Safe download");
    assert.deepEqual(
      f.prepareDownload(current, { snapshot: refreshed.snapshot }),
      { ok: true, url: "https://fixture.invalid/changed.bin" },
    );
    assert.equal(f.dispatchDownload(current, { snapshot: refreshed.snapshot }).ok, true);
    assert.equal(downloads, 1);
    assert.equal(f.dispatchDownload(current, { snapshot: refreshed.snapshot }).ok, false);
    assert.equal(downloads, 1);
  } finally { f.close(); }
});

test("functional fixture clock is restored before actions, including an observation exception", () => {
  const f = fixture("<button>Target</button>");
  try {
    const original = f.window.performance.now;
    const descriptor = Object.getOwnPropertyDescriptor(f.window.performance, "now");
    f.read();
    assert.equal(f.window.performance.now, original);
    assert.deepEqual(Object.getOwnPropertyDescriptor(f.window.performance, "now"), descriptor);
    assert.throws(() => withObservationClock(f.window, () => {
      assert.equal(f.window.performance.now(), f.window.performance.now());
      throw new Error("fixture exception");
    }), /fixture exception/);
    assert.equal(f.window.performance.now, original);
    assert.deepEqual(Object.getOwnPropertyDescriptor(f.window.performance, "now"), descriptor);
  } finally { f.close(); }
});
