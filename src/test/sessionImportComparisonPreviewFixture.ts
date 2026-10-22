import type { SourceGroupMember } from "@/lib/api/sessionMigrationGroups";
import type { ComparisonPreviewContext } from "@/lib/sessionImport/comparisonPreviewState";
import { groupCandidate, groupMetadata, groupScope } from "./sessionImportGroupFixture";

export function previewComparisonContext(): ComparisonPreviewContext {
  const group = groupMetadata("a");
  return { scope: { ...groupScope }, group, primary: { ...group.firstMember } };
}
export function previewComparisonMember(id = "b", context = previewComparisonContext()): SourceGroupMember {
  return { groupId: context.group.groupId, format: "cursor-transcript", sourceId: context.group.sourceId,
    rootPath: "/synthetic/other-root", legacyId: `cursor-transcript:${id}`, location: null,
    candidate: { ...groupCandidate(id), nativeSessionId: context.group.nativeSessionId } };
}
export function previewComparisonResponse(id = "a") {
  return { previewId: `fixed-preview-${id}`, adapter: {
    title: id, cwd: "/synthetic/project", eventCount: 2, activeLeaf: `leaf-${id}`,
    leaves: [`leaf-${id}`, `branch-${id}`], events: [], nextCursor: null,
    inventory: { imported: { messages: 2, toolCalls: 0, attachments: 0 }, missing: { attachments: 0 },
      unsupported: { messages: 0, toolCalls: 0, toolResults: 0 }, sourceTokens: 0 },
    defaultSelection: { version: 2, eventIds: [], pinnedEventIds: [], assetIds: [], includeSubagents: false },
    issues: [], outcome: "complete",
  } };
}
