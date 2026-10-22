import type { SourceCandidate } from "@/lib/api/sessionMigration";
import type { SourceGroup, SourceGroupMember } from "@/lib/api/sessionMigrationGroups";
import type { SourceComparisonSide } from "@/lib/api/sessionMigrationComparison";
import { emptyImportGroupMembers, validateGroupMemberPage } from "./groupMemberState";
import { groupStateError, validateGroupCandidate, validateGroupPage, validateGroupScope, type ImportGroupScope } from "./groupSelectionState";
import { peekSummaryFromAdapter, type PeekSummary } from "./peekSummary";

export type ComparisonPreviewContext = { scope: ImportGroupScope; group: SourceGroup; primary: SourceCandidate };
export type ComparisonPreviewSlot = {
  candidate: SourceCandidate;
  member: SourceGroupMember | null;
  previewId: string | null;
  summary: PeekSummary | null;
  adapter: unknown;
  leafId: string | null;
  loading: boolean;
  error: string | null;
};
export type ComparisonPreviewState = {
  open: boolean;
  context: ComparisonPreviewContext | null;
  left: ComparisonPreviewSlot | null;
  right: ComparisonPreviewSlot | null;
  error: string | null;
};
export type ComparisonSideName = "left" | "right";
export function emptyComparisonPreviews(): ComparisonPreviewState {
  return { open: false, context: null, left: null, right: null, error: null };
}
export function comparisonContextKey(context: ComparisonPreviewContext | null): string {
  if (!context) return "";
  const { scope, group, primary } = context;
  return JSON.stringify([scope.scanId, scope.epoch, scope.generation, group.groupId,
    group.sourceId, group.provider, group.host, group.nativeSessionId, group.proof,
    primary.candidateId, primary.sourceRevision, primary.provider, primary.nativeSessionId]);
}
export function copyComparisonContext(context: ComparisonPreviewContext): ComparisonPreviewContext {
  return { scope: { ...context.scope }, primary: { ...context.primary },
    group: { ...context.group, firstMember: { ...context.group.firstMember } } };
}
export function validateComparisonContext(context: ComparisonPreviewContext) {
  validateGroupScope(context.scope);
  validateGroupPage(context.scope, { version: 1, groups: [context.group], nextCursor: null,
    totalCount: 1, complete: true, stateVersion: 0 });
  validateGroupCandidate(context.scope, context.primary);
  if (context.primary.provider !== context.group.provider ||
    (context.group.proof === "typed-native" && context.primary.nativeSessionId !== context.group.nativeSessionId)) {
    groupStateError("foreign primary comparison candidate");
  }
}
export function validateComparisonGroupMember(context: ComparisonPreviewContext, member: SourceGroupMember) {
  if (!member || new TextEncoder().encode(JSON.stringify(member)).byteLength > 256 * 1024) {
    groupStateError("invalid or oversized source comparison member metadata");
  }
  validateGroupMemberPage(context.scope, context.group, { version: 1, groupId: context.group.groupId,
    members: [member], nextCursor: null, memberCount: context.group.memberCount, complete: true, stateVersion: 0 },
  emptyImportGroupMembers());
}
export function validateComparisonMember(context: ComparisonPreviewContext, member: SourceGroupMember) {
  validateComparisonGroupMember(context, member);
  if (member.candidate.candidateId === context.primary.candidateId) groupStateError("comparison requires a different member");
}

/** Validate the actual bounded PreviewDetails wire, not the permissive legacy display parser. */
export function comparisonPreviewSummary(response: { previewId: string; adapter: unknown }): PeekSummary {
  if (!response || typeof response.previewId !== "string" || !response.previewId ||
    !response.adapter || typeof response.adapter !== "object") groupStateError("invalid fixed comparison preview");
  const adapter = response.adapter as Record<string, unknown>;
  if (!Number.isSafeInteger(adapter.eventCount) || (adapter.eventCount as number) < 0 ||
    !["complete", "partial", "unsupported", "failed"].includes(String(adapter.outcome)) ||
    !(adapter.activeLeaf === null || typeof adapter.activeLeaf === "string") ||
    !Array.isArray(adapter.leaves) || adapter.leaves.length > (adapter.eventCount as number) ||
    adapter.leaves.some((leaf) => typeof leaf !== "string" || !leaf) ||
    new Set(adapter.leaves).size !== adapter.leaves.length || !Array.isArray(adapter.events) ||
    adapter.events.length > 100 || adapter.events.length > (adapter.eventCount as number) ||
    !Array.isArray(adapter.issues)) groupStateError("invalid fixed comparison preview metadata");
  if ((adapter.eventCount as number) > 0 && !adapter.activeLeaf) groupStateError("comparison preview has no active branch");
  const summary = peekSummaryFromAdapter(adapter);
  return { ...summary, leaf: summary.leaf || null,
    // A rollback's active event can be an ancestor rather than a terminal leaf.
    leaves: [...new Set([...(summary.leaf ? [summary.leaf] : []), ...summary.leaves])] };
}

export function comparisonPairOf(state: ComparisonPreviewState): { left: SourceComparisonSide; right: SourceComparisonSide } | null {
  const side = (slot: ComparisonPreviewSlot | null): SourceComparisonSide | null => slot &&
    !slot.loading && !slot.error && slot.previewId && slot.leafId ? {
      previewId: slot.previewId, candidateId: slot.candidate.candidateId,
      sourceRevision: slot.candidate.sourceRevision, leafId: slot.leafId,
    } : null;
  if (!state.open) return null;
  const left = side(state.left);
  const right = side(state.right);
  return left && right ? { left, right } : null;
}
