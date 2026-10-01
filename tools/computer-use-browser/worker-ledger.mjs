import { createHmac, randomBytes } from "node:crypto";

import {
  WORKER_COMPLETION_UNKNOWN,
  workerException,
} from "./worker-errors.mjs";

const PROCESS_KEY = randomBytes(32);

export function canonicalSemantics(body) {
  const params = {};
  for (const key of [
    "text",
    "value",
    "key",
    "delta",
    "dy",
    "button",
    "count",
    "url",
    "filename",
    "nameEquals",
    "timeoutMs",
    "destElementRef",
    "dropElementRef",
    "toX",
    "toY",
  ]) {
    if (body[key] !== undefined) params[key] = body[key];
  }
  return JSON.stringify({
    kind: String(body.kind || "click"),
    pageId: String(body.pageId || ""),
    pageGeneration: Number(body.pageGeneration) || 0,
    snapshotId: String(body.snapshotId || ""),
    elementRef: body.elementRef ? String(body.elementRef) : null,
    params,
  });
}

export function fingerprint(canonical) {
  return createHmac("sha256", PROCESS_KEY).update(canonical).digest("hex");
}

export function beginAction(slot, body, id) {
  const fp = fingerprint(canonicalSemantics(body));
  const prior = slot.actions.get(id);
  if (prior) {
    if (prior.fingerprint !== fp) {
      throw workerException(
        409,
        "action_id_conflict",
        "actionId reused with different parameters",
      );
    }
    if (prior.state === "done") return { replay: { ...prior.result, replayed: true } };
    if (prior.state === "rejected") {
      throw workerException(
        409,
        prior.code || "action_rejected",
        prior.error || "action was rejected",
      );
    }
    if (prior.state === "pending") {
      throw workerException(409, "action_in_flight", "action is already in flight");
    }
    throw workerException(
      409,
      prior.code || "action_outcome_unknown",
      "previous action outcome is unknown; observe before continuing",
      WORKER_COMPLETION_UNKNOWN,
    );
  }
  if (slot.inFlight) {
    throw workerException(
      409,
      "action_in_flight",
      "another action is already in flight",
    );
  }
  const controller = new AbortController();
  const entry = { fingerprint: fp, state: "pending", controller };
  slot.actions.set(id, entry);
  slot.inFlight = id;
  slot.actionEpoch = {};
  return { entry };
}

export function finishAction(slot, id, next) {
  const entry = slot.actions.get(id);
  if (entry) {
    if (entry.state !== "unknown") Object.assign(entry, next);
  }
  if (slot.inFlight === id) slot.inFlight = null;
}

export function captureRecoveryObservation(slot) {
  // An observation begun during an action cannot establish that action's
  // outcome, even if the action has settled by the time capture returns.
  if (slot.inFlight) return null;
  slot.actionEpoch ??= {};
  return Object.freeze({ slot, epoch: slot.actionEpoch });
}

export function unlockUnknownIfIdle(slot, captured) {
  if (!captured || captured.slot !== slot || slot.inFlight || captured.epoch !== slot.actionEpoch) return false;
  slot.writeBlockedUntilObserve = false;
  return true;
}
