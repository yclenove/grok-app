import test from "node:test";
import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import {
  OBSERVATION_LIMITS,
  PNG_SIGNATURE,
  captureManagedObservation,
  inspectPng,
  modelObservationView,
  normalizeInteractiveDescriptor,
  safeDisplayUrl,
} from "./observation-extract.mjs";
import {
  currentPageObservation,
  invalidatePageObservation,
  publishPageState,
  registerPageState,
  runPageMutation,
} from "./page-state.mjs";
import { resolveObservationTarget } from "./observation-state.mjs";

const catalog = JSON.parse(
  readFileSync(
    join(dirname(fileURLToPath(import.meta.url)), "../computer-use-protocol/golden/catalog.json"),
    "utf8",
  ),
);

test("observation limits stay locked to the Rust/Node golden catalog", () => {
  assert.equal(OBSERVATION_LIMITS.nodes, catalog.observationNodeCap);
  assert.equal(OBSERVATION_LIMITS.candidates, catalog.observationCandidateCap);
  assert.equal(OBSERVATION_LIMITS.roleChars, catalog.observationRoleChars);
  assert.equal(OBSERVATION_LIMITS.nameChars, catalog.observationNameChars);
  assert.equal(OBSERVATION_LIMITS.titleChars, catalog.observationTitleChars);
  assert.equal(OBSERVATION_LIMITS.urlChars, catalog.observationUrlChars);
  assert.equal(OBSERVATION_LIMITS.ariaChars, catalog.observationAriaChars);
  assert.equal(OBSERVATION_LIMITS.jsonBytes, catalog.observationJsonBytes);
  assert.equal(OBSERVATION_LIMITS.pngB64Chars, catalog.observationPngB64Cap);
});

class FakeHandle {
  constructor(descriptor) {
    this.descriptor = descriptor;
    this.connected = true;
    this.disposed = false;
    this.disposeCalls = 0;
  }

  async dispose() { this.disposeCalls += 1; this.disposed = true; }

  async evaluate(fn) {
    // The descriptor now includes isConnected too; do not confuse the full
    // inspection with the separate boolean liveness check.
    if (String(fn).includes("getAttribute")) {
      return { ...this.descriptor, connected: this.connected };
    }
    if (String(fn).includes("isConnected")) return this.connected;
    throw new Error("unexpected fixture evaluation");
  }
}

class FakeInteractiveLocator {
  constructor(handles) {
    this.handles = handles;
  }

  async count() {
    return this.handles.length;
  }

  nth(index) {
    return { elementHandle: async () => this.handles[index] ?? null };
  }
}

class FakePage extends EventEmitter {
  constructor({ url, title = "Fixture", aria = "- document Fixture", descriptors = [], frames = [] }) {
    super();
    this.currentUrl = url;
    this.currentTitle = title;
    this.aria = aria;
    this.handles = descriptors.map((row) => new FakeHandle(row));
    this.closed = false;
    this.main = { url: () => this.currentUrl };
    this.childFrames = frames;
    this.captureHook = null;
  }

  url() {
    return this.currentUrl;
  }

  async title() {
    if (this.captureHook) this.captureHook();
    return this.currentTitle;
  }

  mainFrame() {
    return this.main;
  }

  frames() {
    return [this.main, ...this.childFrames];
  }

  locator(selector) {
    if (selector === "body") return { ariaSnapshot: async () => this.aria };
    return new FakeInteractiveLocator(this.handles);
  }

  async opener() {
    return null;
  }

  viewportSize() {
    return this.viewport || null;
  }

  async screenshot() {
    if (!this.pngBytes) {
      const err = new Error("screenshot unavailable");
      throw err;
    }
    return this.pngBytes;
  }

  isClosed() {
    return this.closed;
  }

  attach(frame) {
    this.childFrames.push(frame);
    this.emit("frameattached", frame);
  }

  navigate(url) {
    this.currentUrl = url;
    this.emit("framenavigated", this.main);
  }
}

function stateFor(page) {
  const state = registerPageState({ pages: new Map(), pageIds: new WeakMap() }, page);
  publishPageState(state);
  return state;
}

const REQUIRED_ROLES = [
  "button",
  "link",
  "textbox",
  "checkbox",
  "radio",
  "combobox",
  "option",
  "menuitem",
  "tab",
  "generic",
];

