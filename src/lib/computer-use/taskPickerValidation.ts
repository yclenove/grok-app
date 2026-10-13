import type { ComputerTaskKey } from "@/lib/api/computerTask";
import type { ComputerTaskPickerStarted, ComputerTaskPickerView, ComputerTaskPickerPreparation, ComputerTaskPickerSurface } from "@/lib/api/computerTaskPicker";

function sameKey(a: ComputerTaskKey, b: ComputerTaskKey) {
  return a && a.connectionId === b.connectionId && a.taskId === b.taskId && a.requestId === b.requestId && a.revision === b.revision;
}
export function validatePickerStarted(picker: ComputerTaskPickerStarted, sessionId: string, key: ComputerTaskKey, surface: ComputerTaskPickerSurface) {
  if (!picker || picker.sessionId !== sessionId || !sameKey(picker.key, key) || picker.surface !== surface
    || typeof picker.pickerId !== "string" || !picker.pickerId || picker.pickerId.length > 256
    || !Number.isSafeInteger(picker.maxSelection) || picker.maxSelection < 1 || picker.maxSelection > 8) throw new Error("picker identity changed");
}
function validateOriginal(view: ComputerTaskPickerStarted, picker: ComputerTaskPickerStarted) {
  validatePickerStarted(view, picker.sessionId, picker.key, picker.surface);
  if (view.pickerId !== picker.pickerId || view.maxSelection !== picker.maxSelection) throw new Error("original picker changed");
}
export function validatePickerView(view: ComputerTaskPickerView, picker: ComputerTaskPickerStarted) {
  validateOriginal(view, picker);
  if (!Array.isArray(view.candidates) || view.candidates.length > 128 || typeof view.truncated !== "boolean"
    || new Set(view.candidates.map(row => row.candidateId)).size !== view.candidates.length
    || view.candidates.some(row => !row || typeof row.candidateId !== "string" || !row.candidateId || row.candidateId.length > 512
      || row.surface !== picker.surface || typeof row.title !== "string" || typeof row.appName !== "string" || typeof row.kind !== "string")) throw new Error("picker candidates changed");
}
export function validatePickerPreparation(view: ComputerTaskPickerPreparation, picker: ComputerTaskPickerStarted) {
  validateOriginal(view, picker);
  if (view.surface !== "existing-tabs" || typeof view.ready !== "boolean" || typeof view.pairingConfirmed !== "boolean"
    || [view.availableTabs, view.incompatibleTabs, view.borrowedTabs].some(count => !Number.isSafeInteger(count) || count < 0)
    || view.ready !== (view.pairing === null && view.availableTabs > 0)) throw new Error("picker preparation changed");
  if (view.pairing === null) return;
  const ch = view.pairing;
  if (!ch || [ch.nonce, ch.instanceId, ch.verificationCode, ch.installedExtensionId].some(value => typeof value !== "string" || !value || value.length > 256)
    || !Number.isSafeInteger(ch.expiresAtMs) || ch.expiresAtMs < 1) throw new Error("picker challenge changed");
  // The renderer never chooses an endpoint. Even a malformed Host reply must
  // not present a remote address or URL credentials as pairing instructions.
  const url = new URL(ch.endpoint);
  if (url.protocol !== "http:" || !["127.0.0.1", "localhost", "[::1]"].includes(url.hostname)
    || !url.port || url.username || url.password || url.search || url.hash || url.pathname !== "/") throw new Error("picker endpoint changed");
}
