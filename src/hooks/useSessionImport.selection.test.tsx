/** @vitest-environment jsdom */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { PrepareBatchStatus, ScanState } from "@/lib/api/sessionMigration";
import { groupCandidate, groupChoice, groupClient, groupDeferred, groupMetadata, groupScope } from "@/test/sessionImportGroupFixture";
import { defaultSelection } from "@/lib/sessionImport/selection";
import { openSessionImport } from "@/lib/sessionImport/open";
import { useSessionImportUi } from "./useSessionImport";

const ipc = vi.hoisted(() => ({
  start: vi.fn(), get: vi.fn(), page: vi.fn(), cancel: vi.fn(), membership: vi.fn(),
  freeze: vi.fn(), set: vi.fn(), clear: vi.fn(), invert: vi.fn(),
  prepare: vi.fn(), items: vi.fn(), prepareCancel: vi.fn(),
  groupGet: vi.fn(), groups: vi.fn(), groupMembership: vi.fn(), groupFreeze: vi.fn(),
  groupSet: vi.fn(), groupClear: vi.fn(), groupInvert: vi.fn(), groupChoose: vi.fn(),
  files: vi.fn(), folder: vi.fn(), workspace: vi.fn(),
}));
vi.mock("@/lib/api", () => ({
  sessionImportRecovery: vi.fn().mockResolvedValue({ expired: false }),
  sessionImportEntries: vi.fn().mockResolvedValue({ items: [] }),
  sessionImportListRoots: vi.fn().mockResolvedValue([]), sessionImportSources: vi.fn().mockResolvedValue([]),
  projectsList: vi.fn().mockResolvedValue([]),
  pickAttachFiles: ipc.files, pickDirectory: ipc.folder,
}));
vi.mock("@/lib/api/sessionMigration", () => ({
  sessionImportScanStart: ipc.start, sessionImportScanGet: ipc.get, sessionImportScanPage: ipc.page,
  sessionImportScanCancel: ipc.cancel, sessionImportScanSelectionMembership: ipc.membership,
  sessionImportScanSelectionFreeze: ipc.freeze, sessionImportScanSelectionSet: ipc.set,
  sessionImportScanSelectionClear: ipc.clear, sessionImportScanSelectionInvert: ipc.invert,
  sessionImportPrepareStart: ipc.prepare, sessionImportPrepareItems: ipc.items, sessionImportPrepareCancel: ipc.prepareCancel,
  sessionImportWorkspacePreview: ipc.workspace,
}));
vi.mock("@/lib/api/sessionMigrationGroups", () => ({
  sessionImportGroupSelectionGet: ipc.groupGet, sessionImportScanGroups: ipc.groups,
  sessionImportGroupSelectionMembership: ipc.groupMembership, sessionImportGroupSelectionFreeze: ipc.groupFreeze,
  sessionImportGroupSelectionSet: ipc.groupSet, sessionImportGroupSelectionClear: ipc.groupClear,
  sessionImportGroupSelectionInvert: ipc.groupInvert, sessionImportGroupSelectionChoose: ipc.groupChoose,
}));

