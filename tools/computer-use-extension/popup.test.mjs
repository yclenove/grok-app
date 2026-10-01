import { test } from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { createRequire } from "node:module";
const require = createRequire(new URL("../../package.json", import.meta.url));
const { JSDOM } = require("jsdom");
const markup = await readFile(new URL("./popup.html", import.meta.url), "utf8");
const script = await readFile(new URL("./popup.js", import.meta.url), "utf8");
async function until(predicate) {
  for (let i = 0; i < 50; i++) {
    if (predicate()) return;
    await new Promise(resolve => setTimeout(resolve, 5));
  }
  assert.fail("popup postcondition timeout");
}

function fixture() {
  const dom = new JSDOM(markup, { runScripts: "outside-only" });
  const state = { paired: true, shared: false, active: 101, failShare: false };
  const calls = []; const listeners = {};
  dom.window.chrome = {
    i18n: { getMessage: key => key, getUILanguage: () => "en" },
    runtime: { id: "fixture", onMessage: { addListener: fn => { listeners.message = fn; } }, async sendMessage(message) {
      calls.push(message);
      if (message.type === "cu-status") return { ok: true, paired: state.paired };
      if (message.type === "cu-tab-state") return { ok: true, paired: state.paired, shared: state.shared, tabId: state.active, title: "fixture" };
      if (message.type === "cu-share-current") {
        if (state.failShare) { state.paired = false; return { ok: false, error: "shareUnavailable" }; }
        state.shared = true;
        return { ok: true, shared: true, tabId: message.tabId };
      }
      if (message.type === "cu-unshare-current") { state.shared = false; return { ok: true, shared: false }; }
      if (message.type === "cu-forget") { state.paired = false; return { ok: true, paired: false }; }
      return { ok: false, error: "pairingRejected" };
    } },
    tabs: { onActivated: { addListener: fn => { listeners.activated = fn; } },
      onUpdated: { addListener: fn => { listeners.updated = fn; } } },
  };
  dom.window.eval(script);
  const find = id => dom.window.document.getElementById(id);
  return { dom, state, calls, find, listeners };
}

test("share updates its own status and does not turn a successful pairing into unpaired", async () => {
  const f = fixture();
  try {
    await until(() => !f.find("share-current").disabled);
    f.find("share-current").click();
    await until(() => f.find("tab-status").textContent === "sharedCurrentTab");
    await until(() => !f.find("unshare-current").disabled);
    assert.equal(f.find("status").dataset.state, "paired");
    assert(f.find("share-current").disabled);
    assert.deepEqual(f.calls.find(call => call.type === "cu-share-current").tabId, 101);
    f.find("unshare-current").click();
    await until(() => !f.find("share-current").disabled);
    assert.equal(f.find("tab-status").textContent, "unsharedCurrentTab");
    assert.equal(f.find("status").dataset.state, "paired");
  } finally { f.dom.window.close(); }
});

test("share failure and revoked pairing leave controls disabled", async () => {
  const f = fixture();
  try {
    await until(() => !f.find("share-current").disabled);
    f.state.failShare = true; f.find("share-current").click();
    await until(() => f.find("status").dataset.state === "unpaired");
    assert(f.find("share-current").disabled);
    assert(f.find("unshare-current").disabled);
    assert.equal(f.find("tab-status").dataset.state, "shareUnavailable");
  } finally { f.dom.window.close(); }
});

test("popup selection updates on activation instead of reusing the previous tab ID", async () => {
  const f = fixture();
  try {
    await until(() => !f.find("share-current").disabled);
    f.state.active = 202; f.listeners.activated();
    await until(() => f.calls.filter(call => call.type === "cu-tab-state").length >= 2);
    await new Promise(resolve => setTimeout(resolve, 0));
    f.find("share-current").click();
    await until(() => f.find("tab-status").textContent === "sharedCurrentTab");
    assert.equal(f.calls.find(call => call.type === "cu-share-current").tabId, 202);
  } finally { f.dom.window.close(); }
});

test("worker revocation disables an already open popup without a tab change or another click", async () => {
  const f = fixture();
  try {
    await until(() => !f.find("share-current").disabled);
    f.find("share-current").click();
    await until(() => !f.find("unshare-current").disabled);
    assert.equal(f.find("tab-status").dataset.state, "shared");
    f.state.paired = false;
    f.listeners.message?.({ type: "cu-state-changed" }, { id: "fixture" });
    await until(() => f.find("status").dataset.state === "unpaired");
    assert(f.find("share-current").disabled);
    assert(f.find("unshare-current").disabled);
    assert.equal(f.find("tab-title").textContent, "");
    assert.equal(f.find("tab-status").dataset.state, "");
    assert.equal(f.find("tab-status").textContent, "",
      "revocation cannot leave a successful sharing message on screen");
    assert.equal(f.calls.filter(call => call.type === "cu-status").length, 1,
      "local status notifications must not create network polling");
  } finally { f.dom.window.close(); }
});
