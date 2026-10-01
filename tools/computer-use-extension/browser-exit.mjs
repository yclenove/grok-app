import { exactKeys, uuid } from "./completion-scope.mjs";

const PHASES = new Set(["prepared", "physicallySettled"]);
const HOLD = Object.freeze({ release: false, result: null, actions: 0 });

// Cleanup metadata only. Callers must not pass Bearer, page text, commands, or proof keys.
function cleanupFields(input) {
  if (input === null || typeof input !== "object" || Array.isArray(input)) {
    throw new Error("browserExitUnavailable");
  }
  if (Object.hasOwn(input, "version") && input.version !== 1) throw new Error("browserExitUnavailable");
  const { version: _version, ...fields } = input;
  return fields;
}

export function durableCleanupRecord(input) {
  const fields = cleanupFields(input);
  if (!exactKeys(fields, ["browserId", "requestId", "documentId", "phase"])
    || !uuid(fields.browserId) || !uuid(fields.requestId)
    || !/^[0-9a-f]{32}$/i.test(String(fields.documentId)) || !PHASES.has(fields.phase)) {
    throw new Error("browserExitUnavailable");
  }
  return Object.freeze({
    version: 1,
    browserId: fields.browserId,
    requestId: fields.requestId,
    documentId: fields.documentId,
    phase: fields.phase,
  });
}

export function durableCleanupFromJournal(record, browserId) {
  return durableCleanupRecord({
    browserId,
    requestId: record?.proof?.binding?.requestId,
    documentId: record?.proof?.binding?.documentId,
    phase: record?.phase,
  });
}

export function assertCleanupHasNoSecrets(record) {
  const banned = new Set([
    "bearer", "token", "cookie", "completionKey", "command", "input", "text",
    "html", "url", "endpoint", "page", "authorization",
  ]);
  if (record === null || typeof record !== "object" || Array.isArray(record)) {
    throw new Error("browserExitUnavailable");
  }
  if (Object.keys(record).some((key) => banned.has(key))) throw new Error("browserExitUnavailable");
  const encoded = JSON.stringify(record);
  if (/bearer|cookie|completionkey|authorization|eyJ/i.test(encoded)) {
    throw new Error("browserExitUnavailable");
  }
  return record;
}

// Release the old owner only when the new startup identity, a fresh pairing,
// and the exact pending/proof/physical record all agree. The result is never
// applied or verified, and it never replays the action.
export function decideBrowserExit(input) {
  if (!input || input.writeSucceeded !== true) return HOLD;
  if (input.newShare || input.documentMissing || input.timedOut || input.storageLost || input.hostAbsent) {
    return HOLD;
  }
  let record;
  try {
    record = assertCleanupHasNoSecrets(durableCleanupRecord(input.record || {}));
  } catch {
    return HOLD;
  }
  if (!uuid(input.currentId) || !uuid(input.previousId) || input.currentId === input.previousId) return HOLD;
  if (input.paired !== true) return HOLD;
  if (record.browserId !== input.previousId) return HOLD;
  const pending = input.pending;
  if (!pending || pending.browserId !== input.previousId || pending.requestId !== record.requestId
    || pending.documentId !== record.documentId || input.physical !== record.phase) {
    return HOLD;
  }
  if (record.phase === "physicallySettled") {
    return Object.freeze({ release: true, result: "cleanup", actions: 0 });
  }
  return Object.freeze({ release: true, result: "unknown", actions: 0 });
}