const a = groupMetadata("a").groupId;
const b = groupMetadata("b").groupId;
let source: ScanState;
let host: ReturnType<typeof groupClient>;
function slot(id: string) {
  return { candidateId: groupCandidate(id).candidateId, previewId: `preview-${id}`, leafId: "leaf",
    selection: defaultSelection() };
}
const batch = (version = 2): PrepareBatchStatus => ({
  version: 1, batchId: "synthetic-batch", scanId: groupScope.scanId, epoch: 1,
  selectionId: "synthetic-selector", selectionStateVersion: version,
  state: "completed", stateVersion: 1, total: 2, prepared: 2, failed: 0, skipped: 0,
  active: 0, cancelRequested: false, createdAt: "2026-10-12T00:00:00Z", updatedAt: "2026-10-12T00:00:00Z",
});
beforeEach(() => {
  vi.clearAllMocks();
  for (const mock of Object.values(ipc)) mock.mockReset();
  source = { scanId: groupScope.scanId, epoch: 1, state: "completed", stateVersion: 1,
    candidates: [], discoveredCount: 3, complete: true, issueCount: 0, extraCount: 0,
    selection: { version: 1, selectionId: "candidate-selector", scanId: groupScope.scanId,
      epoch: 1, mode: "explicit", selectedCount: 0, stateVersion: 1 } };
  ipc.start.mockResolvedValue(source); ipc.get.mockImplementation(async () => source);
  ipc.page.mockResolvedValue({ candidates: [groupCandidate("a"), groupCandidate("alternate"), groupCandidate("b")],
    nextCursor: null, complete: true, discoveredCount: 3 });
  ipc.membership.mockImplementation(async (_scan, _epoch, selector, ids: string[]) => ({
    selectionId: selector, stateVersion: source.selection.stateVersion,
    selectedIds: source.selection.selectedCount ? ids : [],
  }));
  ipc.freeze.mockImplementation(async () => {
    source = { ...source, selection: { ...source.selection, stateVersion: 2, selectedCount: 3 } };
    return source.selection;
  });
  ipc.cancel.mockResolvedValue(source);
  ipc.prepare.mockResolvedValue(batch()); ipc.items.mockResolvedValue({ items: [], nextCursor: null });
  ipc.prepareCancel.mockResolvedValue({ ...batch(), state: "cancelled" });
  host = groupClient();
  for (const [mock, action] of [[ipc.groupGet, host.client.get], [ipc.groups, host.client.groups],
    [ipc.groupMembership, host.client.membership], [ipc.groupFreeze, host.client.freeze],
    [ipc.groupSet, host.client.set], [ipc.groupClear, host.client.clear],
    [ipc.groupInvert, host.client.invert], [ipc.groupChoose, host.client.choose]] as const) {
    mock.mockImplementation(action);
  }
});
afterEach(cleanup);
async function start(result: { current: ReturnType<typeof useSessionImportUi> }) {
  await act(async () => { openSessionImport({}); });
  await act(async () => { await result.current.scan(); });
}

test("new scans show one stable row per group without adopting its selector into legacy scan state", async () => {
  const { result } = renderHook(() => useSessionImportUi());
  await start(result);
  expect(result.current.selectionView?.domain).toBe("fixed-groups");
  expect(result.current.selectionRows.map((row) => row.rowId)).toEqual([a, b]);
  expect(result.current.candidates.map((candidate) => candidate.candidateId)).toEqual([groupCandidate("a").candidateId, groupCandidate("b").candidateId]);
  expect(result.current.scanState?.selection.selectionId).toBe("candidate-selector");
  expect(ipc.page).not.toHaveBeenCalled();
  expect(ipc.membership).not.toHaveBeenCalled();
});

test("a new source intent cannot reopen or prepare the previous source selection", async () => {
  const { result } = renderHook(() => useSessionImportUi());
  await start(result);
  await act(async () => { await result.current.selectThese([a]); });
  ipc.groupGet.mockClear();
  act(() => { result.current.chooseListedRoot("codex", "/synthetic/other-source"); result.current.setOpen(false); });
  await act(async () => { result.current.setOpen(true); });
  await act(async () => { await result.current.previewSelected([]); });
  expect(ipc.groupGet).not.toHaveBeenCalled();
  expect(ipc.prepare).not.toHaveBeenCalled();
  expect(result.current.scanState).toBeNull();
  expect(result.current.selectionReady).toBe(false);
});