test("capture returns stable opaque nodes and a redacted private transport envelope", async () => {
  const descriptors = REQUIRED_ROLES.map((role, index) => ({
    role,
    name: `node-${index}`,
    disabled: index === 1,
    focusable: role === "generic",
    signature: `private-selector-${index}`,
  }));
  const page = new FakePage({
    url: "https://user:password@example.test/path?token=URL_SECRET#fragment",
    descriptors,
    frames: [{ url: () => "https://cross-origin.test/frame?secret=FRAME_SECRET" }],
  });
  const state = stateFor(page);
  const result = await captureManagedObservation(state);

  assert.equal(result.ok, true);
  assert.equal(result.pageId, state.id);
  assert.equal(result.pageGeneration, 1);
  assert.match(result.snapshotId, /^snapshot-[0-9a-f-]{36}$/);
  assert.deepEqual(result.nodes.map((node) => node.role), REQUIRED_ROLES);
  assert.equal(new Set(result.nodes.map((node) => node.elementRef)).size, REQUIRED_ROLES.length);
  assert.equal(result.url, "https://example.test/path");
  assert.equal(result.hasQuery, true);
  assert.deepEqual(result.frameObservation, {
    mainFrameOnly: true,
    sameOriginOmitted: 0,
    crossOriginOmitted: 1,
  });
  assert.ok(result.aria.includes("Fixture"));

  const serialized = JSON.stringify(result);
  for (const secret of [
    "URL_SECRET",
    "FRAME_SECRET",
    "password",
    "private-selector",
    "cross-origin.test",
  ]) {
    assert.equal(serialized.includes(secret), false, secret);
  }

  const privateObservation = currentPageObservation(state);
  const target = resolveObservationTarget(privateObservation, {
    pageGeneration: result.pageGeneration,
    snapshotId: result.snapshotId,
    elementRef: result.nodes[0].elementRef,
  });
  assert.equal(target.handle, page.handles[0]);
  assert.equal(target.signature, "private-selector-0");
});

const ONE_BY_ONE_PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQD3A+hkAAAAAElFTkSuQmCC",
  "base64",
);

test("capture attaches a PNG with a valid signature, geometry, and opaque refs", async () => {
  const page = new FakePage({
    url: "https://example.test/form",
    title: "Form",
    aria: '- document Form\n  - button "+1"',
    descriptors: [{ role: "button", name: "+1", signature: "btn" }],
  });
  page.viewport = { width: 1, height: 1 };
  page.pngBytes = ONE_BY_ONE_PNG;
  const result = await captureManagedObservation(stateFor(page));
  assert.equal(result.textOnly, false);
  assert.equal(result.imageOmittedReason, undefined);
  const png = Buffer.from(result.pngBase64, "base64");
  assert.deepEqual([...png.subarray(0, 8)], [...PNG_SIGNATURE]);
  const inspected = inspectPng(png);
  assert.equal(inspected.ok, true);
  assert.equal(inspected.width, result.image.width);
  assert.equal(inspected.height, result.image.height);
  assert.equal(result.image.width, 1);
  assert.equal(result.image.height, 1);
  assert.equal(inspected.empty, false);
  assert.ok(result.aria.includes("+1"));
  assert.equal(result.nodes[0].elementRef.startsWith("dummy"), false);
  assert.match(result.nodes[0].elementRef, /^element-[0-9a-f-]{36}$|^[0-9a-f-]{36}$|^elementRef-|^el-/i);
  const model = JSON.stringify(modelObservationView(result));
  assert.equal(model.includes(result.pngBase64), false);
  assert.equal(model.includes("pageId"), false);
  assert.equal(model.includes("pngBase64"), false);
});

test("oversized PNG is omitted as text-only size_limit without truncating bytes", async () => {
  const page = new FakePage({
    url: "https://example.test/huge",
    descriptors: [{ role: "button", name: "Go", signature: "go" }],
  });
  page.viewport = { width: 1, height: 1 };
  page.pngBytes = Buffer.concat([
    ONE_BY_ONE_PNG,
    Buffer.alloc(OBSERVATION_LIMITS.pngB64Chars, 1),
  ]);
  const result = await captureManagedObservation(stateFor(page));
  assert.equal(result.textOnly, true);
  assert.equal(result.imageOmittedReason, "size_limit");
  assert.equal(result.pngBase64, undefined);
  assert.equal(result.truncated, true);
});


