import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { createObservationState, invalidateObservationState } from "./observation-state.mjs";
import {
  BASIC_ACT_KINDS,
  KEY_MAX_LEN,
  SCROLL_DELTA_MAX,
  SCROLL_DELTA_MIN,
  SPACE_ACT_KINDS,
  TEXT_MAX_CHARS,
  isDummyElementRef,
  preflightTypedAct,
  prepareTypedAct,
} from "./typed-act.mjs";

const catalog = JSON.parse(
  readFileSync(
    join(dirname(fileURLToPath(import.meta.url)), "../computer-use-protocol/golden/catalog.json"),
    "utf8",
  ),
);

function fakeHandle(init = {}) {
  const state = {
    value: init.value ?? "",
    tag: init.tag ?? "input",
    type: init.type ?? "",
    id: init.id ?? "",
    nameAttr: init.nameAttr ?? "",
    role: init.role ?? "",
    connected: init.connected !== false,
    visibleName: init.visibleName ?? "",
    hidden: false,
    scrollTop: 0,
  };
  const calls = [];
  const element = {
    get tagName() {
      return state.tag.toUpperCase();
    },
    get value() {
      return state.value;
    },
    set value(next) {
      state.value = String(next);
    },
    get isConnected() {
      return state.connected;
    },
    get disabled() {
      return false;
    },
    get labels() {
      return [];
    },
    get textContent() {
      return state.visibleName;
    },
    get isContentEditable() {
      return false;
    },
    get tabIndex() {
      return 0;
    },
    getAttribute(name) {
      if (name === "type") return state.type;
      if (name === "id") return state.id;
      if (name === "name") return state.nameAttr;
      if (name === "role") return state.role;
      return "";
    },
    closest() {
      return null;
    },
    checkVisibility() {
      return !state.hidden;
    },
    getClientRects() {
      return { length: 1 };
    },
    get scrollTop() {
      return state.scrollTop;
    },
    set scrollTop(value) {
      state.scrollTop = Number(value) || 0;
    },
    scrollBy(_x, y) {
      state.scrollTop += Number(y) || 0;
    },
  };
  return {
    state,
    calls,
    async click() {
      calls.push(["click"]);
    },
    async fill(value) {
      calls.push(["fill", value]);
      state.value = String(value);
    },
    async type(value) {
      calls.push(["type", value]);
      state.value += String(value);
    },
    async pressSequentially(value) {
      calls.push(["pressSequentially", value]);
      state.value += String(value);
    },
    async selectOption(value) {
      calls.push(["selectOption", value]);
      if (state.tag !== "select") throw new Error("Element is not a <select> element");
      state.value = String(value);
    },
    async press(key) {
      calls.push(["press", key]);
    },
    async focus() {
      calls.push(["focus"]);
    },
    async evaluate(fn, arg) {
      return fn(element, arg);
    },
    async hover() {
      calls.push(["hover"]);
    },
    async dragTo(dest) {
      calls.push(["dragTo", dest]);
      if (typeof init.onDrag === "function") init.onDrag(dest);
    },
    async boundingBox() {
      return { x: init.x ?? 0, y: init.y ?? 0, width: 16, height: 16 };
    },
  };
}

function signatureFor(init = {}) {
  const tag = init.tag ?? "input";
  const type = init.type ?? "";
  let role = (init.role ?? "").trim().split(/\s+/)[0].toLowerCase();
  if (!role) {
    if (tag === "button") role = "button";
    else if (tag === "select") role = "combobox";
    else if (tag === "option") role = "option";
    else if (tag === "textarea") role = "textbox";
    else if (tag === "input") role = "textbox";
    else role = "generic";
  }
  return [tag, type, role, init.id ?? "", init.nameAttr ?? ""].join("|");
}

function observationFor(handle, init, role, name) {
  return createObservationState(3, [
    {
      target: { handle, signature: signatureFor(init) },
      role,
      name,
    },
  ]);
}

