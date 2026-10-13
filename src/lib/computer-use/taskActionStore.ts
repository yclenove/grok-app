import {
  ComputerTaskActions, EMPTY_TASK_ACTION_STATE,
} from "./taskActions";
import { getTaskSessionSnapshot, subscribeTaskSession } from "./taskSource";

type Entry = { actions: ComputerTaskActions; releaseHost: () => void };
const entries = new Map<string, Entry>();

export function getTaskActions(sessionId: string | null) {
  return sessionId ? entries.get(sessionId)?.actions : undefined;
}

export function getTaskActionSnapshot(sessionId: string | null) {
  return getTaskActions(sessionId)?.getSnapshot() ?? EMPTY_TASK_ACTION_STATE;
}

/** A card and panel share one transaction, including under StrictMode.
 * The last release cancels its picker/observer, never the task grant. */
export function subscribeTaskActions(sessionId: string | null, listener: () => void) {
  if (!sessionId) return () => {};
  let entry = entries.get(sessionId);
  if (!entry) {
    const actions = new ComputerTaskActions(sessionId);
    entry = { actions, releaseHost: () => {} };
    entries.set(sessionId, entry);
    entry.releaseHost = subscribeTaskSession(sessionId, () => actions.update(getTaskSessionSnapshot(sessionId)));
    actions.update(getTaskSessionSnapshot(sessionId));
  }
  const release = entry.actions.subscribe(listener);
  const original = entry;
  let active = true;
  return () => {
    if (!active) return;
    active = false;
    release();
    if (!original.actions.hasSubscribers()) {
      original.releaseHost();
      if (entries.get(sessionId) === original) entries.delete(sessionId);
    }
  };
}
