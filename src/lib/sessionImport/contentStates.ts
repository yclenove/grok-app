/** Per-preview content slots. Owner + generation + requestSeq beat AbortController. */

import {
  emptyContentSelection,
  reduceContentSelection,
  selectionFromState,
  type ContentSelectionAction,
  type ContentSelectionState,
} from "./contentSelection";
import type { PeekSummary } from "./peekSummary";
import type { AllowedSelection } from "./selection";

export type BusyOperation = "idle" | "preview" | "branch" | "page";

export type PreviewContentSlot = {
  candidateId: string;
  previewId: string | null;
  snapshotId: string | null;
  sourceRevision: string | null;
  leafId: string | null;
  filterKey: string;
  title: string | null;
  summary: PeekSummary | null;
  selection: ContentSelectionState;
  generation: number;
  requestSeq: number;
  scanGeneration: number;
  dirty: boolean;
  busy: boolean;
  busyOperation: BusyOperation;
  error: string | null;
};

export type ContentRequestOwner = {
  candidateId: string;
  previewId: string | null;
  snapshotId: string | null;
  leafId: string | null;
  filterKey: string;
  generation: number;
  requestSeq: number;
  operation: BusyOperation;
};

export function filterKeyOf(leafId: string | null | undefined): string {
  return `leaf:${leafId ?? ""}`;
}

export function emptyPreviewSlot(
  candidateId: string,
  sourceRevision: string | null,
  title: string | null,
  scanGeneration = 0,
): PreviewContentSlot {
  return {
    candidateId,
    previewId: null,
    snapshotId: sourceRevision,
    sourceRevision,
    leafId: null,
    filterKey: filterKeyOf(null),
    title,
    summary: null,
    selection: emptyContentSelection(),
    generation: 1,
    requestSeq: 0,
    scanGeneration,
    dirty: false,
    busy: false,
    busyOperation: "idle",
    error: null,
  };
}

export function isCurrentRequest(
  slot: PreviewContentSlot | undefined,
  owner: ContentRequestOwner,
): boolean {
  if (!slot) return false;
  if (slot.candidateId !== owner.candidateId) return false;
  if (slot.generation !== owner.generation) return false;
  if (slot.requestSeq !== owner.requestSeq) return false;
  if (owner.previewId && slot.previewId && owner.previewId !== slot.previewId) return false;
  if (owner.snapshotId && slot.snapshotId && owner.snapshotId !== slot.snapshotId) return false;
  if (
    owner.operation !== "branch" &&
    owner.leafId &&
    slot.leafId &&
    owner.leafId !== slot.leafId
  ) {
    return false;
  }
  if (
    owner.operation !== "branch" &&
    owner.filterKey &&
    slot.filterKey &&
    owner.filterKey !== slot.filterKey
  ) {
    return false;
  }
  return true;
}

export function slotNeedsReload(
  slot: PreviewContentSlot | undefined,
  sourceRevision: string | null,
  scanGeneration?: number,
): boolean {
  if (!slot) return true;
  if (slot.sourceRevision !== sourceRevision) return true;
  if (scanGeneration != null && slot.scanGeneration !== scanGeneration) return true;
  return false;
}

export function sameOperationBusy(slot: PreviewContentSlot | undefined, operation: BusyOperation): boolean {
  if (!slot || operation === "idle") return false;
  return slot.busyOperation === operation;
}

function ownerFrom(slot: PreviewContentSlot, operation: BusyOperation): ContentRequestOwner {
  return {
    candidateId: slot.candidateId,
    previewId: slot.previewId,
    snapshotId: slot.snapshotId,
    leafId: slot.leafId,
    filterKey: slot.filterKey,
    generation: slot.generation,
    requestSeq: slot.requestSeq,
    operation,
  };
}

export function beginRequest(slot: PreviewContentSlot): {
  slot: PreviewContentSlot;
  owner: ContentRequestOwner;
} {
  return beginOperation(slot, "preview");
}

export function beginOperation(
  slot: PreviewContentSlot,
  operation: BusyOperation,
): { slot: PreviewContentSlot; owner: ContentRequestOwner } {
  const requestSeq = slot.requestSeq + 1;
  const next: PreviewContentSlot = {
    ...slot,
    requestSeq,
    busy: true,
    busyOperation: operation,
    error: operation === "preview" ? null : slot.error,
  };
  return { slot: next, owner: ownerFrom(next, operation) };
}

/** Bump generation and requestSeq from the live slot. Never reset via emptyPreviewSlot. */
export function beginRevisionReload(
  slot: PreviewContentSlot,
  sourceRevision: string | null,
  title: string | null,
): { slot: PreviewContentSlot; owner: ContentRequestOwner } {
  const generation = slot.generation + 1;
  const requestSeq = slot.requestSeq + 1;
  const next: PreviewContentSlot = {
    ...slot,
    sourceRevision,
    snapshotId: sourceRevision,
    title: title ?? slot.title,
    generation,
    requestSeq,
    busy: true,
    busyOperation: "preview",
    error: null,
  };
  return { slot: next, owner: ownerFrom(next, "preview") };
}

export function applySelection(
  slot: PreviewContentSlot,
  action: ContentSelectionAction,
): PreviewContentSlot {
  const selection = reduceContentSelection(slot.selection, action);
  const dirty =
    slot.dirty ||
    action.type === "toggleEvent" ||
    action.type === "selectPage" ||
    action.type === "invertPage" ||
    action.type === "setField" ||
    action.type === "setSubagents" ||
    action.type === "toggleAsset" ||
    action.type === "confirmLeaf";
  const leafId = selection.leaf ?? slot.leafId;
  return {
    ...slot,
    selection,
    dirty,
    leafId,
    filterKey: filterKeyOf(leafId),
  };
}

export function allowedSelectionOf(slot: PreviewContentSlot): AllowedSelection {
  return selectionFromState(slot.selection);
}

export function migratePendingToPreview(
  pending: PreviewContentSlot,
  previewId: string,
): PreviewContentSlot {
  return {
    ...pending,
    previewId,
  };
}