test("Node/Rust golden lockstep for kinds and text/key caps", () => {
  assert.equal(TEXT_MAX_CHARS, catalog.textMaxChars);
  assert.equal(KEY_MAX_LEN, catalog.keyMaxLen);
  assert.deepEqual(BASIC_ACT_KINDS, ["click", "set_value", "type_text", "select", "key"]);
  for (const kind of BASIC_ACT_KINDS) {
    assert.equal(catalog.workerActionKinds.includes(kind), true, kind);
  }
  assert.equal(catalog.actionKinds.includes("select"), false);
  assert.equal(catalog.actionKinds.includes("click"), true);
  assert.equal(SCROLL_DELTA_MIN, catalog.scrollDeltaMin);
  assert.equal(SCROLL_DELTA_MAX, catalog.scrollDeltaMax);
  assert.deepEqual(SPACE_ACT_KINDS, ["scroll", "drag"]);
  assert.equal(isDummyElementRef("dummy"), true);
  assert.equal(isDummyElementRef("element-1"), false);
});

test("identity matrix rejects missing snapshot/ref and dummy key refs before dispatch", async () => {
  const handle = fakeHandle({ tag: "button" });
  const observation = observationFor(handle, { tag: "button" }, "button", "+1");
  const page = { keyboard: { press: async () => handle.calls.push(["page.press"]) }, isClosed: () => false };

  await assert.rejects(
    () =>
      prepareTypedAct({
        page,
        observation,
        kind: "click",
        body: {
          pageGeneration: 3,
          snapshotId: observation.snapshotId,
          actionId: "a1",
          role: "button",
          name: "+1",
        },
      }),
    (error) => error.workerCode === "element_ref_required" && error.completion === "not_started",
  );
  await assert.rejects(
    () =>
      prepareTypedAct({
        page,
        observation,
        kind: "set_value",
        body: {
          pageGeneration: 3,
          elementRef: observation.nodes[0].elementRef,
          actionId: "a2",
          text: "x",
        },
      }),
    (error) => error.workerCode === "snapshot_required",
  );
  await assert.rejects(
    () =>
      prepareTypedAct({
        page,
        observation,
        kind: "key",
        body: { pageGeneration: 3, snapshotId: observation.snapshotId, actionId: "a3", elementRef: "dummy", key: "enter" },
      }),
    (error) => error.workerCode === "dummy_element_ref" && error.completion === "not_started",
  );
  await assert.rejects(
    () =>
      prepareTypedAct({
        page,
        observation,
        kind: "key",
        body: { pageGeneration: 3, actionId: "a4", key: "enter" },
      }),
    (error) => error.workerCode === "snapshot_required",
  );
  assert.deepEqual(handle.calls, []);
});

test("set_value replaces, type_text appends, select has no fill fallback", async () => {
  const input = fakeHandle({ tag: "input", value: "keep" });
  const inputObs = observationFor(input, { tag: "input" }, "textbox", "Name");
  const select = fakeHandle({ tag: "select", value: "red" });
  const selectObs = observationFor(select, { tag: "select" }, "combobox", "color");
  const page = { keyboard: { press: async () => {} }, isClosed: () => false };

  const replace = await prepareTypedAct({
    page,
    observation: inputObs,
    kind: "set_value",
    body: {
      pageGeneration: 3,
      snapshotId: inputObs.snapshotId,
      elementRef: inputObs.nodes[0].elementRef,
      actionId: "set-1",
      text: "ab",
    },
  });
  await replace.dispatch();
  assert.equal(input.state.value, "ab");
  assert.equal(input.calls.some((row) => row[0] === "fill"), true);
  assert.equal(input.calls.some((row) => row[0] === "selectOption"), false);

  input.calls.length = 0;
  const typed = await prepareTypedAct({
    page,
    observation: inputObs,
    kind: "type_text",
    body: {
      pageGeneration: 3,
      snapshotId: inputObs.snapshotId,
      elementRef: inputObs.nodes[0].elementRef,
      actionId: "type-1",
      text: "cd",
    },
  });
  await typed.dispatch();
  assert.equal(input.state.value, "abcd");
  assert.equal(input.calls.some((row) => row[0] === "fill"), false);
  assert.equal(input.calls.some((row) => row[0] === "type"), true);

  await assert.rejects(
    () =>
      prepareTypedAct({
        page,
        observation: inputObs,
        kind: "select",
        body: {
          pageGeneration: 3,
          snapshotId: inputObs.snapshotId,
          elementRef: inputObs.nodes[0].elementRef,
          actionId: "sel-bad",
          value: "blue",
        },
      }),
    (error) => error.workerCode === "select_target_invalid" && error.completion === "not_started",
  );
  assert.equal(input.state.value, "abcd");

  const selected = await prepareTypedAct({
    page,
    observation: selectObs,
    kind: "select",
    body: {
      pageGeneration: 3,
      snapshotId: selectObs.snapshotId,
      elementRef: selectObs.nodes[0].elementRef,
      actionId: "sel-ok",
      value: "blue",
    },
  });
  await selected.dispatch();
  assert.equal(select.state.value, "blue");
  assert.equal(select.calls.some((row) => row[0] === "selectOption"), true);
  assert.equal(select.calls.some((row) => row[0] === "fill"), false);

  await assert.rejects(
    () =>
      prepareTypedAct({
        page,
        observation: selectObs,
        kind: "set_value",
        body: {
          pageGeneration: 3,
          snapshotId: selectObs.snapshotId,
          elementRef: selectObs.nodes[0].elementRef,
          actionId: "set-sel",
          text: "blue",
        },
      }),
    (error) => error.workerCode === "set_value_not_supported_on_select",
  );

  const pagePresses = [];
  const keyPage = {
    keyboard: { press: async (key) => pagePresses.push(key) },
    isClosed: () => false,
  };
  const keyed = await prepareTypedAct({
    page: keyPage,
    observation: inputObs,
    kind: "key",
    body: {
      pageGeneration: 3,
      snapshotId: inputObs.snapshotId,
      actionId: "key-page",
      key: "enter",
    },
  });
  await keyed.dispatch();
  assert.deepEqual(pagePresses, ["Enter"]);
  assert.equal(input.calls.some((row) => row[0] === "press" && row[1] === "Enter"), false);
});

