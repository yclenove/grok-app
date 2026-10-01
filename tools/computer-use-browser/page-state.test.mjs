import test from "node:test";
import assert from "node:assert/strict";
import { EventEmitter } from "node:events";

import {
  capturePageStateIdentity,
  capturePageObservationIdentity,
  currentPageObservation,
  pageStateIdentityMatches,
  pageObservationIdentityMatches,
  publishPageState,
  registerPageState,
  replacePageObservation,
  runPageMutation,
} from "./page-state.mjs";
import {
  createObservationState,
  resolveObservationTarget,
} from "./observation-state.mjs";

class FakePage extends EventEmitter {
  constructor(url = "about:blank") {
    super();
    this.currentUrl = url;
    this.closed = false;
    this.main = { kind: "main" };
  }

  url() {
    return this.currentUrl;
  }

  mainFrame() {
    return this.main;
  }

  isClosed() {
    return this.closed;
  }

  navigate(url, frame = this.main) {
    this.currentUrl = url;
    this.emit("framenavigated", frame);
  }

  attach(frame) {
    this.emit("frameattached", frame);
  }

  detach(frame) {
    this.emit("framedetached", frame);
  }

  closePage() {
    this.closed = true;
    this.emit("close");
  }
}

function slot() {
  return { pages: new Map(), pageIds: new WeakMap() };
}

function observation(generation, name) {
  return createObservationState(generation, [
    { target: { name }, role: "button", name },
  ]);
}

function resolveFirst(obs) {
  return resolveObservationTarget(obs, {
    pageGeneration: obs.pageGeneration,
    snapshotId: obs.snapshotId,
    elementRef: obs.nodes[0].elementRef,
  });
}

test("initial navigation publishes generation one and every later main navigation advances", () => {
  const registry = slot();
  const page = new FakePage();
  const state = registerPageState(registry, page);

  page.navigate("http://fixture.test/");
  assert.equal(state.generation, 1);
  publishPageState(state);
  assert.equal(state.generation, 1);
  assert.equal(state.lastUrl, "http://fixture.test/");

  const first = observation(1, "first");
  replacePageObservation(state, first);
  page.navigate("http://fixture.test/");
  assert.equal(state.generation, 2, "same URL reload must advance generation");
  assert.equal(currentPageObservation(state), null);
  assert.throws(() => resolveFirst(first), (error) => error.workerCode === "observation_required");

  const second = observation(2, "second");
  replacePageObservation(state, second);
  page.navigate("http://fixture.test/next");
  assert.equal(state.generation, 3);
  assert.equal(currentPageObservation(state), null);
});

test("subframe lifecycle invalidates observations without advancing main generation", () => {
  const registry = slot();
  const page = new FakePage("http://fixture.test/");
  const state = registerPageState(registry, page);
  publishPageState(state);
  const subframe = { kind: "subframe" };

  for (const event of ["attach", "navigate", "detach"]) {
    replacePageObservation(state, observation(1, event));
    if (event === "navigate") page.navigate(page.url(), subframe);
    else page[event](subframe);
    assert.equal(state.generation, 1, event);
    assert.equal(currentPageObservation(state), null, event);
  }
});

test("a fresh observation atomically retires the previous target map", () => {
  const state = registerPageState(slot(), new FakePage("http://fixture.test/"));
  publishPageState(state);
  const first = observation(1, "first");
  const second = observation(1, "second");

  replacePageObservation(state, first);
  replacePageObservation(state, second);
  assert.throws(() => resolveFirst(first), (error) => error.workerCode === "observation_required");
  assert.equal(resolveFirst(second).name, "second");
});

test("popup identity is independent from its parent", () => {
  const registry = slot();
  const parent = new FakePage("http://fixture.test/");
  const popup = new FakePage();
  const parentState = registerPageState(registry, parent);
  const popupState = registerPageState(registry, popup);
  popup.navigate("http://fixture.test/popup");
  publishPageState(parentState);
  publishPageState(popupState);

  assert.notEqual(parentState.id, popupState.id);
  assert.equal(parentState.generation, 1);
  assert.equal(popupState.generation, 1);
  parent.navigate("http://fixture.test/parent-next");
  assert.equal(parentState.generation, 2);
  assert.equal(popupState.generation, 1);
  popup.navigate("http://fixture.test/popup");
  assert.equal(popupState.generation, 2);
});

