/** AllowedSelection v2. Keep JSON shape in sync with Rust session_migration::selection. */

export const SELECTION_SCHEMA_VERSION = 2;

export type SelectionFields = {
  messageText: boolean;
  toolCallName: boolean;
  toolCallInput: boolean;
  toolResultContent: boolean;
  compactionSummary: boolean;
  visibleReasoning: boolean;
};

export type AllowedSelection = {
  version: number;
  eventIds: string[];
  pinnedEventIds: string[];
  assetIds: string[];
  fields: SelectionFields;
  includeSubagents: boolean;
};

export type PreviewSelection = {
  previewId: string;
  leafId: string;
  selection: AllowedSelection;
};

export type SelectionSet = {
  version: number;
  entries: PreviewSelection[];
};

export type StoredSelection = {
  selection: AllowedSelection;
  requiresReprepare: boolean;
};

export type PlanSelectionBoundaryInput = {
  selected: Iterable<unknown>;
  previewIdByCandidate: Readonly<Record<string, unknown>>;
  selectionByCandidate: Readonly<Record<string, unknown>>;
};

export type ValidatedPlanSelection = {
  previewIds: string[];
  selectionSet: SelectionSet;
};

export const SELECTION_BOUNDARY_ERROR = "SELECTION_BOUNDARY_INVALID";

const defaultFields = (): SelectionFields => ({
  messageText: true,
  toolCallName: true,
  toolCallInput: true,
  toolResultContent: true,
  compactionSummary: true,
  visibleReasoning: false,
});

export function defaultSelection(): AllowedSelection {
  return {
    version: SELECTION_SCHEMA_VERSION,
    eventIds: [],
    pinnedEventIds: [],
    assetIds: [],
    fields: defaultFields(),
    includeSubagents: false,
  };
}

function uniqueSorted(ids: string[]): string[] {
  return [...new Set(ids.filter((id) => id.trim() !== ""))].sort();
}

function boundaryError(detail: string): never {
  throw new Error(`${SELECTION_BOUNDARY_ERROR}: ${detail}`);
}

function recordValue(value: unknown, label: string): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    boundaryError(`${label} must be an object`);
  }
  return value as Record<string, unknown>;
}

function exactKeys(
  value: Record<string, unknown>,
  expected: readonly string[],
  missingDetail: string,
  unknownDetail: string,
): void {
  const actual = Object.keys(value);
  if (expected.some((key) => !Object.prototype.hasOwnProperty.call(value, key))) {
    boundaryError(missingDetail);
  }
  if (actual.some((key) => !expected.includes(key))) {
    boundaryError(unknownDetail);
  }
}

function nonEmptyString(value: unknown, label: string): string {
  if (typeof value !== "string" || value.trim() === "") {
    boundaryError(`${label} must be non-empty`);
  }
  return value;
}

function strictIds(value: unknown, label: string): string[] {
  if (!Array.isArray(value)) boundaryError(`${label} ids must be an array`);
  const ids: string[] = [];
  const seen = new Set<string>();
  for (const item of value) {
    const id = nonEmptyString(item, `${label} id`);
    if (seen.has(id)) boundaryError(`duplicate ${label} id`);
    seen.add(id);
    ids.push(id);
  }
  return ids;
}

const SELECTION_KEYS = [
  "version",
  "eventIds",
  "pinnedEventIds",
  "assetIds",
  "fields",
  "includeSubagents",
] as const;

const FIELD_KEYS = [
  "messageText",
  "toolCallName",
  "toolCallInput",
  "toolResultContent",
  "compactionSummary",
  "visibleReasoning",
] as const;