test.each(["file", "folder"] as const)("a selected workspace %s expires the previous transcript selection even when preview fails", async (kind) => {
  const { result } = renderHook(() => useSessionImportUi());
  await start(result);
  await act(async () => { await result.current.selectThese([a]); });
  ipc.groupGet.mockClear();
  ipc.files.mockResolvedValue(["/synthetic/project.code-workspace"]);
  ipc.folder.mockResolvedValue("/synthetic/workspace");
  ipc.workspace.mockRejectedValue(new Error("synthetic workspace preview failure"));
  await act(async () => { await (kind === "file" ? result.current.pickCustomFile() : result.current.pickWorkspaceFolder()); });
  act(() => { result.current.setOpen(false); });
  await act(async () => { result.current.setOpen(true); });
  await act(async () => { await result.current.previewSelected([]); });
  expect(ipc.groupGet).not.toHaveBeenCalled();
  expect(ipc.prepare).not.toHaveBeenCalled();
  expect(result.current.scanState).toBeNull();
});

test("a same-turn refresh reserves choice reads before another preparation can start", async () => {
  const { result } = renderHook(() => useSessionImportUi());
  await start(result);
  await act(async () => { await result.current.selectThese([a]); });
  const pending = groupDeferred<Awaited<ReturnType<typeof host.client.membership>>>();
  ipc.groupMembership.mockReturnValueOnce(pending.promise);
  let refreshing!: Promise<boolean>;
  let preparing!: Promise<void>;
  act(() => { refreshing = result.current.refreshScan(); preparing = result.current.previewSelected([]); });
  await act(async () => { await preparing; });
  expect(ipc.prepare).not.toHaveBeenCalled();
  await act(async () => { pending.resolve(await host.client.membership(groupScope.scanId, 1, "synthetic-selector", [a, b])); await refreshing; });
});

test.each(["before-start", "after-start"] as const)("closing %s during discovery startup cannot recreate group ownership", async (phase) => {
  const pending = groupDeferred<ScanState>();
  ipc.start.mockReturnValueOnce(pending.promise);
  const { result } = renderHook(() => useSessionImportUi());
  await act(async () => { openSessionImport({}); });
  let scanning!: Promise<void>;
  act(() => { scanning = result.current.scan(); });
  if (phase === "after-start") {
    await act(async () => {});
    expect(ipc.start).toHaveBeenCalledTimes(1);
  }
  await act(async () => { result.current.setOpen(false); });
  await act(async () => { pending.resolve(source); await scanning; });
  expect(ipc.groupGet).not.toHaveBeenCalled();
  expect(result.current.selectionReady).toBe(false);
  expect(result.current.step).toBe("source");
});

test("group bulk and single-row actions forward group IDs and the accepted group CAS version", async () => {
  const { result } = renderHook(() => useSessionImportUi());
  await start(result);
  await act(async () => { await result.current.selectThese([a]); });
  expect(ipc.groupSet).toHaveBeenLastCalledWith({ scanId: groupScope.scanId, epoch: 1,
    selectionId: "synthetic-selector", expectedStateVersion: 1 }, [a], true);
  expect(result.current.selectedCount).toBe(1);
  await act(async () => { await result.current.toggle(a); });
  expect(result.current.selectedCount).toBe(0);
  expect(result.current.selectionRows[0].rowId).toBe(a);
  expect(ipc.set).not.toHaveBeenCalled();
});

test("prepare uses the active group selector and drops retained overrides for deselected members", async () => {
  const { result } = renderHook(() => useSessionImportUi());
  await start(result);
  act(() => { result.current.rememberPreview(groupCandidate("a").candidateId); result.current.rememberPreview(groupCandidate("b").candidateId); });
  await act(async () => { await result.current.selectThese([a]); });
  await act(async () => { await result.current.previewSelected([slot("a"), slot("b")]); });
  expect(ipc.prepare).toHaveBeenCalledExactlyOnceWith(groupScope.scanId, 1, "synthetic-selector", [slot("a")]);
  expect(result.current.step).toBe("prepare");
});

