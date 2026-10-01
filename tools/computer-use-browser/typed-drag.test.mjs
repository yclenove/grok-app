import test from "node:test";
import assert from "node:assert/strict";
import { createObservationState, invalidateObservationState } from "./observation-state.mjs";
import { preflightTypedAct, prepareTypedAct } from "./typed-act.mjs";

function fixture({ visual = true, hook = async () => {} } = {}) {
  const events = [];
  const element = {
    tagName: "DIV", isConnected: true, disabled: false, textContent: "Drag", tabIndex: 0,
    getAttribute: (name) => name === "role" ? "button" : "",
    closest: () => null, checkVisibility: () => true, getClientRects: () => ({ length: 1 }),
  };
  const handle = {
    evaluate: async (fn, arg) => fn(element, arg),
    boundingBox: async () => ({ x: 10, y: 20, width: 20, height: 20 }),
  };
  const observation = createObservationState(1, [{
    role: "button", name: "Drag", target: { handle },
  }], visual ? { width: 400, height: 300 } : undefined);
  let closed = false;
  const page = {
    viewportSize: () => ({ width: 400, height: 300 }),
    isClosed: () => closed,
    close: async () => { events.push(["close"]); await hook("close"); closed = true; },
    mouse: Object.fromEntries(["move", "down", "up"].map(name => [name, async (...args) => {
      events.push([name, ...args]); await hook(name, args);
    }])),
  };
  const body = { pageGeneration: 1, snapshotId: observation.snapshotId,
    elementRef: observation.nodes[0].elementRef, toX: 180, toY: 190 };
  return { page, observation, body, events, element };
}

test("drag destinations reject coercion, incomplete pairs, hidden aliases and mixed authority", () => {
  const base = fixture().body;
  for (const extra of [
    { toX: null, toY: 5 }, { toX: "12", toY: 5 }, { toX: -1, toY: 5 },
    { toX: 4 }, { toY: 4 }, {}, { toX: NaN, toY: 4 },
    { destElementRef: "dest", toX: 4, toY: 5 },
    { destElementRef: "dest", dropElementRef: "dest" },
    { destElementRef: 12 }, { destElementRef: "" },
    { toX: 4, toY: 5, x1: 8, y1: 9 }, { toX: 4, toY: 5, selector: {} },
  ]) {
    const { toX, toY, ...identity } = base;
    assert.throws(() => preflightTypedAct("drag", { ...identity, ...extra }),
      error => error.completion === "not_started", JSON.stringify(extra));
  }
});

test("pixel destination requires a delivered screenshot and exclusive in-image coordinates", async () => {
  for (const [visual, extra] of [[false, {}], [true, { toX: 400 }], [true, { toY: 300 }]]) {
    const f = fixture({ visual });
    await assert.rejects(prepareTypedAct({ ...f, kind: "drag", body: { ...f.body, ...extra } }),
      error => error.completion === "not_started");
    assert.deepEqual(f.events, []);
  }
});

test("pixel drag uses the explicit endpoint and one down/up pair", async () => {
  const f = fixture();
  const action = await prepareTypedAct({ ...f, kind: "drag" });
  await action.dispatch();
  assert.equal(f.events.filter(e => e[0] === "down").length, 1);
  assert.equal(f.events.filter(e => e[0] === "up").length, 1);
  assert.deepEqual(f.events.filter(e => e[0] === "move").at(-1).slice(0, 3), ["move", 180, 190]);
  assert.equal(f.events.some(e => e[0] === "close"), false);
});

test("retired snapshot after preparation dispatches no input", async () => {
  const f = fixture();
  const action = await prepareTypedAct({ ...f, kind: "drag" });
  invalidateObservationState(f.observation);
  await assert.rejects(action.dispatch());
  assert.deepEqual(f.events, []);
});

test("cancel after down closes only the owned page without dropping on the destination", async () => {
  const controller = new AbortController();
  const f = fixture({ hook: async name => { if (name === "down") controller.abort(); } });
  const action = await prepareTypedAct({ ...f, kind: "drag", signal: controller.signal });
  await assert.rejects(action.dispatch(), error => error.completion === "unknown");
  assert.deepEqual(f.events.map(e => e[0]), ["move", "down", "close"]);
});

test("ambiguous down and failed cleanup are unknown, never safe to replay", async () => {
  const f = fixture({ hook: async name => {
    if (name === "down" || name === "close") throw new Error("injected transport failure");
  } });
  const action = await prepareTypedAct({ ...f, kind: "drag" });
  await assert.rejects(action.dispatch(), error =>
    error.workerCode === "input_cleanup_pending" && error.completion === "unknown");
  assert.deepEqual(f.events.map(e => e[0]), ["move", "down", "close"]);
});

test("document replacement and handle retirement during drag never release on a replacement", async () => {
  for (const failure of ["document", "handle", "resize", "move", "up"]) {
    let current = true;
    let f;
    f = fixture({ hook: async name => {
      if (name === "down") {
        if (failure === "document") current = false;
        if (failure === "handle") f.element.isConnected = false;
        if (failure === "resize") f.page.viewportSize = () => ({ width: 600, height: 300 });
      }
      if (name === "move" && failure === "move" && f.events.some(e => e[0] === "down")) throw new Error("move failed");
      if (name === "up" && failure === "up") throw new Error("up response lost");
    } });
    const action = await prepareTypedAct({ ...f, kind: "drag", checkIdentity: () => {
      if (!current) throw new Error("document replaced");
    } });
    await assert.rejects(action.dispatch(), error => error.completion === "unknown", failure);
    assert.equal(f.events.filter(e => e[0] === "close").length, 1, failure);
    assert.equal(f.events.filter(e => e[0] === "up").length, failure === "up" ? 1 : 0, failure);
  }
});

test("geometry and viewport changes before drag are rejected without input", async () => {
  const f = fixture();
  const action = await prepareTypedAct({ ...f, kind: "drag" });
  f.page.viewportSize = () => ({ width: 600, height: 300 });
  await assert.rejects(action.dispatch(), error => error.workerCode === "drag_viewport_changed" &&
    error.completion === "not_started");
  assert.deepEqual(f.events, []);
});
