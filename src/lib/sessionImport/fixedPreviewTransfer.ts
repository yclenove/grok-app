import type { sessionImportPreview, sessionImportPreviewEvents } from "@/lib/api/sessionMigration";
import type { SourceGroupMember } from "@/lib/api/sessionMigrationGroups";
import type { SourceComparisonSummary } from "@/lib/api/sessionMigrationComparison";
import { comparisonPreviewSummary, validateComparisonContext, validateComparisonGroupMember,
  type ComparisonPreviewContext } from "./comparisonPreviewState";
import { defaultSelection, validateSelectionSetV2Raw, type AllowedSelection } from "./selection";
import { applySelection, emptyPreviewSlot, type PreviewContentSlot } from "./contentStates";
import type { ImportGroupScope } from "./groupSelectionState";

export type FixedPreviewTransferRequest = {
  context: ComparisonPreviewContext;
  member: SourceGroupMember;
  preview: Awaited<ReturnType<typeof sessionImportPreview>>;
  comparison: SourceComparisonSummary;
};
export type FixedPreviewAuthority = (request: FixedPreviewTransferRequest, owns: () => boolean) => Promise<boolean>;
export type FixedPreviewTransfer = {
  run: (request: FixedPreviewTransferRequest, authorize: FixedPreviewAuthority) => Promise<boolean>;
  /** The just-committed content owner, not merely matching candidate/preview IDs. */
  isCurrent: (request: FixedPreviewTransferRequest) => boolean;
  getError: () => string | null;
};
export type FixedPreviewPage = Awaited<ReturnType<typeof sessionImportPreviewEvents>>;
export const FIXED_PREVIEW_PAGE_SIZE = 20;
export function fixedScopeKey(scope: ImportGroupScope | null): string {
  return scope ? JSON.stringify([scope.scanId, scope.epoch, scope.generation]) : "";
}
export function fixedTransferIdentity(request: FixedPreviewTransferRequest): string {
  return JSON.stringify([fixedScopeKey(request.context.scope), request.context.group.groupId,
    request.context.primary.candidateId, request.context.primary.sourceRevision,
    request.member.candidate.candidateId, request.member.candidate.sourceRevision,
    request.preview.previewId, request.comparison.leafId, request.comparison.snapshotDigest]);
}
function stale(detail: string): never { throw new Error(`PLAN_STALE: ${detail}`); }
function bounded(value: unknown, limit: number) {
  if (new TextEncoder().encode(JSON.stringify(value)).byteLength > limit) {
    throw new Error("RESOURCE_LIMIT: fixed preview transfer byte budget");
  }
}
function safeDefault(request: FixedPreviewTransferRequest): AllowedSelection {
  const { comparison, preview } = request;
  const summary = comparisonPreviewSummary(preview);
  const checked = validateSelectionSetV2Raw({ version: 2, entries: [{ previewId: preview.previewId,
    leafId: comparison.leafId, selection: summary.defaultSelection }] }).entries[0].selection;
  if (checked.eventIds.length > summary.eventCount || checked.pinnedEventIds.length ||
    checked.assetIds.length || checked.includeSubagents ||
    Object.entries(defaultSelection().fields).some(([key, value]) => checked.fields[key as keyof typeof checked.fields] !== value)) {
    stale("fixed preview defaults contain non-default grants");
  }
  return checked;
}