test("an off-page selected preview is checked without loading the entire chosen inventory", async () => {
  const { result } = renderHook(() => useSessionImportUi());
  await start(result);
  act(() => { result.current.rememberPreview(groupCandidate("a").candidateId); });
  await act(async () => { await result.current.selectCurrent(); });
  ipc.groups.mockResolvedValueOnce({ version: 1, groups: [groupMetadata("c")], nextCursor: null,
    totalCount: 3, complete: true, stateVersion: 2 });
  await act(async () => { await result.current.refreshScan(); });
  ipc.groupMembership.mockClear();
  await act(async () => { await result.current.previewSelected([slot("a")]); });
  expect(ipc.groupMembership).toHaveBeenCalledExactlyOnceWith(groupScope.scanId, 1, "synthetic-selector", [a]);
  expect(ipc.prepare).toHaveBeenCalledExactlyOnceWith(groupScope.scanId, 1, "synthetic-selector", [slot("a")]);
});

test("choosing another member and clearing preferences invalidate only obsolete preview ownership", async () => {
  const invalidated = vi.fn();
  const { result } = renderHook(() => useSessionImportUi(undefined, { onCandidateInvalidated: invalidated }));
  await start(result);
  act(() => { result.current.rememberPreview(groupCandidate("a").candidateId); result.current.rememberPreview(groupCandidate("b").candidateId); });
  await act(async () => { await result.current.chooseGroupMember(a, groupCandidate("alternate").candidateId); });
  expect(invalidated).toHaveBeenCalledExactlyOnceWith(groupCandidate("a").candidateId);
  expect(result.current.selectionRows[0].rowId).toBe(a);
  expect(result.current.selectionRows[0].candidate.candidateId).toBe(groupCandidate("alternate").candidateId);
  act(() => { result.current.rememberPreview(groupCandidate("alternate").candidateId); });
  await act(async () => { await result.current.clearSelected(); });
  expect(invalidated.mock.calls.map(([id]) => id)).toEqual([groupCandidate("a").candidateId, groupCandidate("alternate").candidateId]);
  expect(result.current.selectionRows[0].candidate.candidateId).toBe(groupCandidate("a").candidateId);
});

test("group initialization failure stays visible and cannot implicitly prepare with candidate selection", async () => {
  ipc.groupGet.mockRejectedValueOnce(new Error("PLAN_STALE: damaged selector"));
  const { result } = renderHook(() => useSessionImportUi());
  await start(result);
  expect(result.current.error).toContain("PLAN_STALE");
  await act(async () => { await result.current.previewSelected([]); });
  expect(ipc.prepare).not.toHaveBeenCalled();
  expect(ipc.freeze).not.toHaveBeenCalled();
  expect(result.current.selectionView?.domain).toBe("fixed-groups");
});

test("only an explicit independent-candidate choice uses the legacy selector", async () => {
  const { result } = renderHook(() => useSessionImportUi());
  await start(result);
  await act(async () => { await result.current.useIndependentCandidates(); });
  await act(async () => { await result.current.selectCurrent(); });
  ipc.prepare.mockResolvedValueOnce({ ...batch(), selectionId: "candidate-selector", total: 3, prepared: 3 });
  await act(async () => { await result.current.previewSelected([]); });
  expect(result.current.selectionView?.domain).toBe("independent-candidates");
  expect(ipc.prepare).toHaveBeenCalledExactlyOnceWith(groupScope.scanId, 1, "candidate-selector", []);
  expect(ipc.groupFreeze).not.toHaveBeenCalled();
});

test("both explicit domain changes release only actually opened preview owners", async () => {
  const invalidated = vi.fn();
  const { result } = renderHook(() => useSessionImportUi(undefined, { onCandidateInvalidated: invalidated }));
  await start(result);
  act(() => { result.current.rememberPreview(groupCandidate("a").candidateId); });
  await act(async () => { await result.current.useIndependentCandidates(); });
  expect(invalidated.mock.calls.map(([id]) => id)).toEqual([groupCandidate("a").candidateId]);
  act(() => {
    result.current.rememberPreview(groupCandidate("alternate").candidateId);
    result.current.rememberPreview(groupCandidate("b").candidateId);
  });
  await act(async () => { await result.current.useFixedGroups(); });
  expect(invalidated.mock.calls.map(([id]) => id)).toEqual([
    groupCandidate("a").candidateId, groupCandidate("alternate").candidateId, groupCandidate("b").candidateId,
  ]);
  act(() => { result.current.setOpen(false); });
  expect(invalidated).toHaveBeenCalledTimes(3);
  expect(ipc.prepare).not.toHaveBeenCalled();
  expect(ipc.groupChoose).not.toHaveBeenCalled();
});

