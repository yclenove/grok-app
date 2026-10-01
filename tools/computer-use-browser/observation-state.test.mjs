import test from "node:test";
import assert from "node:assert/strict";

import {
  createObservationState,
  invalidateObservationState,
  resolveObservationTarget,
} from "./observation-state.mjs";

function target(name) {
  return { target: { name }, role: "button", name };
}

test("opaque refs are scoped to one page generation and snapshot", () => {
  const first = createObservationState(3, [target("first")]);
  const second = createObservationState(3, [target("second")]);
  const ref = first.nodes[0].elementRef;

  assert.equal(
    resolveObservationTarget(first, {
      pageGeneration: 3,
      snapshotId: first.snapshotId,
      elementRef: ref,
    }).name,
    "first",
  );
  assert.throws(
    () =>
      resolveObservationTarget(second, {
        pageGeneration: 3,
        snapshotId: first.snapshotId,
        elementRef: ref,
      }),
    (error) => error.workerCode === "stale_snapshot" && error.completion === "not_started",
  );
});

test("cross-page refs and stale generations have zero resolution", () => {
  const pageA = createObservationState(1, [target("a")]);
  const pageB = createObservationState(1, [target("b")]);
  assert.throws(
    () =>
      resolveObservationTarget(pageB, {
        pageGeneration: 1,
        snapshotId: pageB.snapshotId,
        elementRef: pageA.nodes[0].elementRef,
      }),
    (error) => error.workerCode === "stale_element_ref",
  );
  assert.throws(
    () =>
      resolveObservationTarget(pageA, {
        pageGeneration: 2,
        snapshotId: pageA.snapshotId,
        elementRef: pageA.nodes[0].elementRef,
      }),
    (error) =>
      error.workerCode === "stale_observation_generation" &&
      error.currentPageGeneration === 1,
  );
});

test("new observation and explicit invalidation retire every old ref", () => {
  let current = createObservationState(5, [target("old")]);
  const old = current;
  current = createObservationState(5, [target("new")]);
  assert.notEqual(current.snapshotId, old.snapshotId);
  assert.throws(
    () =>
      resolveObservationTarget(current, {
        pageGeneration: 5,
        snapshotId: old.snapshotId,
        elementRef: old.nodes[0].elementRef,
      }),
    (error) => error.workerCode === "stale_snapshot",
  );
  assert.equal(invalidateObservationState(current), null);
  assert.throws(
    () =>
      resolveObservationTarget(null, {
        pageGeneration: 5,
        snapshotId: current.snapshotId,
        elementRef: current.nodes[0].elementRef,
      }),
    (error) => error.workerCode === "observation_required",
  );
});

test("public observation is immutable and never serializes private routing details", () => {
  const privateTarget = {
    selector: "#password-field",
    profile: String.raw`C:\Users\Example\profile`,
    pageId: "page-private",
    token: "PRIVATE-TARGET-TOKEN",
  };
  const observation = createObservationState(7, [
    {
      target: privateTarget,
      role: "button",
      name: "Safe label",
      disabled: false,
      selector: "#must-not-serialize",
      profile: "profile-must-not-serialize",
      pageId: "page-must-not-serialize",
      locator: { token: "LOCATOR-SECRET" },
    },
  ]);

  assert.deepEqual(Object.keys(observation).sort(), [
    "nodes",
    "pageGeneration",
    "snapshotId",
  ]);
  assert.deepEqual(Object.keys(observation.nodes[0]).sort(), [
    "disabled",
    "elementRef",
    "name",
    "role",
    "truncated",
  ]);
  assert.equal(Object.isFrozen(observation), true);
  assert.equal(Object.isFrozen(observation.nodes), true);
  assert.equal(Object.isFrozen(observation.nodes[0]), true);

  const serialized = JSON.stringify(observation);
  for (const secret of [
    "password-field",
    "Example",
    "page-private",
    "PRIVATE-TARGET-TOKEN",
    "must-not-serialize",
    "LOCATOR-SECRET",
  ]) {
    assert.equal(serialized.includes(secret), false, secret);
  }
  assert.equal(
    resolveObservationTarget(observation, {
      pageGeneration: 7,
      snapshotId: observation.snapshotId,
      elementRef: observation.nodes[0].elementRef,
    }),
    privateTarget,
  );
});