function validateSelectionV2Raw(value: unknown): AllowedSelection {
  const selection = recordValue(value, "selection");
  exactKeys(
    selection,
    SELECTION_KEYS,
    "selection fields are incomplete",
    "selection has unknown fields",
  );
  if (selection.version !== SELECTION_SCHEMA_VERSION) {
    if (
      typeof selection.version === "number" &&
      selection.version > SELECTION_SCHEMA_VERSION
    ) {
      throw new Error(
        `IMPORT_SCHEMA_TOO_NEW: unsupported selection schema ${selection.version}`,
      );
    }
    boundaryError(`selection version must be ${SELECTION_SCHEMA_VERSION}`);
  }
  const eventIds = strictIds(selection.eventIds, "event");
  const pinnedEventIds = strictIds(selection.pinnedEventIds, "pinned event");
  const assetIds = strictIds(selection.assetIds, "asset");
  const selected = new Set(eventIds);
  for (const pinnedId of pinnedEventIds) {
    if (!selected.has(pinnedId)) {
      boundaryError(`pinned event ${pinnedId} is not selected`);
    }
  }
  const fields = recordValue(selection.fields, "selection fields");
  exactKeys(
    fields,
    FIELD_KEYS,
    "selection fields are incomplete",
    "selection fields have unknown fields",
  );
  const checkedFields = {} as SelectionFields;
  for (const field of FIELD_KEYS) {
    if (typeof fields[field] !== "boolean") {
      boundaryError(`selection field ${field} must be boolean`);
    }
    checkedFields[field] = fields[field];
  }
  if (typeof selection.includeSubagents !== "boolean") {
    boundaryError("includeSubagents must be boolean");
  }
  return {
    version: SELECTION_SCHEMA_VERSION,
    eventIds,
    pinnedEventIds,
    assetIds,
    fields: checkedFields,
    includeSubagents: selection.includeSubagents,
  };
}

export function canonicalizeSelection(selection: AllowedSelection): AllowedSelection {
  return {
    version: selection.version,
    eventIds: uniqueSorted(selection.eventIds),
    pinnedEventIds: uniqueSorted(selection.pinnedEventIds),
    assetIds: uniqueSorted(selection.assetIds),
    fields: { ...selection.fields },
    includeSubagents: selection.includeSubagents,
  };
}

export function canonicalizeSelectionSet(selectionSet: SelectionSet): SelectionSet {
  return {
    version: selectionSet.version,
    entries: selectionSet.entries
      .map((entry) => ({
        previewId: entry.previewId,
        leafId: entry.leafId,
        selection: canonicalizeSelection(entry.selection),
      }))
      .sort(
        (left, right) =>
          left.previewId.localeCompare(right.previewId) ||
          left.leafId.localeCompare(right.leafId),
      ),
  };
}

/**
 * Fail-closed wire boundary for live SelectionSet v2 values. Validation must
 * happen before canonicalization so duplicate or empty grants cannot vanish.
 */
export function validateSelectionSetV2Raw(value: unknown): SelectionSet {
  const selectionSet = recordValue(value, "selection set");
  exactKeys(
    selectionSet,
    ["version", "entries"],
    "selection set fields are incomplete",
    "selection set has unknown fields",
  );
  if (selectionSet.version !== SELECTION_SCHEMA_VERSION) {
    if (
      typeof selectionSet.version === "number" &&
      selectionSet.version > SELECTION_SCHEMA_VERSION
    ) {
      throw new Error(
        `IMPORT_SCHEMA_TOO_NEW: unsupported selection set schema ${selectionSet.version}`,
      );
    }
    boundaryError(`selection set version must be ${SELECTION_SCHEMA_VERSION}`);
  }
  if (!Array.isArray(selectionSet.entries)) {
    boundaryError("selection set entries must be an array");
  }
  if (selectionSet.entries.length === 0) {
    boundaryError("selection set requires at least one entry");
  }
  const seenPreviews = new Set<string>();
  const entries = selectionSet.entries.map((rawEntry, index): PreviewSelection => {
    const entry = recordValue(rawEntry, `selection entry ${index}`);
    exactKeys(
      entry,
      ["previewId", "leafId", "selection"],
      `selection entry ${index} fields are incomplete`,
      `selection entry ${index} has unknown fields`,
    );
    const previewId = nonEmptyString(entry.previewId, "preview id");
    if (seenPreviews.has(previewId)) boundaryError("duplicate preview id");
    seenPreviews.add(previewId);
    return {
      previewId,
      leafId: nonEmptyString(entry.leafId, "leaf id"),
      selection: validateSelectionV2Raw(entry.selection),
    };
  });
  return { version: SELECTION_SCHEMA_VERSION, entries };
}