test("capture bounds every public text field, node count, and total JSON", async () => {
  const descriptors = Array.from({ length: OBSERVATION_LIMITS.nodes + 1 }, (_, index) => ({
    role: index === 0 ? "r".repeat(OBSERVATION_LIMITS.roleChars + 20) : "button",
    name: index === 0 ? "n".repeat(OBSERVATION_LIMITS.nameChars + 20) : `node-${index}`,
    disabled: false,
    focusable: index === 0,
    signature: `private-${index}`,
  }));
  const page = new FakePage({
    url: `https://example.test/${"p".repeat(OBSERVATION_LIMITS.urlChars + 20)}?secret=HIDDEN`,
    title: "t".repeat(OBSERVATION_LIMITS.titleChars + 20),
    aria: "a".repeat(OBSERVATION_LIMITS.ariaChars + 20),
    descriptors,
  });
  const result = await captureManagedObservation(stateFor(page));

  assert.equal(result.nodes.length, OBSERVATION_LIMITS.nodes);
  assert.equal(result.nodes[0].role.length, OBSERVATION_LIMITS.roleChars);
  assert.equal(result.nodes[0].name.length, OBSERVATION_LIMITS.nameChars);
  assert.equal(result.nodes[0].truncated, true);
  assert.equal(Object.hasOwn(result.nodes[0], "elementRef"), false);
  assert.equal(result.title.length, OBSERVATION_LIMITS.titleChars);
  assert.equal(result.aria.length, OBSERVATION_LIMITS.ariaChars);
  assert.ok(result.url.length <= OBSERVATION_LIMITS.urlChars);
  assert.equal(result.truncated, true);
  assert.ok(Buffer.byteLength(JSON.stringify(result)) <= OBSERVATION_LIMITS.jsonBytes);
  assert.equal(JSON.stringify(result).includes("HIDDEN"), false);
});

test("descriptor normalization is allowlisted and marks lossy fields non-actionable", () => {
  const handle = new FakeHandle({});
  const normalized = normalizeInteractiveDescriptor(
    {
      role: " BUTTON ",
      name: "label\0with-control",
      disabled: "false",
      signature: "private-signature",
      selector: "#must-not-pass",
      pageId: "page-private",
    },
    handle,
  );
  assert.deepEqual(Object.keys(normalized.publicNode).sort(), [
    "disabled",
    "name",
    "role",
    "truncated",
  ]);
  assert.equal(normalized.publicNode.role, "button");
  assert.equal(normalized.publicNode.disabled, false);
  assert.equal(normalized.publicNode.truncated, true);
  assert.equal(normalized.target.handle, handle);
  assert.equal(normalized.target.signature, "private-signature");
  assert.equal(JSON.stringify(normalized.publicNode).includes("selector"), false);
});

test("non-interactive and hidden candidates cannot consume the actionable node budget", async () => {
  const descriptors = [
    ...Array.from({ length: OBSERVATION_LIMITS.nodes }, (_, index) => ({
      role: "heading",
      name: `noise-${index}`,
      disabled: false,
      signature: `noise-${index}`,
    })),
    {
      role: "button",
      name: "late-button",
      disabled: false,
      signature: "late-button",
    },
    {
      role: "button",
      name: "hidden-button",
      hidden: true,
      disabled: false,
      signature: "hidden-button",
    },
    {
      role: "link",
      name: "inert-link",
      inert: true,
      disabled: false,
      signature: "inert-link",
    },
    {
      role: "textbox",
      name: "aria-hidden-input",
      ariaHidden: true,
      disabled: false,
      signature: "aria-hidden-input",
    },
    {
      role: "generic",
      name: "focusable-custom-control",
      focusable: true,
      disabled: false,
      signature: "focusable-custom-control",
    },
  ];
  const page = new FakePage({ url: "https://example.test/", descriptors });
  const result = await captureManagedObservation(stateFor(page));

  assert.deepEqual(
    result.nodes.map((node) => node.name),
    ["late-button", "focusable-custom-control"],
  );
  assert.equal(result.truncated, false);
});