test("scroll and drag use snapshot identity and reject selector destinations", async () => {
  const scroller = fakeHandle({ tag: "div", role: "generic" });
  scroller.state.tag = "div";
  const scrollObs = observationFor(scroller, { tag: "div", role: "generic" }, "generic", "scroller");
  const source = fakeHandle({ tag: "div", role: "generic" });
  const dest = fakeHandle({ tag: "div", role: "generic" });
  const dragObs = createObservationState(3, [
    { target: { handle: source, signature: signatureFor({ tag: "div", role: "generic" }) }, role: "generic", name: "Drag" },
    { target: { handle: dest, signature: signatureFor({ tag: "div", role: "generic" }) }, role: "generic", name: "Drop" },
  ]);
  const page = {
    keyboard: { press: async () => {} },
    isClosed: () => false,
    viewportSize: () => ({ width: 800, height: 600 }),
    evaluate: async (fn, arg) => fn({ scrollBy() {} }, arg),
    mouse: { wheel: async () => {}, move: async () => {}, down: async () => {}, up: async () => {} },
  };

  await assert.rejects(
    () =>
      prepareTypedAct({
        page,
        observation: scrollObs,
        kind: "scroll",
        body: { pageGeneration: 3, actionId: "s0", delta: 200 },
      }),
    (error) => error.workerCode === "snapshot_required",
  );
  await assert.rejects(
    () =>
      prepareTypedAct({
        page,
        observation: dragObs,
        kind: "drag",
        body: {
          pageGeneration: 3,
          snapshotId: dragObs.snapshotId,
          elementRef: dragObs.nodes[0].elementRef,
          actionId: "d0",
          toSelector: "#drop",
        },
      }),
    (error) => error.workerCode === "selector_forbidden" && error.completion === "not_started",
  );
  assert.deepEqual(source.calls, []);

  const scrolled = await prepareTypedAct({
    page,
    observation: scrollObs,
    kind: "scroll",
    body: {
      pageGeneration: 3,
      snapshotId: scrollObs.snapshotId,
      elementRef: scrollObs.nodes[0].elementRef,
      actionId: "s1",
      delta: 180,
    },
  });
  await scrolled.dispatch();
  assert.equal(scroller.state.scrollTop, 180);

  const dragEvents = [];
  for (const name of ["move", "down", "up"]) {
    page.mouse[name] = async (...args) => { dragEvents.push([name, ...args]); };
  }
  const dragged = await prepareTypedAct({
    page,
    observation: dragObs,
    kind: "drag",
    body: {
      pageGeneration: 3,
      snapshotId: dragObs.snapshotId,
      elementRef: dragObs.nodes[0].elementRef,
      destElementRef: dragObs.nodes[1].elementRef,
      actionId: "d1",
    },
  });
  await dragged.dispatch();
  assert.equal(dragEvents.filter(row => row[0] === "down").length, 1);
  assert.equal(dragEvents.filter(row => row[0] === "up").length, 1);
  assert.equal(source.calls.some(row => row[0] === "dragTo"), false);
});

