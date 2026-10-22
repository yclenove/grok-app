/** Map host error prefixes to i18n keys. */

import type { MessageKey } from "@/i18n";

const MAP: Record<string, MessageKey> = {
  SOURCE_NOT_FOUND: "sessionImport.err.sourceNotFound",
  SOURCE_PERMISSION_DENIED: "sessionImport.err.permissionDenied",
  SOURCE_BUSY: "sessionImport.err.sourceBusy",
  SOURCE_CHANGED: "sessionImport.err.sourceChanged",
  SNAPSHOT_UNAVAILABLE: "sessionImport.err.snapshotUnavailable",
  FORMAT_UNSUPPORTED: "sessionImport.err.formatUnsupported",
  SCHEMA_UNSUPPORTED: "sessionImport.err.schemaUnsupported",
  PARTIAL_HISTORY: "sessionImport.err.partialHistory",
  WORKSPACE_UNRESOLVED: "sessionImport.err.workspaceUnresolved",
  MAPPING_CONFLICT: "sessionImport.err.mappingConflict",
  RESOURCE_LIMIT: "sessionImport.err.resourceLimit",
  DISK_SPACE_LOW: "sessionImport.err.diskSpaceLow",
  PLAN_STALE: "sessionImport.err.planStale",
  JOB_STATE_CHANGED: "sessionImport.err.jobStateChanged",
  JOB_ACTION_CONFLICT: "sessionImport.err.jobActionConflict",
  SELECTION_BOUNDARY_INVALID: "sessionImport.err.selectionInvalid",
  IMPORTED_HISTORY_READ_ONLY: "sessionImport.err.readOnly",
  HISTORY_ACCESS_UNAVAILABLE: "sessionImport.err.historyUnavailable",
  HANDOFF_CHANGED: "sessionImport.err.handoffChanged",
  DISPATCH_UNKNOWN: "sessionImport.err.dispatchUnknown",
  IMPORT_SCHEMA_TOO_NEW: "sessionImport.err.schemaTooNew",
};

const ISSUES: Record<string, MessageKey> = {
  "empty-jsonl": "sessionImport.diagnostic.empty",
  "trailing-incomplete": "sessionImport.diagnostic.trailingIncomplete",
  "broken-record": "sessionImport.diagnostic.brokenRecord",
  "invalid-utf8": "sessionImport.diagnostic.invalidUtf8",
  "binary-record": "sessionImport.diagnostic.binary",
  "jsonl-record-too-large": "sessionImport.diagnostic.oversizedLine",
  "unknown-record": "sessionImport.diagnostic.unknownRecord",
  "unknown-role": "sessionImport.diagnostic.unknownRecord",
  "unknown-block": "sessionImport.diagnostic.unknownRecord",
  "unsupported-item": "sessionImport.diagnostic.unknownRecord",
};

export function importIssueKey(code: string): MessageKey | null {
  return Object.hasOwn(ISSUES, code) ? ISSUES[code] : null;
}

export function importErrorKey(raw: string): MessageKey {
  const code = raw.split(":")[0]?.trim() ?? "";
  const reason = raw.slice(raw.indexOf(":") + 1).trim();
  if ((code === "FORMAT_UNSUPPORTED" || code === "RESOURCE_LIMIT") && raw.includes(":")) {
    const key = importIssueKey(reason);
    if (key) return key;
  }
  return Object.hasOwn(MAP, code) ? MAP[code] : "sessionImport.err.generic";
}

export function importErrorCode(raw: string): string {
  return raw.split(":")[0]?.trim() ?? "";
}