/** Capture before any await; authenticated native snapshots remain the authority. */
export function copyFixedPreviewTransfer(input: FixedPreviewTransferRequest): FixedPreviewTransferRequest {
  // The existing JSON-record budget, not the much smaller 20-row display budget.
  bounded(input, 32 * 1024 * 1024);
  const request = structuredClone(input);
  validateComparisonContext(request.context);
  validateComparisonGroupMember(request.context, request.member);
  const { member, preview, comparison, context } = request;
  const summary = comparisonPreviewSummary(preview);
  const title = (preview.adapter as { title?: unknown }).title;
  if (!comparison || comparison.previewId !== preview.previewId ||
    comparison.candidateId !== member.candidate.candidateId ||
    comparison.sourceRevision !== member.candidate.sourceRevision || comparison.groupId !== context.group.groupId ||
    !/^[a-f0-9]{64}$/.test(comparison.snapshotDigest) || !summary.leaves.includes(comparison.leafId) ||
    !Number.isSafeInteger(comparison.eventCount) || comparison.eventCount <= 0 || comparison.eventCount > summary.eventCount ||
    comparison.outcome !== summary.outcome || comparison.cwd !== summary.cwd ||
    !(title === null || typeof title === "string") || comparison.title !== title ||
    (member.candidate.candidateId === context.primary.candidateId && member.candidate.sourceRevision !== context.primary.sourceRevision)) {
    stale("fixed transfer does not match the compared member/branch");
  }
  safeDefault(request);
  return request;
}

export function fixedPreviewContentSlot(request: FixedPreviewTransferRequest, page: FixedPreviewPage, requestSeq: number): PreviewContentSlot {
  bounded(page, 256 * 1024);
  const summary = comparisonPreviewSummary(request.preview);
  const side = request.comparison;
  if (!page || !Array.isArray(page.events) || page.events.length !== Math.min(FIXED_PREVIEW_PAGE_SIZE, side.eventCount) ||
    (page.nextCursor !== null && (typeof page.nextCursor !== "string" || !page.nextCursor)) ||
    (page.nextCursor === null) !== (page.events.length === side.eventCount)) stale("invalid fixed preview first-page boundary");
  const ids = new Set<string>();
  for (const row of page.events) {
    if (!row || typeof row.eventId !== "string" || !row.eventId.trim() || ids.has(row.eventId) ||
      !["message", "tool-call", "tool-result", "compaction", "metadata", "unsupported"].includes(row.kind) ||
      typeof row.text !== "string" || typeof row.truncated !== "boolean" ||
      typeof row.selectable !== "boolean" || typeof row.defaultSelected !== "boolean" || typeof row.subagent !== "boolean" ||
      (row.defaultSelected && (!row.selectable || row.subagent)) ||
      ((row.kind === "metadata" || row.kind === "unsupported" || row.role === "system") && (row.selectable || row.defaultSelected)) ||
      (row.role !== undefined && row.role !== null && !["user", "assistant", "system"].includes(row.role)) ||
      (row.assetIds !== undefined && (!Array.isArray(row.assetIds) || row.assetIds.some((id) => typeof id !== "string" || !id.trim()))) ||
      (row.fieldAvailability !== undefined && (!row.fieldAvailability || typeof row.fieldAvailability !== "object" ||
        Array.isArray(row.fieldAvailability) ||
        Object.values(row.fieldAvailability).some((value) => typeof value !== "boolean")))) stale("invalid fixed preview event projection");
    ids.add(row.eventId);
  }
  const activeDefault = side.leafId === summary.leaf ? safeDefault(request) : null;
  if (activeDefault && (activeDefault.eventIds.length > side.eventCount ||
    page.events.some((row) => row.defaultSelected !== activeDefault.eventIds.includes(row.eventId)))) {
    stale("fixed preview page disagrees with active-branch defaults");
  }
  // Never borrow IDs or privileged grants from the adapter's other branch.
  const base = { ...emptyPreviewSlot(request.member.candidate.candidateId,
    request.member.candidate.sourceRevision, request.member.candidate.title, request.context.scope.generation),
    previewId: side.previewId, snapshotId: side.snapshotDigest, summary, requestSeq };
  return applySelection(base, { type: "reset", previewId: side.previewId, leaf: side.leafId,
    leaves: summary.leaves, events: structuredClone(page.events), nextCursor: page.nextCursor,
    defaultSelection: activeDefault ? { ...defaultSelection(), ...activeDefault } : null, archiveCount: summary.eventCount });
}
