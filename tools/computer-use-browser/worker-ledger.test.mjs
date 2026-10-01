import assert from "node:assert/strict";
import test from "node:test";
import {
  beginAction,
  canonicalSemantics,
  captureRecoveryObservation,
  finishAction,
  fingerprint,
  unlockUnknownIfIdle,
} from "./worker-ledger.mjs";

function slot() {
  return { actions: new Map(), inFlight: null, writeBlockedUntilObserve: false };
}

function clickBody(extra = {}) {
  return {
    kind: "click",
    pageId: "page-1",
    pageGeneration: 1,
    snapshotId: "snap-1",
    elementRef: "ref-1",
    ...extra,
  };
}

test("HMAC fingerprint is keyed over canonical semantics only", () => {
  const a = fingerprint(canonicalSemantics(clickBody()));
  const b = fingerprint(canonicalSemantics(clickBody()));
  const other = fingerprint(canonicalSemantics(clickBody({ elementRef: "ref-2" })));
  assert.match(a, /^[0-9a-f]{64}$/);
  assert.equal(a, b);
  assert.notEqual(a, other);
  const ignored = fingerprint(
    canonicalSemantics(clickBody({ owner: "run-x", role: "button", selector: "#x" })),
  );
  assert.equal(a, ignored);
});

test("same id same fingerprint replays the terminal envelope", () => {
  const s = slot();
  const body = clickBody();
  const started = beginAction(s, body, "act-1");
  assert.equal(started.replay, undefined);
  assert.equal(s.inFlight, "act-1");
  finishAction(s, "act-1", {
    state: "done",
    result: { ok: true, pageId: "page-1", pageGeneration: 2, url: "http://127.0.0.1/", popup: false },
  });
  assert.equal(s.inFlight, null);
  const replay = beginAction(s, body, "act-1");
  assert.equal(replay.replay.ok, true);
  assert.equal(replay.replay.replayed, true);
  assert.equal(replay.replay.pageId, "page-1");
  assert.equal(replay.replay.pageGeneration, 2);
  assert.equal(s.inFlight, null);
});

test("same id different fingerprint is conflict with zero execution", () => {
  const s = slot();
  beginAction(s, clickBody(), "act-1");
  finishAction(s, "act-1", { state: "done", result: { ok: true } });
  assert.throws(
    () => beginAction(s, clickBody({ elementRef: "other" }), "act-1"),
    (error) => error.workerCode === "action_id_conflict",
  );
  assert.equal(s.inFlight, null);
});

test("run-wide in-flight mutex uses begin state as the barrier", async () => {
  const s = slot();
  beginAction(s, clickBody(), "act-1");
  const second = await Promise.resolve().then(() => {
    try {
      beginAction(s, clickBody({ elementRef: "ref-2" }), "act-2");
      return "executed";
    } catch (error) {
      return error.workerCode;
    }
  });
  assert.equal(second, "action_in_flight");
  assert.equal(s.inFlight, "act-1");
  finishAction(s, "act-1", { state: "done", result: { ok: true } });
  const third = beginAction(s, clickBody({ elementRef: "ref-2" }), "act-2");
  assert.equal(third.replay, undefined);
  assert.equal(s.inFlight, "act-2");
});

test("unknown is frozen and observe does not unlock while in-flight", () => {
  const s = slot();
  beginAction(s, clickBody(), "act-1");
  finishAction(s, "act-1", { state: "unknown", error: "lost", code: "worker_internal" });
  finishAction(s, "act-1", {
    state: "done",
    result: { ok: true, pageId: "should-not-land" },
  });
  assert.throws(
    () => beginAction(s, clickBody(), "act-1"),
    (error) => error.completion === "unknown" || error.workerCode === "action_outcome_unknown",
  );

  s.writeBlockedUntilObserve = true;
  const before = captureRecoveryObservation(s);
  s.inFlight = "act-2";
  unlockUnknownIfIdle(s, before);
  assert.equal(s.writeBlockedUntilObserve, true);
  const during = captureRecoveryObservation(s);
  s.inFlight = null;
  unlockUnknownIfIdle(s, during);
  assert.equal(s.writeBlockedUntilObserve, true, "a pre-completion observation cannot recover unknown");
  unlockUnknownIfIdle(s, captureRecoveryObservation(s));
  assert.equal(s.writeBlockedUntilObserve, false);
});

test("an observation overlapping a new completed action cannot unlock its unknown outcome", () => {
  const s = slot();
  const before = captureRecoveryObservation(s);
  beginAction(s, clickBody(), "act-1");
  const during = captureRecoveryObservation(s);
  finishAction(s, "act-1", { state: "unknown", code: "worker_internal" });
  s.writeBlockedUntilObserve = true;
  for (const proof of [undefined, null, before, during, captureRecoveryObservation(slot())]) {
    assert.equal(unlockUnknownIfIdle(s, proof), false);
    assert.equal(s.writeBlockedUntilObserve, true);
  }
  assert.equal(unlockUnknownIfIdle(s, captureRecoveryObservation(s)), true);
  assert.throws(() => beginAction(s, clickBody(), "act-1"), error => error.completion === "unknown");
});

test("replay and rejected admission do not invalidate an otherwise current recovery capture", () => {
  const s = slot();
  beginAction(s, clickBody(), "act-1");
  finishAction(s, "act-1", { state: "done", result: { ok: true } });
  const captured = captureRecoveryObservation(s);
  assert.equal(beginAction(s, clickBody(), "act-1").replay.replayed, true);
  assert.throws(() => beginAction(s, clickBody({ text: "different" }), "act-1"));
  assert.equal(unlockUnknownIfIdle(s, captured), true);
});