test("closed pages become tombstones and never fall back to another tab", () => {
  const registry = slot();
  const firstPage = new FakePage("http://fixture.test/first");
  const secondPage = new FakePage("http://fixture.test/second");
  const first = registerPageState(registry, firstPage);
  const second = registerPageState(registry, secondPage);
  publishPageState(first);
  publishPageState(second);
  const old = observation(1, "old");
  replacePageObservation(first, old);

  firstPage.closePage();
  assert.equal(registry.pages.has(first.id), false);
  assert.equal(registry.pages.has(second.id), true);
  assert.equal(registerPageState(registry, firstPage), null);
  assert.throws(() => resolveFirst(old), (error) => error.workerCode === "observation_required");
});

test("a rebuilt worker registry creates a new page identity", () => {
  const oldState = registerPageState(slot(), new FakePage("http://fixture.test/"));
  const newState = registerPageState(slot(), new FakePage("http://fixture.test/"));
  publishPageState(oldState);
  publishPageState(newState);
  assert.notEqual(oldState.id, newState.id);
  assert.equal(oldState.generation, 1);
  assert.equal(newState.generation, 1);
});

test("a page mutation retires observation before success or unknown completion", async () => {
  const state = registerPageState(slot(), new FakePage("http://fixture.test/"));
  publishPageState(state);
  const successful = observation(1, "success");
  replacePageObservation(state, successful);

  const result = await runPageMutation(state, async () => {
    assert.equal(currentPageObservation(state), null);
    return "done";
  });
  assert.equal(result, "done");
  assert.throws(
    () => resolveFirst(successful),
    (error) => error.workerCode === "observation_required",
  );

  const uncertain = observation(1, "unknown");
  replacePageObservation(state, uncertain);
  await assert.rejects(
    runPageMutation(state, async () => {
      throw new Error("worker outcome unknown");
    }),
    /worker outcome unknown/,
  );
  assert.equal(currentPageObservation(state), null);
  assert.throws(
    () => resolveFirst(uncertain),
    (error) => error.workerCode === "observation_required",
  );
});

test("capture identity detects main and subframe changes without exposing the page", () => {
  const page = new FakePage("http://fixture.test/");
  const state = registerPageState(slot(), page);
  publishPageState(state);
  const first = capturePageStateIdentity(state);
  assert.deepEqual(Object.keys(first).sort(), ["frameRevision", "pageGeneration", "pageId"]);
  assert.equal(pageStateIdentityMatches(state, first), true);

  page.attach({ kind: "subframe" });
  assert.equal(pageStateIdentityMatches(state, first), false);
  const second = capturePageStateIdentity(state);
  assert.equal(second.pageGeneration, 1);
  page.navigate("http://fixture.test/");
  assert.equal(pageStateIdentityMatches(state, second), false);
  assert.equal(state.generation, 2);
});

test("mutation fences capture without rejecting its own prepared document identity", async () => {
  const state = registerPageState(slot(), new FakePage("http://fixture.test/"));
  const document = capturePageStateIdentity(state), capture = capturePageObservationIdentity(state);
  let dispatched = 0;
  await runPageMutation(state, async () => {
    assert.equal(pageStateIdentityMatches(state, document), true, "drag guard must survive its own admission");
    assert.equal(pageObservationIdentityMatches(state, capture), false);
    assert.throws(() => capturePageObservationIdentity(state), error => error.workerCode === "page_mutation_in_flight");
    await assert.rejects(runPageMutation(state, async () => { dispatched += 1; }),
      error => error.workerCode === "page_mutation_in_flight");
    assert.equal(state.mutationInFlight, true, "rejected nested call must not clear original ownership");
  });
  assert.equal(dispatched, 0);
  assert.equal(pageStateIdentityMatches(state, document), true);
  assert.equal(pageObservationIdentityMatches(state, capture), false);
  assert.equal(state.mutationInFlight, false);
  assert.equal(pageObservationIdentityMatches(state, capturePageObservationIdentity(state)), true);
});

test("unknown native completion retires capture, releases only its promise and leaves other pages intact", async () => {
  const registry = slot(), state = registerPageState(registry, new FakePage("http://fixture.test/first"));
  const other = registerPageState(registry, new FakePage("http://fixture.test/other"));
  const before = capturePageObservationIdentity(state), unaffected = capturePageObservationIdentity(other);
  await assert.rejects(runPageMutation(state, async () => { throw new Error("native outcome unknown"); }), /native outcome unknown/);
  assert.equal(state.mutationInFlight, false);
  assert.equal(pageObservationIdentityMatches(state, before), false);
  assert.equal(pageObservationIdentityMatches(other, unaffected), true);
  // Higher-level unknown quarantine remains a separate admission requirement.
  assert.equal(pageObservationIdentityMatches(state, capturePageObservationIdentity(state)), true);
});
