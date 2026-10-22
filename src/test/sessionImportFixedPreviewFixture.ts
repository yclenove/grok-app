import { previewComparisonContext, previewComparisonMember, previewComparisonResponse } from "./sessionImportComparisonPreviewFixture";
import { defaultSelection } from "@/lib/sessionImport/selection";
import type { FixedPreviewPage, FixedPreviewTransferRequest } from "@/lib/sessionImport/fixedPreviewTransfer";

export function fixedTransferPage(ids = ["fixed-one", "fixed-two"], nextCursor: string | null = null): FixedPreviewPage {
  return { events: ids.map((eventId) => ({ eventId, kind: "message", role: "user",
    text: eventId, truncated: false, selectable: true, defaultSelected: true, subagent: false,
    fieldAvailability: { messageText: true, visibleReasoning: false, toolCallName: false,
      toolCallInput: false, toolResultContent: false, compactionSummary: false, assets: false },
    assetIds: [], citation: eventId })), nextCursor };
}

export function fixedTransferRequest(id = "b"): FixedPreviewTransferRequest {
  const context = previewComparisonContext(), member = previewComparisonMember(id), preview = previewComparisonResponse(id);
  Object.assign(preview.adapter, { defaultSelection: { ...defaultSelection(), eventIds: ["fixed-one", "fixed-two"] } });
  return { context, member, preview, comparison: { previewId: preview.previewId,
    candidateId: member.candidate.candidateId, sourceRevision: member.candidate.sourceRevision,
    leafId: `leaf-${id}`, snapshotDigest: "b".repeat(64), groupId: context.group.groupId,
    eventCount: 2, title: id, cwd: "/synthetic/project", outcome: "complete" } };
}