/**
 * Convert the three independently-owned wizard maps into one canonical v2
 * request only after proving that their candidate ownership is identical.
 */
export function validatePlanSelectionBoundary(
  input: PlanSelectionBoundaryInput,
): ValidatedPlanSelection {
  const selected: string[] = [];
  const seenCandidates = new Set<string>();
  for (const value of input.selected) {
    const candidateId = nonEmptyString(value, "selected candidate id");
    if (seenCandidates.has(candidateId)) boundaryError("duplicate selected candidate id");
    seenCandidates.add(candidateId);
    selected.push(candidateId);
  }
  if (selected.length === 0) boundaryError("no candidates are selected");

  const previewKeys = Object.keys(input.previewIdByCandidate).sort();
  const selectionKeys = Object.keys(input.selectionByCandidate).sort();
  const selectedKeys = [...selected].sort();
  const sameKeys = (left: string[], right: string[]) =>
    left.length === right.length && left.every((key, index) => key === right[index]);
  if (!sameKeys(selectedKeys, previewKeys)) {
    boundaryError("selected and preview candidates differ");
  }
  if (!sameKeys(selectedKeys, selectionKeys)) {
    boundaryError("selected and selection candidates differ");
  }

  const rawEntries = selectedKeys.map((candidateId) => {
    const previewId = nonEmptyString(
      input.previewIdByCandidate[candidateId],
      `candidate ${candidateId} preview id`,
    );
    const candidateSelection = recordValue(
      input.selectionByCandidate[candidateId],
      `candidate ${candidateId} selection`,
    );
    exactKeys(
      candidateSelection,
      ["leafId", "selection"],
      `candidate ${candidateId} selection fields are incomplete`,
      `candidate ${candidateId} selection has unknown fields`,
    );
    return {
      previewId,
      leafId: candidateSelection.leafId,
      selection: candidateSelection.selection,
    };
  });
  const validated = validateSelectionSetV2Raw({
    version: SELECTION_SCHEMA_VERSION,
    entries: rawEntries,
  });
  const selectionSet = canonicalizeSelectionSet(validated);
  return {
    previewIds: selectionSet.entries.map((entry) => entry.previewId),
    selectionSet,
  };
}

export function canonicalJson(value: unknown): string {
  const write = (v: unknown): string => {
    if (v === null) return "null";
    if (v === true) return "true";
    if (v === false) return "false";
    if (typeof v === "number" && Number.isFinite(v)) return String(v);
    if (typeof v === "string") return JSON.stringify(v);
    if (Array.isArray(v)) return `[${v.map(write).join(",")}]`;
    if (v && typeof v === "object") {
      const rec = v as Record<string, unknown>;
      const keys = Object.keys(rec).sort();
      return `{${keys.map((k) => `${JSON.stringify(k)}:${write(rec[k])}`).join(",")}}`;
    }
    return "null";
  };
  return write(value);
}

function asStringIds(value: unknown): string[] {
  return Array.isArray(value)
    ? value.filter((item): item is string => typeof item === "string")
    : [];
}

export function parseStoredSelection(value: unknown): StoredSelection {
  if (!value || typeof value !== "object") {
    throw new Error("SCHEMA_UNSUPPORTED: invalid selection");
  }
  const rec = value as Record<string, unknown>;
  const version = typeof rec.version === "number" ? rec.version : 0;
  if (version > SELECTION_SCHEMA_VERSION) {
    throw new Error(`IMPORT_SCHEMA_TOO_NEW: unsupported selection schema ${version}`);
  }
  const v1 =
    version === 1 ||
    (version === 0 && ("includeReasoning" in rec || "includeImages" in rec));
  if (v1) {
    const selection = canonicalizeSelection({
      ...defaultSelection(),
      eventIds: asStringIds(rec.eventIds),
      pinnedEventIds: asStringIds(rec.pinnedEventIds),
      assetIds: [],
      includeSubagents: rec.includeSubagents === true,
      fields: {
        ...defaultFields(),
        visibleReasoning: rec.includeReasoning === true,
      },
    });
    return { selection, requiresReprepare: true };
  }
  const selection = canonicalizeSelection(validateSelectionV2Raw(value));
  return { selection, requiresReprepare: false };
}