test("node field truncation is distinct from observation-level omission", async () => {
  const page = new FakePage({
    url: "https://example.test/",
    descriptors: [
      {
        role: "r".repeat(OBSERVATION_LIMITS.roleChars + 1),
        name: "n".repeat(OBSERVATION_LIMITS.nameChars + 1),
        focusable: true,
        disabled: false,
        signature: "lossy-control",
      },
    ],
  });
  const result = await captureManagedObservation(stateFor(page));

  assert.equal(result.truncated, false);
  assert.equal(result.nodes.length, 1);
  assert.equal(result.nodes[0].truncated, true);
  assert.equal(Object.hasOwn(result.nodes[0], "elementRef"), false);
});

test("display URL strips credentials, fragments, and every query value", () => {
  assert.deepEqual(
    safeDisplayUrl("https://alice:secret@example.test/a?token=SECRET&empty=#fragment"),
    {
      url: "https://example.test/a",
      hasQuery: true,
      truncated: false,
    },
  );
  assert.deepEqual(safeDisplayUrl("not a url"), {
    url: "",
    hasQuery: false,
    truncated: true,
  });
  assert.deepEqual(safeDisplayUrl("about:blank"), {
    url: "about:blank",
    hasQuery: false,
    truncated: false,
  });
  for (const unsafe of [
    "data:text/plain,SECRET_PAYLOAD",
    "file:///C:/Users/Administrator/SECRET_PROFILE/auth.json",
    "javascript:SECRET_SCRIPT()",
    "about:blank#SECRET_FRAGMENT",
  ]) {
    const display = safeDisplayUrl(unsafe);
    assert.deepEqual(display, { url: "", hasQuery: false, truncated: true });
    assert.equal(JSON.stringify(display).includes("SECRET"), false, unsafe);
  }
});

test("capture rejects a page or frame change instead of publishing mixed state", async () => {
  for (const change of ["subframe", "main-frame"]) {
    const page = new FakePage({
      url: "https://example.test/start",
      descriptors: [{ role: "button", name: "go", disabled: false, signature: "go" }],
    });
    const state = stateFor(page);
    page.captureHook = () => {
      page.captureHook = null;
      if (change === "subframe") page.attach({ url: () => "https://example.test/frame" });
      else page.navigate("https://example.test/next");
    };

    await assert.rejects(
      captureManagedObservation(state),
      (error) =>
        error.workerCode === "observation_changed_during_capture" &&
        error.completion === "not_started" &&
        error.currentPageGeneration === state.generation,
      change,
    );
    assert.equal(currentPageObservation(state), null);
  }
});

test("managed preview preserves model references and releases its temporary handles", async () => {
  const descriptor = { role: "button", name: "go", signature: "go" };
  const page = new FakePage({ url: "https://example.test/", descriptors: [descriptor] });
  const state = stateFor(page);
  const model = await captureManagedObservation(state);
  const saved = currentPageObservation(state);
  const modelHandle = page.handles[0];
  page.handles = [new FakeHandle(descriptor), new FakeHandle({ role: "button", hidden: true })];
  const preview = await captureManagedObservation(state, { preview: true, screenshot: false });
  assert.notEqual(preview.snapshotId, model.snapshotId);
  assert.equal(currentPageObservation(state), saved, "preview replaced the model snapshot");
  assert.equal(resolveObservationTarget(saved, { pageGeneration: model.pageGeneration,
    snapshotId: model.snapshotId, elementRef: model.nodes[0].elementRef }).handle, modelHandle);
  assert(page.handles.every(handle => handle.disposed), "preview leaked temporary handles");
  assert(page.handles.every(handle => handle.disposeCalls === 1), "preview double-disposed handles");
  assert.equal(modelHandle.disposed, false);
});

test("text-only managed observe does not call screenshot", async () => {
  const page = new FakePage({ url: "https://example.test/" });
  let captures = 0;
  page.screenshot = async () => { captures += 1; return ONE_BY_ONE_PNG; };
  const result = await captureManagedObservation(stateFor(page), { screenshot: false });
  assert.equal(captures, 0, "text-only observe still took a screenshot");
  assert.equal(result.textOnly, true);
  assert.equal(result.pngBase64, undefined);
});

