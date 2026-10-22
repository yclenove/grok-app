import { act, renderHook } from "@testing-library/react";
import { vi } from "vitest";
import type { sessionImportPreview, sessionImportPreviewEvents } from "@/lib/api/sessionMigration";
import { useSessionImportContentStates } from "@/hooks/useSessionImportContentStates";
import { defaultSelection } from "@/lib/sessionImport/selection";
import type { FixedPreviewAuthority } from "@/lib/sessionImport/fixedPreviewTransfer";
import { groupCandidate } from "./sessionImportGroupFixture";
import { previewComparisonResponse } from "./sessionImportComparisonPreviewFixture";
import { fixedTransferPage, fixedTransferRequest } from "./sessionImportFixedPreviewFixture";

export function fixedContentHarness(id = "b") {
  const request = fixedTransferRequest(id), onError = vi.fn();
  const preview = vi.fn<typeof sessionImportPreview>().mockImplementation(async (candidateId) => {
    const response = previewComparisonResponse(candidateId.split(":").at(-1));
    response.previewId = `ordinary-${candidateId}`;
    Object.assign(response.adapter, { defaultSelection: { ...defaultSelection(), eventIds: ["old-one", "old-two"] } });
    return response;
  });
  const events = vi.fn<typeof sessionImportPreviewEvents>().mockResolvedValue(fixedTransferPage(["old-one", "old-two"], "old-cursor"));
  const hook = renderHook(() => useSessionImportContentStates(onError, { preview, events }));
  act(() => { hook.result.current.ensureSelected([], request.context.scope.generation, request.context.scope); });
  return { ...hook, preview, events, request, onError };
}

export async function openOrdinary(harness: ReturnType<typeof fixedContentHarness>, id: string) {
  const candidate = groupCandidate(id);
  await act(async () => { await harness.result.current.open({ candidateId: candidate.candidateId,
    sourceRevision: candidate.sourceRevision, title: candidate.title ?? id }); });
  return candidate.candidateId;
}

export function beginFixed(harness: ReturnType<typeof fixedContentHarness>, authorize: FixedPreviewAuthority,
  options?: { discardEdits?: boolean }) {
  let promise!: Promise<boolean>;
  act(() => { promise = harness.result.current.adoptFixedPreview(harness.request, authorize, options); });
  return promise;
}