test("wait is read-only nameEquals and reload forbids elementRef", async () => {
  const button = fakeHandle({ tag: "button", visibleName: "+1" });
  const observation = observationFor(button, { tag: "button" }, "button", "+1");
  const reloads = [];
  const page = {
    keyboard: { press: async () => {} },
    isClosed: () => false,
    reload: async (options) => {
      reloads.push(options);
    },
  };

  const matched = await prepareTypedAct({
    page,
    observation,
    kind: "wait",
    body: {
      pageGeneration: 3,
      snapshotId: observation.snapshotId,
      elementRef: observation.nodes[0].elementRef,
      nameEquals: "+1",
      timeoutMs: 200,
    },
  });
  await matched.dispatch();
  assert.equal(button.calls.some((row) => row[0] === "click"), false);

  await assert.rejects(
    () =>
      prepareTypedAct({
        page,
        observation,
        kind: "wait",
        body: {
          pageGeneration: 3,
          snapshotId: observation.snapshotId,
          elementRef: observation.nodes[0].elementRef,
          nameEquals: "never-this",
          timeoutMs: 80,
        },
      }).then((prepared) => prepared.dispatch()),
    (error) => error.workerCode === "wait_timeout" && error.completion === "not_started",
  );

  await assert.rejects(
    () =>
      prepareTypedAct({
        page,
        observation,
        kind: "reload",
        body: {
          pageGeneration: 3,
          actionId: "rel-dummy",
          elementRef: "dummy",
        },
      }),
    (error) => error.workerCode === "element_ref_forbidden",
  );
  const reloaded = await prepareTypedAct({
    page,
    observation,
    kind: "reload",
    body: { pageGeneration: 3, actionId: "rel-1" },
  });
  await reloaded.dispatch();
  assert.deepEqual(reloads, [{ waitUntil: "domcontentloaded", timeout: 8000 }]);
});

test("wait revalidates the original handle, visibility, signature, snapshot and cancellation", async (t) => {
  for (const kind of ["name", "removed", "hidden", "signature", "snapshot", "cancelled"]) {
    await t.test(kind, async () => {
      const button = fakeHandle({ tag: "button", visibleName: "Waiting" });
      const observation = observationFor(button, { tag: "button" }, "button", "Waiting");
      const cancellation = new AbortController();
      const prepared = await prepareTypedAct({
        page: {}, observation, kind: "wait", signal: cancellation.signal,
        body: { pageGeneration: 3, snapshotId: observation.snapshotId,
          elementRef: observation.nodes[0].elementRef, nameEquals: "Ready", timeoutMs: 1000 },
      });
      let entered;
      const polling = new Promise((resolve) => { entered = resolve; });
      const evaluate = button.evaluate.bind(button);
      button.evaluate = async (...args) => {
        const result = await evaluate(...args);
        if (result && typeof result === "object" && "name" in result) entered();
        return result;
      };
      const pending = prepared.dispatch().then(() => ({ ok: true }), (error) => ({ error }));
      await polling;
      if (kind === "name") button.state.visibleName = "Ready";
      if (kind === "removed") button.state.connected = false;
      if (kind === "hidden") button.state.hidden = true;
      if (kind === "signature") button.state.id = "replacement";
      if (kind === "snapshot") invalidateObservationState(observation);
      if (kind === "cancelled") cancellation.abort();
      const result = await pending;
      if (kind === "name") assert.equal(result.ok, true);
      else if (kind === "cancelled") assert.match(result.error.message, /cancelled/);
      else assert.equal(result.error.workerCode, {
        removed: "stale_element_ref", hidden: "wait_target_hidden",
        signature: "stale_element_signature", snapshot: "observation_required",
      }[kind]);
      assert.deepEqual(button.calls, []);
    });
  }
});

test("preflightTypedAct enforces text and key caps", () => {
  const body = {
    pageGeneration: 1,
    snapshotId: "snapshot-1",
    elementRef: "element-1",
    actionId: "a",
  };
  assert.equal(preflightTypedAct("click", body), "click");
  assert.throws(
    () => preflightTypedAct("type_text", { ...body, text: "x".repeat(TEXT_MAX_CHARS + 1) }),
    (error) => error.workerCode === "invalid_text",
  );
  assert.throws(
    () => preflightTypedAct("key", { pageGeneration: 1, snapshotId: "s", actionId: "a", key: "alt+f4" }),
    (error) => error.workerCode === "invalid_key",
  );
});