test("replacing a model observation releases its old handles, not the new model", async () => {
  const descriptor = { role: "button", name: "go", signature: "go" };
  const page = new FakePage({ url: "https://example.test/", descriptors: [descriptor] });
  const state = stateFor(page);
  await captureManagedObservation(state, { screenshot: false });
  const old = currentPageObservation(state);
  const oldHandle = page.handles[0];
  page.handles = [new FakeHandle(descriptor)];
  await captureManagedObservation(state, { screenshot: false });
  assert.equal(oldHandle.disposed, true, "retired model handle leaked");
  assert.equal(oldHandle.disposeCalls, 1);
  assert.equal(page.handles[0].disposed, false, "current model was disposed");
  assert.throws(() => resolveObservationTarget(old, {
    pageGeneration: old.pageGeneration, snapshotId: old.snapshotId,
    elementRef: old.nodes[0].elementRef,
  }), error => error.workerCode === "observation_required");
});

test("late screenshot failure disposes unpublished handles and allows a later capture", async () => {
  const page = new FakePage({ url: "https://example.test/", descriptors: [
    { role: "button", name: "go" }, { role: "button", name: "x".repeat(300) },
  ] });
  const state = stateFor(page);
  page.screenshot = async () => { page.navigate("https://example.test/next"); return ONE_BY_ONE_PNG; };
  await assert.rejects(captureManagedObservation(state),
    error => error.workerCode === "observation_changed_during_capture");
  assert(page.handles.every(handle => handle.disposeCalls === 1));
  assert.equal(currentPageObservation(state), null);
  page.handles = [];
  await captureManagedObservation(state, { screenshot: false });
});

test("capture disposal applies backpressure and navigation rejects the pending result", async () => {
  const page = new FakePage({ url: "https://example.test/", descriptors: [{ role: "button", name: "go" }] });
  const state = stateFor(page);
  await captureManagedObservation(state, { screenshot: false });
  let entered; let release;
  const disposing = new Promise(resolve => { entered = resolve; });
  const gate = new Promise(resolve => { release = resolve; });
  page.handles[0].dispose = async () => { entered(); await gate; };
  page.handles = [new FakeHandle({ role: "button", name: "new" })];
  const pending = captureManagedObservation(state, { screenshot: false });
  await disposing;
  await assert.rejects(captureManagedObservation(state), error => error.workerCode === "observation_busy");
  page.navigate("https://example.test/next");
  release();
  await assert.rejects(pending, error => error.workerCode === "observation_changed_during_capture");
  assert.equal(page.handles[0].disposeCalls, 1);
});

test("cancel during screenshot waits for the actual capture before releasing handles", async () => {
  const page = new FakePage({ url: "https://example.test/", descriptors: [{ role: "button", name: "go" }] });
  const state = stateFor(page), controller = new AbortController();
  let entered, release;
  const started = new Promise(resolve => { entered = resolve; });
  const gate = new Promise(resolve => { release = resolve; });
  page.screenshot = async () => { entered(); await gate; return ONE_BY_ONE_PNG; };
  let settled = false;
  const capture = captureManagedObservation(state, { signal: controller.signal });
  const result = assert.rejects(capture, error => error.workerCode === "run_cancelled" && error.completion === "unknown")
    .then(() => { settled = true; });
  await started;
  controller.abort();
  await Promise.resolve();
  assert.equal(settled, false, "abort is not physical completion");
  assert.equal(state.captureInFlight, true);
  assert.equal(page.handles[0].disposed, false);
  release(); await result;
  assert.equal(state.captureInFlight, false);
  assert.equal(currentPageObservation(state), null);
  assert.equal(page.handles[0].disposeCalls, 1);
});

test("cancel during old-handle disposal retires the unpublished replacement too", async () => {
  const page = new FakePage({ url: "https://example.test/", descriptors: [{ role: "button", name: "go" }] });
  const state = stateFor(page), controller = new AbortController();
  await captureManagedObservation(state, { screenshot: false });
  let entered, release;
  const started = new Promise(resolve => { entered = resolve; });
  const gate = new Promise(resolve => { release = resolve; });
  page.handles[0].dispose = async () => { entered(); await gate; };
  page.handles = [new FakeHandle({ role: "button", name: "new" })];
  const capture = captureManagedObservation(state, { screenshot: false, signal: controller.signal });
  const rejected = assert.rejects(capture, error => error.workerCode === "run_cancelled");
  await started;
  controller.abort(); release();
  await rejected;
  assert.equal(currentPageObservation(state), null);
  assert.equal(page.handles[0].disposeCalls, 1);
  await assert.rejects(captureManagedObservation(state, { signal: controller.signal }),
    error => error.completion === "not_started");
});

