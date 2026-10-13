import { ComputerTaskUploadChoice, EMPTY_UPLOAD_STATE } from "./taskUpload";
import { getTaskSessionSnapshot, subscribeTaskSession } from "./taskSource";

type Entry = { actions: ComputerTaskUploadChoice; releaseHost: () => void; releaseIdle: () => void };
const entries = new Map<string, Entry>();
export function getTaskUploadActions(sessionId: string | null) {
  return sessionId ? entries.get(sessionId)?.actions : undefined;
}
export function getTaskUploadSnapshot(sessionId: string | null) {
  return getTaskUploadActions(sessionId)?.getSnapshot() ?? EMPTY_UPLOAD_STATE;
}
export function subscribeTaskUpload(sessionId: string | null, listener: () => void) {
  if (!sessionId) return () => {};
  let entry = entries.get(sessionId);
  if (!entry) {
    const actions = new ComputerTaskUploadChoice(sessionId);
    entry = { actions, releaseHost: () => {}, releaseIdle: () => {} };
    const original = entry;
    entries.set(sessionId, original);
    entry.releaseHost = subscribeTaskSession(sessionId, () => actions.update(getTaskSessionSnapshot(sessionId)));
    actions.update(getTaskSessionSnapshot(sessionId));
  }
  const original = entry;
  // The native chooser may outlive all visible cards. Do not delete its lock
  // or start a second chooser when the user reopens the same session.
  original.releaseIdle();
  const release = entry.actions.subscribe(listener);
  let active = true;
  return () => {
    if (!active) return;
    active = false;
    release();
    if (original.actions.hasSubscribers()) return;
    original.actions.release();
    const retire = () => {
      if (original.actions.hasPendingCall()) return;
      original.releaseIdle();
      original.releaseHost();
      if (entries.get(sessionId) === original) entries.delete(sessionId);
    };
    if (original.actions.hasPendingCall()) original.releaseIdle = original.actions.subscribe(retire);
    else retire();
  };
}
