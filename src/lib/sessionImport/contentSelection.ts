/** Per-preview content selection reducer. UI never shares a bare event-id set. */

import {
  canonicalizeSelection,
  defaultSelection,
  type AllowedSelection,
  type SelectionFields,
} from "./selection";

export type PreviewEventRow = {
  eventId: string;
  kind: string;
  role?: string | null;
  text: string;
  truncated?: boolean;
  selectable?: boolean;
  defaultSelected?: boolean;
  subagent?: boolean;
  fieldAvailability?: Record<string, boolean>;
  assetIds?: string[];
  displayParts?: unknown[];
  displayLimited?: boolean;
  citation?: string;
};

export type ContentSelectionState = {
  previewId: string | null;
  leaf: string | null;
  leaves: string[];
  events: PreviewEventRow[];
  nextCursor: string | null;
  selectedEventIds: string[];
  pinnedEventIds: string[];
  assetIds: string[];
  fields: SelectionFields;
  includeSubagents: boolean;
  pendingLeaf: string | null;
  generation: number;
  archiveCount: number;
  modelCount: number;
  autoSelectPageDefaults: boolean;
};

export type ContentSelectionAction =
  | {
      type: "reset";
      previewId: string;
      leaf: string | null;
      leaves: string[];
      events: PreviewEventRow[];
      nextCursor: string | null;
      defaultSelection?: AllowedSelection | null;
      archiveCount?: number;
    }
  | {
      type: "appendPage";
      previewId: string;
      generation: number;
      events: PreviewEventRow[];
      nextCursor: string | null;
    }
  | { type: "toggleEvent"; eventId: string }
  | { type: "selectPage" }
  | { type: "invertPage" }
  | { type: "setField"; field: keyof SelectionFields; value: boolean }
  | { type: "setSubagents"; value: boolean }
  | { type: "toggleAsset"; assetId: string }
  | { type: "requestLeaf"; leaf: string }
  | { type: "cancelLeaf" }
  | {
      type: "confirmLeaf";
      leaf: string;
      events: PreviewEventRow[];
      nextCursor: string | null;
      defaultSelection?: AllowedSelection | null;
    };

const emptyFields = (): SelectionFields => defaultSelection().fields;

export function emptyContentSelection(): ContentSelectionState {
  return {
    previewId: null,
    leaf: null,
    leaves: [],
    events: [],
    nextCursor: null,
    selectedEventIds: [],
    pinnedEventIds: [],
    assetIds: [],
    fields: emptyFields(),
    includeSubagents: false,
    pendingLeaf: null,
    generation: 0,
    archiveCount: 0,
    modelCount: 0,
    autoSelectPageDefaults: true,
  };
}

function unique(ids: string[]): string[] {
  return [...new Set(ids.filter((id) => id.trim() !== ""))];
}

function recount(state: ContentSelectionState): ContentSelectionState {
  const archiveCount = state.archiveCount || state.events.length;
  const modelCount = state.selectedEventIds.length;
  return { ...state, archiveCount, modelCount };
}

function applyDefault(
  events: PreviewEventRow[],
  fallback: AllowedSelection | null | undefined,
): Pick<
  ContentSelectionState,
  | "selectedEventIds"
  | "pinnedEventIds"
  | "assetIds"
  | "fields"
  | "includeSubagents"
  | "autoSelectPageDefaults"
> {
  if (fallback) {
    return {
      selectedEventIds: unique(fallback.eventIds),
      pinnedEventIds: unique(fallback.pinnedEventIds),
      assetIds: unique(fallback.assetIds),
      fields: { ...fallback.fields },
      includeSubagents: fallback.includeSubagents,
      autoSelectPageDefaults: false,
    };
  }
  return {
    selectedEventIds: unique(
      events.filter((e) => e.defaultSelected && e.selectable !== false).map((e) => e.eventId),
    ),
    pinnedEventIds: [],
    assetIds: [],
    fields: emptyFields(),
    includeSubagents: false,
    autoSelectPageDefaults: true,
  };
}