test("a capture cannot publish handles collected before a completed non-navigation mutation", async () => {
  for (const preview of [false, true]) {
    const page = new FakePage({ url: "https://example.test/", descriptors: [{ role: "button", name: "before" }] });
    const state = stateFor(page);
    let entered, release;
    const started = new Promise(resolve => { entered = resolve; });
    const gate = new Promise(resolve => { release = resolve; });
    page.screenshot = async () => { entered(); await gate; return ONE_BY_ONE_PNG; };
    const pending = captureManagedObservation(state, { preview });
    await started;
    await runPageMutation(state, async () => { page.currentTitle = "after mutation"; });
    release();
    await assert.rejects(pending, error => error.workerCode === "observation_changed_during_capture");
    assert.equal(currentPageObservation(state), null);
    assert.equal(page.handles[0].disposeCalls, 1);
    assert.equal(state.captureInFlight, false);
    page.handles = [];
    const fresh = await captureManagedObservation(state, { screenshot: false });
    assert.equal(fresh.title, "after mutation");
  }
});

test("capture started during pending native mutation is rejected without reading the page", async () => {
  for (const preview of [false, true]) {
    const page = new FakePage({ url: "https://example.test/" });
    const state = stateFor(page);
    let release;
    const gate = new Promise(resolve => { release = resolve; });
    const mutation = runPageMutation(state, () => gate);
    let titleReads = 0;
    page.title = async () => { titleReads += 1; return "must not capture"; };
    try {
      await assert.rejects(captureManagedObservation(state, { preview, screenshot: false }),
        error => error.workerCode === "page_mutation_in_flight" && error.completion === "not_started");
      assert.equal(titleReads, 0);
    } finally { release(); await mutation; }
  }
});

test("mutation during replacement disposal rejects and retires the undelivered replacement", async () => {
  const page = new FakePage({ url: "https://example.test/", descriptors: [{ role: "button", name: "old" }] });
  const state = stateFor(page);
  await captureManagedObservation(state, { screenshot: false });
  let entered, release;
  const started = new Promise(resolve => { entered = resolve; });
  const gate = new Promise(resolve => { release = resolve; });
  page.handles[0].dispose = async () => { entered(); await gate; };
  page.handles = [new FakeHandle({ role: "button", name: "new" })];
  const pending = captureManagedObservation(state, { screenshot: false });
  await started;
  await runPageMutation(state, async () => {});
  release();
  await assert.rejects(pending, error => error.workerCode === "observation_changed_during_capture");
  assert.equal(currentPageObservation(state), null);
  assert.equal(page.handles[0].disposeCalls, 1);
});

test("explicit observation retirement fences captures even without document or input changes", async () => {
  const page = new FakePage({ url: "https://example.test/", descriptors: [{ role: "button", name: "old" }] });
  const state = stateFor(page);
  page.screenshot = async () => { invalidatePageObservation(state); return ONE_BY_ONE_PNG; };
  await assert.rejects(captureManagedObservation(state), error => error.workerCode === "observation_changed_during_capture");
  assert.equal(currentPageObservation(state), null);
  assert.equal(page.handles[0].disposeCalls, 1);
});

test("retirement during unused-handle cleanup cannot return a retired model envelope", async () => {
  const page = new FakePage({ url: "https://example.test/", descriptors: [
    { role: "button", name: "kept" }, { role: "button", name: "x".repeat(300) },
  ] });
  const state = stateFor(page);
  let entered, release;
  const started = new Promise(resolve => { entered = resolve; });
  const gate = new Promise(resolve => { release = resolve; });
  page.handles[1].dispose = async () => { entered(); await gate; };
  const capture = captureManagedObservation(state, { screenshot: false });
  await started;
  const published = currentPageObservation(state);
  assert.ok(published);
  invalidatePageObservation(state);
  release();
  await assert.rejects(capture, error => error.workerCode === "observation_changed_during_capture");
  assert.equal(currentPageObservation(state), null);
  assert.equal(page.handles[0].disposeCalls, 1);
  assert.equal(state.captureInFlight, false);
});