test("independent preview ownership is bounded, allows repeated IDs and resets on close", async () => {
  const invalidated = vi.fn();
  const { result } = renderHook(() => useSessionImportUi(undefined, { onCandidateInvalidated: invalidated }));
  await start(result);
  await act(async () => { await result.current.useIndependentCandidates(); });
  act(() => {
    for (let index = 0; index < 200; index++) expect(result.current.rememberPreview(`synthetic-preview-${index}`)).toBe(true);
    expect(result.current.rememberPreview("synthetic-preview-0")).toBe(true);
    expect(result.current.rememberPreview("synthetic-preview-200")).toBe(false);
  });
  expect(result.current.error).toContain("RESOURCE_LIMIT");
  act(() => { result.current.setOpen(false); });
  expect(invalidated).toHaveBeenCalledTimes(200);
  expect(invalidated).not.toHaveBeenCalledWith("synthetic-preview-200");
});

test("closing during override membership verification drops success/error and cannot create a prepare batch", async () => {
  const { result } = renderHook(() => useSessionImportUi());
  await start(result);
  act(() => { result.current.rememberPreview(groupCandidate("a").candidateId); });
  await act(async () => { await result.current.selectCurrent(); });
  const pending = groupDeferred<Awaited<ReturnType<typeof host.client.membership>>>();
  ipc.groupMembership.mockReturnValueOnce(pending.promise);
  let preparing!: Promise<void>;
  act(() => { preparing = result.current.previewSelected([slot("a")]); result.current.setOpen(false); });
  await act(async () => { pending.resolve({ selectionId: "synthetic-selector", stateVersion: 2, choices: [groupChoice("a", true)] }); await preparing; });
  expect(ipc.prepare).not.toHaveBeenCalled();
  expect(result.current.step).toBe("content");
});

test("same-turn preparation reserves selection input and blocks member edits and duplicate prepare", async () => {
  const { result } = renderHook(() => useSessionImportUi());
  await start(result);
  act(() => { result.current.rememberPreview(groupCandidate("a").candidateId); });
  await act(async () => { await result.current.selectCurrent(); });
  const pending = groupDeferred<Awaited<ReturnType<typeof host.client.membership>>>();
  ipc.groupMembership.mockReturnValueOnce(pending.promise);
  let first!: Promise<void>;
  let duplicate!: Promise<void>;
  act(() => { first = result.current.previewSelected([slot("a")]); duplicate = result.current.previewSelected([slot("a")]);
    void result.current.chooseGroupMember(a, groupCandidate("alternate").candidateId); });
  await act(async () => { pending.resolve({ selectionId: "synthetic-selector", stateVersion: 2, choices: [groupChoice("a", true)] }); await first; await duplicate; });
  expect(ipc.groupChoose).not.toHaveBeenCalled();
  expect(ipc.prepare).toHaveBeenCalledTimes(1);
});

test("explicit refresh observes another writer's member change and invalidates the old preview", async () => {
  const invalidated = vi.fn();
  const { result } = renderHook(() => useSessionImportUi(undefined, { onCandidateInvalidated: invalidated }));
  await start(result);
  act(() => { result.current.rememberPreview(groupCandidate("a").candidateId); result.current.rememberPreview(groupCandidate("b").candidateId); });
  await host.client.choose({}, a, groupCandidate("alternate").candidateId);
  await act(async () => { await result.current.refreshScan(); });
  expect(invalidated).toHaveBeenCalledExactlyOnceWith(groupCandidate("a").candidateId);
  expect(result.current.selectionRows[0].candidate.candidateId).toBe(groupCandidate("alternate").candidateId);
});