export function reduceContentSelection(
  state: ContentSelectionState,
  action: ContentSelectionAction,
): ContentSelectionState {
  switch (action.type) {
    case "reset": {
      const applied = applyDefault(action.events, action.defaultSelection);
      return recount({
        ...emptyContentSelection(),
        previewId: action.previewId,
        leaf: action.leaf,
        leaves: action.leaves,
        events: action.events,
        nextCursor: action.nextCursor,
        generation: state.generation + 1,
        archiveCount: action.archiveCount ?? action.events.length,
        ...applied,
      });
    }
    case "appendPage": {
      if (action.previewId !== state.previewId || action.generation !== state.generation) {
        return state;
      }
      const seen = new Set(state.events.map((e) => e.eventId));
      const extra = action.events.filter((e) => !seen.has(e.eventId));
      const extraSelected = state.autoSelectPageDefaults
        ? extra
            .filter((e) => e.defaultSelected && e.selectable !== false)
            .map((e) => e.eventId)
        : [];
      return recount({
        ...state,
        events: [...state.events, ...extra],
        nextCursor: action.nextCursor,
        selectedEventIds: unique([...state.selectedEventIds, ...extraSelected]),
      });
    }
    case "toggleEvent": {
      const row = state.events.find((e) => e.eventId === action.eventId);
      if (row && row.selectable === false) return state;
      const has = state.selectedEventIds.includes(action.eventId);
      const selectedEventIds = has
        ? state.selectedEventIds.filter((id) => id !== action.eventId)
        : [...state.selectedEventIds, action.eventId];
      const pinnedEventIds = state.pinnedEventIds.filter((id) => selectedEventIds.includes(id));
      return recount({
        ...state,
        selectedEventIds,
        pinnedEventIds,
        autoSelectPageDefaults: false,
      });
    }
    case "selectPage": {
      const pageIds = state.events
        .filter((e) => e.selectable !== false)
        .map((e) => e.eventId);
      return recount({
        ...state,
        selectedEventIds: unique([...state.selectedEventIds, ...pageIds]),
        autoSelectPageDefaults: false,
      });
    }
    case "invertPage": {
      const page = new Set(
        state.events.filter((e) => e.selectable !== false).map((e) => e.eventId),
      );
      const keep = state.selectedEventIds.filter((id) => !page.has(id));
      const add = [...page].filter((id) => !state.selectedEventIds.includes(id));
      const selectedEventIds = unique([...keep, ...add]);
      const pinnedEventIds = state.pinnedEventIds.filter((id) => selectedEventIds.includes(id));
      return recount({
        ...state,
        selectedEventIds,
        pinnedEventIds,
        autoSelectPageDefaults: false,
      });
    }
    case "setField":
      return { ...state, fields: { ...state.fields, [action.field]: action.value } };
    case "setSubagents":
      return { ...state, includeSubagents: action.value };
    case "toggleAsset": {
      const has = state.assetIds.includes(action.assetId);
      return {
        ...state,
        assetIds: has
          ? state.assetIds.filter((id) => id !== action.assetId)
          : [...state.assetIds, action.assetId],
      };
    }
    case "requestLeaf":
      if (!action.leaf || action.leaf === state.leaf) return { ...state, pendingLeaf: null };
      return { ...state, pendingLeaf: action.leaf };
    case "cancelLeaf":
      return { ...state, pendingLeaf: null };
    case "confirmLeaf": {
      const applied = applyDefault(action.events, action.defaultSelection);
      return recount({
        ...state,
        leaf: action.leaf,
        events: action.events,
        nextCursor: action.nextCursor,
        pendingLeaf: null,
        generation: state.generation + 1,
        archiveCount: action.events.length,
        ...applied,
      });
    }
    default:
      return state;
  }
}

export function selectionFromState(state: ContentSelectionState): AllowedSelection {
  return canonicalizeSelection({
    ...defaultSelection(),
    eventIds: state.selectedEventIds,
    pinnedEventIds: state.pinnedEventIds,
    assetIds: state.assetIds,
    fields: state.fields,
    includeSubagents: state.includeSubagents,
  });
}