test("creation rejects malformed generations, target rows, and overlong fields", () => {
  for (const pageGeneration of [0, -1, 1.5, Number.MAX_SAFE_INTEGER + 1, NaN, Infinity, "1"]) {
    assert.throws(
      () => createObservationState(pageGeneration, []),
      (error) =>
        error.statusCode === 400 &&
        error.workerCode === "invalid_observation_state" &&
        error.completion === "not_started",
    );
  }

  const invalidTargets = [
    null,
    {},
    { target: null, role: "button", name: "missing target" },
    { target: {}, role: "", name: "missing role" },
    { target: {}, role: "r".repeat(65), name: "role too long" },
    { target: {}, role: "button", name: "n".repeat(257) },
    { target: {}, role: "button", name: "bad disabled", disabled: "false" },
    { target: {}, role: "button", name: "bad truncated", truncated: "false" },
  ];
  for (const row of invalidTargets) {
    assert.throws(
      () => createObservationState(1, row === null ? null : [row]),
      (error) => error.workerCode === "invalid_observation_state",
    );
  }
  assert.throws(
    () =>
      createObservationState(
        1,
        Array.from({ length: 65 }, (_, i) => target(`node-${i}`)),
      ),
    (error) => error.workerCode === "invalid_observation_state",
  );
});

test("opaque ids are unique and bounded and truncated nodes receive no ref", () => {
  const observation = createObservationState(
    9,
    Array.from({ length: 64 }, (_, i) => target(`node-${i}`)),
  );
  const refs = observation.nodes.map((node) => node.elementRef);
  assert.match(observation.snapshotId, /^snapshot-[0-9a-f-]{36}$/);
  assert.equal(new Set(refs).size, 64);
  for (const ref of refs) {
    assert.match(ref, /^element-[0-9a-f-]{36}$/);
    assert.ok(ref.length <= 128);
  }

  const truncated = createObservationState(9, [
    { target: { name: "hidden" }, role: "button", name: "partial", truncated: true },
  ]);
  assert.equal(Object.hasOwn(truncated.nodes[0], "elementRef"), false);
});

test("resolution check order is observation then generation then snapshot then ref", () => {
  const observation = createObservationState(11, [target("ordered")]);
  const valid = {
    pageGeneration: 11,
    snapshotId: observation.snapshotId,
    elementRef: observation.nodes[0].elementRef,
  };

  assert.throws(
    () => resolveObservationTarget(null, {}),
    (error) => error.workerCode === "observation_required",
  );
  assert.throws(
    () =>
      resolveObservationTarget(observation, {
        pageGeneration: 12,
        snapshotId: "wrong",
        elementRef: "wrong",
      }),
    (error) =>
      error.workerCode === "stale_observation_generation" &&
      error.currentPageGeneration === 11,
  );
  assert.throws(
    () => resolveObservationTarget(observation, { ...valid, snapshotId: "x".repeat(129) }),
    (error) => error.workerCode === "stale_snapshot",
  );
  assert.throws(
    () => resolveObservationTarget(observation, { ...valid, elementRef: "x".repeat(129) }),
    (error) => error.workerCode === "stale_element_ref",
  );

  const cloned = JSON.parse(JSON.stringify(observation));
  assert.throws(
    () => resolveObservationTarget(cloned, valid),
    (error) => error.workerCode === "observation_required",
  );
  invalidateObservationState(observation);
  assert.throws(
    () => resolveObservationTarget(observation, valid),
    (error) => error.workerCode === "observation_required",
  );
});