test("failed member CAS retains content and its preview binding for a later prepare", async () => {
  const invalidated = vi.fn();
  const { result } = renderHook(() => useSessionImportUi(undefined, { onCandidateInvalidated: invalidated }));
  await start(result);
  act(() => { result.current.rememberPreview(groupCandidate("a").candidateId); });
  await act(async () => { await result.current.selectCurrent(); });
  ipc.groupChoose.mockRejectedValueOnce(new Error("PLAN_STALE: CAS failed"));
  await act(async () => { expect(await result.current.chooseGroupMember(a, groupCandidate("alternate").candidateId)).toBe(false); });
  expect(invalidated).not.toHaveBeenCalled();
  await act(async () => { await result.current.previewSelected([slot("a")]); });
  expect(ipc.prepare).toHaveBeenCalledWith(groupScope.scanId, 1, "synthetic-selector", [slot("a")]);
});

test("cleared preference ACK invalidates its preview even if the subsequent membership read fails", async () => {
  const invalidated = vi.fn();
  const { result } = renderHook(() => useSessionImportUi(undefined, { onCandidateInvalidated: invalidated }));
  await start(result);
  await act(async () => { await result.current.chooseGroupMember(a, groupCandidate("alternate").candidateId); });
  invalidated.mockClear();
  act(() => { result.current.rememberPreview(groupCandidate("alternate").candidateId); result.current.rememberPreview(groupCandidate("b").candidateId); });
  ipc.groupMembership.mockRejectedValueOnce(new Error("NETWORK: unavailable"));
  await act(async () => { await result.current.clearSelected(); });
  expect(invalidated).toHaveBeenCalledExactlyOnceWith(groupCandidate("alternate").candidateId);
  expect(result.current.selectionReady).toBe(false);
  expect(result.current.error).toContain("NETWORK");
});

test("a newer override membership cannot silently change which snapshot gets prepared", async () => {
  const { result } = renderHook(() => useSessionImportUi());
  await start(result);
  act(() => { result.current.rememberPreview(groupCandidate("a").candidateId); });
  await act(async () => { await result.current.selectCurrent(); });
  ipc.groupMembership.mockResolvedValueOnce({ selectionId: "synthetic-selector", stateVersion: 3, choices: [groupChoice("a", true)] });
  await act(async () => { await result.current.previewSelected([slot("a")]); });
  expect(result.current.error).toContain("PLAN_STALE");
  expect(ipc.prepare).not.toHaveBeenCalled();
});

test("a raced prepare ACK with a different selection version is cancelled, not adopted", async () => {
  const { result } = renderHook(() => useSessionImportUi());
  await start(result);
  await act(async () => { await result.current.selectCurrent(); });
  ipc.prepare.mockResolvedValueOnce(batch(3));
  await act(async () => { await result.current.previewSelected([]); });
  expect(ipc.prepareCancel).toHaveBeenCalledExactlyOnceWith("synthetic-batch");
  expect(result.current.prepareBatch).toBeNull();
  expect(result.current.step).toBe("content");
  expect(result.current.error).toContain("PLAN_STALE");
});

test("a completed batch locks member edits until the explicit abandon flow", async () => {
  const { result } = renderHook(() => useSessionImportUi());
  await start(result);
  await act(async () => { await result.current.selectCurrent(); await result.current.previewSelected([]); });
  const saved = JSON.stringify(result.current.prepareBatch);
  await act(async () => { expect(await result.current.chooseGroupMember(a, groupCandidate("alternate").candidateId)).toBe(false); });
  expect(ipc.groupChoose).not.toHaveBeenCalled();
  expect(JSON.stringify(result.current.prepareBatch)).toBe(saved);
});
