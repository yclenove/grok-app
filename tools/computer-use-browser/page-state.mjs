import { randomUUID } from "node:crypto";

import { assertObservationIdentity, invalidateObservationState } from "./observation-state.mjs";
import {
  WORKER_COMPLETION_NOT_STARTED,
  workerException,
} from "./worker-errors.mjs";

function safePageUrl(page) {
  try {
    return String(page.url());
  } catch {
    return "about:blank";
  }
}

function rejectPage(code, message, currentPageGeneration) {
  throw workerException(
    code === "page_closed" ? 404 : 409,
    code,
    message,
    WORKER_COMPLETION_NOT_STARTED,
    currentPageGeneration,
  );
}

function retirePageObservation(state) {
  if (!state || typeof state !== "object") return null;
  if (state.observation) invalidateObservationState(state.observation);
  state.observation = null;
  return null;
}

export function invalidatePageObservation(state) {
  if (!state || typeof state !== "object") return null;
  // Retire captures that have not published yet as well as the current map.
  // This covers pause/unknown/explicit revocation without any frame event.
  state.observationEpoch = {};
  return retirePageObservation(state);
}

function mainFrameOf(page) {
  try {
    return page.mainFrame();
  } catch {
    return null;
  }
}

function frameChanged(state, frame) {
  if (state.closed) return;
  state.frameRevision += 1;
  invalidatePageObservation(state);
  if (frame !== mainFrameOf(state.page)) return;

  state.lastUrl = safePageUrl(state.page);
  if (!state.published) return;
  if (state.generation >= Number.MAX_SAFE_INTEGER) {
    state.identityExhausted = true;
    return;
  }
  state.generation += 1;
}

export function registerPageState(slot, page) {
  const known = slot.pageIds.get(page);
  if (known) return slot.pages.get(known) ?? null;
  const state = {
    id: `page-${randomUUID()}`,
    page,
    generation: 1,
    lastUrl: page.url(),
    observation: null,
    captureInFlight: false,
    mutationInFlight: false,
    observationEpoch: {},
    published: false,
    closed: false,
    identityExhausted: false,
    frameRevision: 0,
  };
  slot.pageIds.set(page, state.id);
  slot.pages.set(state.id, state);
  page.on("framenavigated", (frame) => frameChanged(state, frame));
  page.on("frameattached", (frame) => frameChanged(state, frame));
  page.on("framedetached", (frame) => frameChanged(state, frame));
  page.once("close", () => {
    if (state.closed) return;
    state.closed = true;
    state.frameRevision += 1;
    invalidatePageObservation(state);
    slot.pages.delete(state.id);
  });
  return state;
}

export function publishPageState(state) {
  if (!state || state.closed || state.page.isClosed()) {
    rejectPage("page_closed", "page is closed or unknown");
  }
  if (state.identityExhausted) {
    rejectPage(
      "page_generation_exhausted",
      "page identity must be rebuilt before it can be observed",
      state.generation,
    );
  }
  state.published = true;
  state.lastUrl = safePageUrl(state.page);
  return state;
}

export function currentPageObservation(state) {
  if (!state || state.closed) return null;
  return state.observation ?? null;
}

export function capturePageStateIdentity(state) {
  publishPageState(state);
  return Object.freeze({
    pageId: state.id,
    pageGeneration: state.generation,
    frameRevision: state.frameRevision,
  });
}

export function pageStateIdentityMatches(state, identity) {
  return Boolean(
    state &&
      identity &&
      !state.closed &&
      !state.identityExhausted &&
      state.id === identity.pageId &&
      state.generation === identity.pageGeneration &&
      state.frameRevision === identity.frameRevision,
  );
}

// Capture consistency is stricter than document identity. A click/fill/scroll
// can change the page without any frame event. Keep this epoch separate so an
// action's own dispatch does not invalidate its prepared drag-document guard.
export function capturePageObservationIdentity(state) {
  if (state?.mutationInFlight) {
    rejectPage("page_mutation_in_flight", "page input is still in progress");
  }
  return Object.freeze({ ...capturePageStateIdentity(state), observationEpoch: state.observationEpoch });
}

export function pageObservationIdentityMatches(state, identity) {
  return pageStateIdentityMatches(state, identity) && !state.mutationInFlight
    && state.observationEpoch === identity.observationEpoch;
}

export function replacePageObservation(state, observation) {
  publishPageState(state);
  if (
    !observation ||
    typeof observation !== "object" ||
    observation.pageGeneration !== state.generation
  ) {
    if (observation && typeof observation === "object") {
      invalidateObservationState(observation);
    }
    rejectPage(
      "stale_observation_generation",
      "observation generation does not match the current page",
      state.generation,
    );
  }
  // Publishing the capture is not an external revocation of that capture.
  if (state.observation !== observation) retirePageObservation(state);
  state.observation = observation;
  return observation;
}

export async function runPageMutation(state, operation, expectedObservation) {
  publishPageState(state);
  if (state.mutationInFlight) {
    rejectPage("page_mutation_in_flight", "page input is still in progress");
  }
  if (expectedObservation !== undefined) {
    if (state.observation !== expectedObservation) {
      rejectPage("stale_snapshot", "observation changed while preparing the action");
    }
    assertObservationIdentity(expectedObservation, {
      pageGeneration: state.generation, snapshotId: expectedObservation.snapshotId,
    });
  }
  state.mutationInFlight = true;
  // The retirement epoch cannot wrap and accidentally revalidate a capture.
  invalidatePageObservation(state);
  try {
    return await operation();
  } finally {
    state.mutationInFlight = false;
  }
}
