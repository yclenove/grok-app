import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test } from "vitest";
import {
  canonicalJson,
  canonicalizeSelection,
  defaultSelection,
  parseStoredSelection,
  SELECTION_SCHEMA_VERSION,
  validatePlanSelectionBoundary,
  validateSelectionSetV2Raw,
} from "./selection";

const goldenPath = join(
  dirname(fileURLToPath(import.meta.url)),
  "selectionV2.golden.json",
);

test("v2 golden JSON matches Rust canonical field set", () => {
  const raw = JSON.parse(readFileSync(goldenPath, "utf8"));
  const stored = parseStoredSelection(raw);
  expect(stored.requiresReprepare).toBe(false);
  expect(stored.selection.version).toBe(SELECTION_SCHEMA_VERSION);
  expect(stored.selection.eventIds).toEqual(["e1", "e2"]);
  expect(stored.selection.pinnedEventIds).toEqual(["e1"]);
  expect(stored.selection.assetIds).toEqual(["asset-a"]);
  expect(stored.selection.fields.toolCallInput).toBe(false);
  expect(stored.selection.fields.visibleReasoning).toBe(false);
  const expected = canonicalJson({
    assetIds: ["asset-a"],
    eventIds: ["e1", "e2"],
    fields: {
      compactionSummary: true,
      messageText: true,
      toolCallInput: false,
      toolCallName: true,
      toolResultContent: true,
      visibleReasoning: false,
    },
    includeSubagents: false,
    pinnedEventIds: ["e1"],
    version: 2,
  });
  expect(canonicalJson(canonicalizeSelection(stored.selection))).toBe(expected);
});

test("v1 draft stays minimal and requires reprepare", () => {
  const stored = parseStoredSelection({
    eventIds: ["e1", "e2"],
    pinnedEventIds: ["e1"],
    includeReasoning: true,
    includeSubagents: true,
    includeImages: true,
  });
  expect(stored.requiresReprepare).toBe(true);
  expect(stored.selection.fields.visibleReasoning).toBe(true);
  expect(stored.selection.includeSubagents).toBe(true);
  expect(stored.selection.assetIds).toEqual([]);
});

test("too-new selection is rejected", () => {
  expect(() => parseStoredSelection({ version: 3, eventIds: ["e1"] })).toThrow(
    /IMPORT_SCHEMA_TOO_NEW/,
  );
});

test("v2 stored selections require the exact raw field set before canonicalization", () => {
  const valid = defaultSelection();
  expect(() =>
    parseStoredSelection({ ...valid, eventIds: ["e1", "e1"] }),
  ).toThrow(/SELECTION_BOUNDARY_INVALID: duplicate event id/);
  expect(() => {
    const { fields: _fields, ...missing } = valid;
    return parseStoredSelection(missing);
  }).toThrow(/SELECTION_BOUNDARY_INVALID: selection fields are incomplete/);
  expect(() => parseStoredSelection({ ...valid, unexpected: true })).toThrow(
    /SELECTION_BOUNDARY_INVALID: selection has unknown fields/,
  );
  expect(() =>
    parseStoredSelection({
      ...valid,
      fields: { ...valid.fields, toolCallInput: "yes" },
    }),
  ).toThrow(/SELECTION_BOUNDARY_INVALID: selection field toolCallInput must be boolean/);
  expect(() =>
    parseStoredSelection({
      ...valid,
      eventIds: ["e1"],
      pinnedEventIds: ["e2"],
    }),
  ).toThrow(/SELECTION_BOUNDARY_INVALID: pinned event e2 is not selected/);
});

