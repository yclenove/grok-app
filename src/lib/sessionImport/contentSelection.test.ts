import { expect, test } from "vitest";
import {
  emptyContentSelection,
  reduceContentSelection,
  selectionFromState,
  type PreviewEventRow,
} from "./contentSelection";
import { defaultSelection } from "./selection";

const row = (id: string, extra: Partial<PreviewEventRow> = {}): PreviewEventRow => ({
  eventId: id,
  kind: "message",
  text: id,
  selectable: true,
  defaultSelected: extra.defaultSelected ?? true,
  ...extra,
});

test("reset keeps default selected events and ignores a later page from another preview", () => {
  let state = reduceContentSelection(emptyContentSelection(), {
    type: "reset",
    previewId: "p1",
    leaf: "leaf-a",
    leaves: ["leaf-a", "leaf-b"],
    events: [row("e1"), row("e2", { defaultSelected: false })],
    nextCursor: "c1",
    archiveCount: 9,
  });
  expect(state.selectedEventIds).toEqual(["e1"]);
  expect(state.modelCount).toBe(1);
  expect(state.archiveCount).toBe(9);
  const gen = state.generation;
  state = reduceContentSelection(state, {
    type: "appendPage",
    previewId: "p-other",
    generation: gen,
    events: [row("late")],
    nextCursor: null,
  });
  expect(state.events.map((e) => e.eventId)).toEqual(["e1", "e2"]);
  state = reduceContentSelection(state, {
    type: "appendPage",
    previewId: "p1",
    generation: gen - 1,
    events: [row("stale")],
    nextCursor: null,
  });
  expect(state.events.map((e) => e.eventId)).toEqual(["e1", "e2"]);
  state = reduceContentSelection(state, {
    type: "appendPage",
    previewId: "p1",
    generation: gen,
    events: [row("e3")],
    nextCursor: null,
  });
  expect(state.events.map((e) => e.eventId)).toEqual(["e1", "e2", "e3"]);
  expect(state.selectedEventIds).toEqual(["e1", "e3"]);
});

test("select and invert page keep off-page ids, switching leaf clears the old set", () => {
  let state = reduceContentSelection(emptyContentSelection(), {
    type: "reset",
    previewId: "p1",
    leaf: "leaf-a",
    leaves: ["leaf-a", "leaf-b"],
    events: [row("e1"), row("e2"), row("sys", { selectable: false, defaultSelected: false })],
    nextCursor: null,
  });
  state = reduceContentSelection(state, { type: "invertPage" });
  expect(state.selectedEventIds.sort()).toEqual([]);
  state = reduceContentSelection(state, { type: "selectPage" });
  expect(state.selectedEventIds.sort()).toEqual(["e1", "e2"]);
  state = reduceContentSelection(state, { type: "toggleEvent", eventId: "sys" });
  expect(state.selectedEventIds).not.toContain("sys");
  state = reduceContentSelection(state, { type: "requestLeaf", leaf: "leaf-b" });
  expect(state.pendingLeaf).toBe("leaf-b");
  state = reduceContentSelection(state, { type: "cancelLeaf" });
  expect(state.pendingLeaf).toBeNull();
  expect(state.selectedEventIds.sort()).toEqual(["e1", "e2"]);
  state = reduceContentSelection(state, { type: "requestLeaf", leaf: "leaf-b" });
  state = reduceContentSelection(state, {
    type: "confirmLeaf",
    leaf: "leaf-b",
    events: [row("e9")],
    nextCursor: null,
  });
  expect(state.leaf).toBe("leaf-b");
  expect(state.selectedEventIds).toEqual(["e9"]);
  expect(state.pendingLeaf).toBeNull();
});

test("selectionFromState is canonical v2 and field toggles change the hash input", () => {
  let state = reduceContentSelection(emptyContentSelection(), {
    type: "reset",
    previewId: "p1",
    leaf: "l",
    leaves: ["l"],
    events: [row("e1")],
    nextCursor: null,
  });
  state = reduceContentSelection(state, {
    type: "setField",
    field: "visibleReasoning",
    value: true,
  });
  state = reduceContentSelection(state, { type: "setSubagents", value: true });
  state = reduceContentSelection(state, { type: "toggleAsset", assetId: "a1" });
  const sel = selectionFromState(state);
  expect(sel.version).toBe(2);
  expect(sel.fields.visibleReasoning).toBe(true);
  expect(sel.includeSubagents).toBe(true);
  expect(sel.assetIds).toEqual(["a1"]);
});

test("an explicit empty selection stays empty instead of expanding to page defaults", () => {
  const explicitEmpty = defaultSelection();
  let state = reduceContentSelection(emptyContentSelection(), {
    type: "reset",
    previewId: "p1",
    leaf: "leaf-a",
    leaves: ["leaf-a"],
    events: [row("default-one"), row("default-two")],
    nextCursor: null,
    defaultSelection: explicitEmpty,
  });

  expect(state.selectedEventIds).toEqual([]);
  expect(state.modelCount).toBe(0);
  state = reduceContentSelection(state, {
    type: "appendPage",
    previewId: "p1",
    generation: state.generation,
    events: [row("later-default")],
    nextCursor: null,
  });
  expect(state.selectedEventIds).toEqual([]);
  expect(state.modelCount).toBe(0);
});

test("a user-cleared selection does not regain default grants on a later page", () => {
  let state = reduceContentSelection(emptyContentSelection(), {
    type: "reset",
    previewId: "p1",
    leaf: "leaf-a",
    leaves: ["leaf-a"],
    events: [row("first-default")],
    nextCursor: "c1",
  });
  state = reduceContentSelection(state, { type: "invertPage" });
  expect(state.selectedEventIds).toEqual([]);

  state = reduceContentSelection(state, {
    type: "appendPage",
    previewId: "p1",
    generation: state.generation,
    events: [row("later-default")],
    nextCursor: null,
  });
  expect(state.selectedEventIds).toEqual([]);
});
