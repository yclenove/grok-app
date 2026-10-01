import { randomUUID } from "node:crypto";

import {
  WORKER_COMPLETION_NOT_STARTED,
  WORKER_COMPLETION_UNKNOWN,
  workerException,
} from "./worker-errors.mjs";

const MAX_NODES = 64;
const MAX_ROLE_LENGTH = 64;
const MAX_NAME_LENGTH = 256;
const OPAQUE_ID_ATTEMPTS = 8;
const PRIVATE_OBSERVATIONS = new WeakMap();

function invalidState(message) {
  throw workerException(
    400,
    "invalid_observation_state",
    message,
    WORKER_COMPLETION_NOT_STARTED,
  );
}

function stale(code, message, currentPageGeneration) {
  throw workerException(
    409,
    code,
    message,
    WORKER_COMPLETION_NOT_STARTED,
    currentPageGeneration,
  );
}

function opaqueId(prefix, used) {
  for (let attempt = 0; attempt < OPAQUE_ID_ATTEMPTS; attempt += 1) {
    const value = `${prefix}-${randomUUID()}`;
    if (!used || !used.has(value)) return value;
  }
  throw workerException(
    500,
    "observation_identity_unavailable",
    "managed browser could not allocate an observation identity",
    WORKER_COMPLETION_UNKNOWN,
  );
}

function publicNode(row, usedRefs, privateTargets) {
  if (!row || typeof row !== "object" || Array.isArray(row)) {
    invalidState("observation target row must be an object");
  }
  if (!Object.hasOwn(row, "target")) {
    invalidState("observation target row is missing its private target");
  }
  if (
    row.target == null ||
    (typeof row.target !== "object" && typeof row.target !== "function")
  ) {
    invalidState("observation private target must be an object");
  }
  if (
    typeof row.role !== "string" ||
    !row.role.trim() ||
    row.role.length > MAX_ROLE_LENGTH
  ) {
    invalidState("observation role is invalid");
  }
  if (typeof row.name !== "string" || row.name.length > MAX_NAME_LENGTH) {
    invalidState("observation name is invalid");
  }
  if (row.disabled !== undefined && typeof row.disabled !== "boolean") {
    invalidState("observation disabled state is invalid");
  }
  if (row.truncated !== undefined && typeof row.truncated !== "boolean") {
    invalidState("observation truncation state is invalid");
  }

  const node = {
    role: row.role.trim(),
    name: row.name,
    disabled: row.disabled ?? false,
    truncated: row.truncated ?? false,
  };
  if (!node.truncated) {
    const elementRef = opaqueId("element", usedRefs);
    usedRefs.add(elementRef);
    privateTargets.set(elementRef, row.target);
    node.elementRef = elementRef;
  }
  return Object.freeze(node);
}

export function createObservationState(pageGeneration, targets, image) {
  if (!Number.isSafeInteger(pageGeneration) || pageGeneration < 1) {
    invalidState("page generation must be a positive safe integer");
  }
  if (!Array.isArray(targets) || targets.length > MAX_NODES) {
    invalidState("observation target count is invalid");
  }
  if (image !== undefined && (!image || !Number.isSafeInteger(image.width) || image.width < 1 ||
      !Number.isSafeInteger(image.height) || image.height < 1)) {
    invalidState("observation image dimensions are invalid");
  }

  const snapshotId = opaqueId("snapshot");
  const privateTargets = new Map();
  const usedRefs = new Set();
  const nodes = Object.freeze(
    targets.map((row) => publicNode(row, usedRefs, privateTargets)),
  );
  const observation = Object.freeze({
    pageGeneration,
    snapshotId,
    nodes,
  });
  PRIVATE_OBSERVATIONS.set(observation, {
    pageGeneration,
    snapshotId,
    targets: privateTargets,
    image: image ? Object.freeze({ width: image.width, height: image.height }) : null,
    handles: new Set([...privateTargets.values()].map(target => target.handle).filter(Boolean)),
    retired: false,
    pins: 0,
    disposalStarted: false,
    disposal: null,
    finishDisposal: null,
  });
  return observation;
}

export function assertObservationIdentity(observation, request = {}) {
  const state = observation && PRIVATE_OBSERVATIONS.get(observation);
  if (!state || state.retired) {
    stale("observation_required", "observe the page before using an element reference");
  }
  if (request.pageGeneration !== state.pageGeneration) {
    stale(
      "stale_observation_generation",
      "page generation changed; observe the page again",
      state.pageGeneration,
    );
  }
  if (request.snapshotId !== state.snapshotId) {
    stale("stale_snapshot", "snapshot changed; observe the page again");
  }
  return state;
}

export function resolveObservationTarget(observation, request = {}) {
  const state = assertObservationIdentity(observation, request);
  if (!state.targets.has(request.elementRef)) {
    stale("stale_element_ref", "element reference is not in the current snapshot");
  }
  return state.targets.get(request.elementRef);
}

export function invalidateObservationState(observation) {
  const state = observation && PRIVATE_OBSERVATIONS.get(observation);
  if (state && !state.retired) {
    // Authority dies synchronously; an already admitted action may still own
    // the handles until its physical operation finishes.
    state.retired = true;
    state.targets.clear();
    state.disposal = new Promise(resolve => { state.finishDisposal = resolve; });
    disposeIfUnpinned(state);
  }
  return null;
}

function disposeIfUnpinned(state) {
  if (!state.retired || state.pins || state.disposalStarted) return;
  state.disposalStarted = true;
  const handles = [...state.handles];
  state.handles.clear();
  // Closed documents can reject dispose. Still attempt every handle exactly
  // once, and never leave an unhandled rejection on a navigation event.
  void Promise.allSettled(handles.map(handle => Promise.resolve().then(() => handle.dispose?.())))
    .then(() => { state.finishDisposal(); state.finishDisposal = null; });
}

export async function disposeObservationState(observation) {
  const state = observation && PRIVATE_OBSERVATIONS.get(observation);
  invalidateObservationState(observation);
  if (state) await state.disposal;
}

export async function withObservationLease(observation, request, operation) {
  const state = assertObservationIdentity(observation, request);
  state.pins += 1;
  try {
    return await operation();
  } finally {
    state.pins -= 1;
    disposeIfUnpinned(state);
    if (state.retired && !state.pins) await state.disposal;
  }
}