test("plan boundary requires selected, preview, and selection keys to match exactly", () => {
  const selection = defaultSelection();
  const base = {
    selected: new Set(["candidate-a"]),
    previewIdByCandidate: { "candidate-a": "preview-a" },
    selectionByCandidate: {
      "candidate-a": { leafId: "leaf-a", selection },
    },
  };
  expect(validatePlanSelectionBoundary(base)).toEqual({
    previewIds: ["preview-a"],
    selectionSet: {
      version: 2,
      entries: [
        { previewId: "preview-a", leafId: "leaf-a", selection },
      ],
    },
  });
  expect(() =>
    validatePlanSelectionBoundary({
      ...base,
      selected: new Set(["candidate-a", "candidate-b"]),
    }),
  ).toThrow(/SELECTION_BOUNDARY_INVALID: selected and preview candidates differ/);
  expect(() =>
    validatePlanSelectionBoundary({
      ...base,
      selectionByCandidate: {},
    }),
  ).toThrow(/SELECTION_BOUNDARY_INVALID: selected and selection candidates differ/);
});

test("live v2 selection sets require at least one entry", () => {
  expect(() =>
    validateSelectionSetV2Raw({ version: SELECTION_SCHEMA_VERSION, entries: [] }),
  ).toThrow(/SELECTION_BOUNDARY_INVALID: selection set requires at least one entry/);
});

test("plan boundary rejects missing, duplicate, empty, and unknown raw v2 state", () => {
  const selection = defaultSelection();
  const validate = (
    nextSelection: unknown,
    previewIdByCandidate: Record<string, unknown> = { a: "preview-a" },
  ) =>
    validatePlanSelectionBoundary({
      selected: ["a"],
      previewIdByCandidate,
      selectionByCandidate: {
        a: { leafId: "leaf-a", selection: nextSelection },
      },
    });

  expect(() => validate({ ...selection, eventIds: ["e1", "e1"] })).toThrow(
    /SELECTION_BOUNDARY_INVALID: duplicate event id/,
  );
  expect(() => validate({ ...selection, assetIds: [" "] })).toThrow(
    /SELECTION_BOUNDARY_INVALID: asset id must be non-empty/,
  );
  expect(() => validate({ ...selection, includeSubagents: 1 })).toThrow(
    /SELECTION_BOUNDARY_INVALID: includeSubagents must be boolean/,
  );
  expect(() =>
    validate(selection, { a: "same-preview", b: "same-preview" }),
  ).toThrow(/SELECTION_BOUNDARY_INVALID: selected and preview candidates differ/);
  expect(() =>
    validatePlanSelectionBoundary({
      selected: ["a", "b"],
      previewIdByCandidate: { a: "same-preview", b: "same-preview" },
      selectionByCandidate: {
        a: { leafId: "leaf-a", selection },
        b: { leafId: "leaf-b", selection },
      },
    }),
  ).toThrow(/SELECTION_BOUNDARY_INVALID: duplicate preview id/);
  expect(() =>
    validatePlanSelectionBoundary({
      selected: ["a"],
      previewIdByCandidate: { a: "preview-a" },
      selectionByCandidate: {
        a: { leafId: "", selection, unexpected: true },
      },
    }),
  ).toThrow(/SELECTION_BOUNDARY_INVALID: candidate a selection has unknown fields/);
});

test("plan boundary validates raw IDs before returning a canonical set", () => {
  const selection = {
    ...defaultSelection(),
    eventIds: ["e2", "e1"],
    pinnedEventIds: ["e2"],
    assetIds: ["asset-b", "asset-a"],
  };
  const result = validatePlanSelectionBoundary({
    selected: ["b", "a"],
    previewIdByCandidate: { a: "preview-a", b: "preview-b" },
    selectionByCandidate: {
      a: { leafId: "leaf-a", selection },
      b: { leafId: "leaf-b", selection: defaultSelection() },
    },
  });

  expect(result.previewIds).toEqual(["preview-a", "preview-b"]);
  expect(result.selectionSet.entries[0]?.selection.eventIds).toEqual(["e1", "e2"]);
  expect(result.selectionSet.entries[0]?.selection.assetIds).toEqual([
    "asset-a",
    "asset-b",
  ]);
});
