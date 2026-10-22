import { act, renderHook } from "@testing-library/react";
import { vi } from "vitest";
import type { PrepareOverride, ScanState } from "@/lib/api/sessionMigration";
import { useSessionImportContentStates } from "@/hooks/useSessionImportContentStates";
import { useSessionImportSelection } from "@/hooks/useSessionImportSelection";
import { groupClient, groupScope } from "./sessionImportGroupFixture";
import { fixedTransferPage, fixedTransferRequest } from "./sessionImportFixedPreviewFixture";

const fixedAdoptionIpc = vi.hoisted(() => ({ start: vi.fn(), scanGet: vi.fn(), page: vi.fn(), cancel: vi.fn(),
  groupGet: vi.fn(), groups: vi.fn(), membership: vi.fn(), freeze: vi.fn(), set: vi.fn(), clear: vi.fn(),
  invert: vi.fn(), choose: vi.fn(), preview: vi.fn(), events: vi.fn(), candidateMembership: vi.fn() }));
vi.mock("@/lib/api/sessionMigration", () => ({ sessionImportScanStart: fixedAdoptionIpc.start,
  sessionImportScanGet: fixedAdoptionIpc.scanGet, sessionImportScanPage: fixedAdoptionIpc.page,
  sessionImportScanCancel: fixedAdoptionIpc.cancel, sessionImportPreview: fixedAdoptionIpc.preview,
  sessionImportPreviewEvents: fixedAdoptionIpc.events,
  sessionImportScanSelectionMembership: fixedAdoptionIpc.candidateMembership }));
vi.mock("@/lib/api/sessionMigrationGroups", () => ({ sessionImportGroupSelectionGet: fixedAdoptionIpc.groupGet,
  sessionImportScanGroups: fixedAdoptionIpc.groups, sessionImportGroupSelectionMembership: fixedAdoptionIpc.membership,
  sessionImportGroupSelectionFreeze: fixedAdoptionIpc.freeze, sessionImportGroupSelectionSet: fixedAdoptionIpc.set,
  sessionImportGroupSelectionClear: fixedAdoptionIpc.clear, sessionImportGroupSelectionInvert: fixedAdoptionIpc.invert,
  sessionImportGroupSelectionChoose: fixedAdoptionIpc.choose }));

export async function fixedAdoptionHarness(options: { isLocked?: () => boolean } = {}) {
  for (const mock of Object.values(fixedAdoptionIpc)) mock.mockReset();
  const host = groupClient(), request = fixedTransferRequest("alternate"), onError = vi.fn();
  const source: ScanState = { scanId: groupScope.scanId, epoch: 1, state: "completed", stateVersion: 1,
    candidates: [], discoveredCount: 3, complete: true, issueCount: 0, extraCount: 0,
    selection: { version: 1, selectionId: "candidate-selector", scanId: groupScope.scanId, epoch: 1,
      mode: "explicit", selectedCount: 0, stateVersion: 1 } };
  fixedAdoptionIpc.start.mockResolvedValue(source); fixedAdoptionIpc.scanGet.mockResolvedValue(source);
  fixedAdoptionIpc.cancel.mockResolvedValue(source);
  for (const [mock, action] of [[fixedAdoptionIpc.groupGet, host.client.get], [fixedAdoptionIpc.groups, host.client.groups],
    [fixedAdoptionIpc.membership, host.client.membership], [fixedAdoptionIpc.freeze, host.client.freeze],
    [fixedAdoptionIpc.set, host.client.set], [fixedAdoptionIpc.clear, host.client.clear],
    [fixedAdoptionIpc.invert, host.client.invert], [fixedAdoptionIpc.choose, host.client.choose]] as const) mock.mockImplementation(action);
  fixedAdoptionIpc.choose.mockImplementation(async (owner, groupId, candidateId) => {
    const before = host.choices.get(groupId)!;
    if (before.choiceVersion > 0 && before.candidate.candidateId === candidateId) {
      return { selection: await host.client.get(groupScope.scanId, groupScope.epoch), groupId,
        previousCandidateId: candidateId, candidateId, selected: before.selected };
    }
    const ack = await host.client.choose(owner, groupId, candidateId);
    host.choices.set(groupId, { ...host.choices.get(groupId)!, choiceVersion: ack.selection.stateVersion });
    return ack;
  });
  fixedAdoptionIpc.preview.mockRejectedValue(new Error("SOURCE_CHANGED: synthetic source offline"));
  fixedAdoptionIpc.events.mockResolvedValue(fixedTransferPage());
  const hook = renderHook(() => {
    const content = useSessionImportContentStates(onError);
    const selection = useSessionImportSelection(onError, { ...options, onCandidateInvalidated: content.invalidateCandidate });
    return { content, selection };
  });
  await act(async () => { await hook.result.current.selection.startScan(); });
  act(() => { hook.result.current.content.ensureSelected([], groupScope.generation, groupScope); });
  const transfer = (discardEdits = false) => ({
    isCurrent: (fixed: typeof request) => hook.result.current.content.fixedPreviewTransfer.isCurrent(fixed),
    getError: () => hook.result.current.content.fixedPreviewTransfer.getError(),
    run: (fixed: typeof request, authority: Parameters<typeof hook.result.current.content.adoptFixedPreview>[1]) =>
      hook.result.current.content.adoptFixedPreview(fixed, authority, { discardEdits }),
  });
  return { ...hook, host, request, onError, ipc: fixedAdoptionIpc, transfer };
}

export function fixedContentOverrides(h: Awaited<ReturnType<typeof fixedAdoptionHarness>>): PrepareOverride[] {
  return Object.values(h.result.current.content.byCandidate).filter((slot) => slot.previewId && slot.selection.leaf).map((slot) => ({
    candidateId: slot.candidateId, previewId: slot.previewId!, leafId: slot.selection.leaf!,
    selection: h.result.current.content.allowedSelectionOf(slot) }));
}
