/** @vitest-environment jsdom */
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { DiscoveredRoot, PrepareBatchState, PrepareBatchStatus, SourceRegistration, WorkspaceImportPreview } from "@/lib/api/sessionMigration";
import type { ImportSelectionView } from "@/lib/sessionImport/selectionView";
import { useSessionImportUi } from "./useSessionImport";
import { openSessionImport } from "@/lib/sessionImport/open";

const mocks = vi.hoisted(() => ({
  entries: vi.fn().mockResolvedValue([]),
  legacyPlan: vi.fn().mockResolvedValue({ planId: "legacy-plan", planHash: "legacy-hash" }),
  preparePlan: vi.fn().mockResolvedValue({ planId: "plan-1", planHash: "hash-1" }),
  associationPlan: vi.fn().mockResolvedValue({ planId: "linked-plan", planHash: "linked-hash" }),
  associationInspect: vi.fn(),
  entry: vi.fn(),
  prepareMappings: vi.fn().mockResolvedValue([]),
  prepareStart: vi.fn(),
  prepareGet: vi.fn(),
  prepareItems: vi.fn().mockResolvedValue({ items: [], nextCursor: null }),
  prepareCancel: vi.fn(),
  prepareRetry: vi.fn(),
  commit: vi.fn().mockResolvedValue({ jobId: "j1", state: "queued" }),
  recovery: vi.fn().mockResolvedValue({
    plan: null,
    job: null,
    draft: null,
    expired: false,
    failed: null,
  }),
  jobGet: vi.fn(),
  continueCreate: vi.fn(),
  continuePrepare: vi.fn(),
  retryFailed: vi.fn(),
  startScan: vi.fn(),
  stopScan: vi.fn(),
  closeSelection: vi.fn(),
  reopenSelection: vi.fn(),
  pickFiles: vi.fn().mockResolvedValue([]),
  pickFolder: vi.fn().mockResolvedValue(null),
  workspacePreview: vi.fn(),
  workspaceSave: vi.fn(),
  roots: vi.fn().mockResolvedValue([]),
  sources: vi.fn().mockResolvedValue([]),
  projects: vi.fn().mockResolvedValue([]),
  registerSource: vi.fn(),
  selected: new Set(["candidate-without-preview"]),
}));

function prepareStatus(
  state: PrepareBatchState = "completed",
  batchId = "batch-1",
  stateVersion = 1,
): PrepareBatchStatus {
  return {
    version: 1,
    batchId,
    scanId: "scan-1",
    epoch: 1,
    selectionId: "selection-1",
    selectionStateVersion: 1,
    state,
    stateVersion,
    total: 1,
    prepared: state === "completed" || state === "planned" ? 1 : 0,
    failed: 0,
    skipped: 0,
    active: state === "preparing" ? 1 : 0,
    cancelRequested: state === "cancelling" || state === "cancelled",
    createdAt: "2026-09-13T00:00:00Z",
    updatedAt: "2026-09-13T00:00:00Z",
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (cause: unknown) => void;
  const promise = new Promise<T>((ok, fail) => {
    resolve = ok;
    reject = fail;
  });
  return { promise, resolve, reject };
}

vi.mock("@/lib/api", () => ({
  sessionImportEntries: mocks.entries,
  sessionImportPlan: mocks.legacyPlan,
  sessionImportPreparePlan: mocks.preparePlan,
  sessionImportPreparedAssociationPlan: mocks.associationPlan,
  sessionImportPrepareMappings: mocks.prepareMappings,
  sessionImportCommit: mocks.commit,
  sessionImportRecovery: mocks.recovery,
  sessionImportJobGet: mocks.jobGet,
  sessionImportContinueCreate: mocks.continueCreate,
  sessionImportContinuePrepare: mocks.continuePrepare,
  sessionImportRetryFailed: mocks.retryFailed,
  sessionImportListRoots: mocks.roots,
  sessionImportSources: mocks.sources,
  sessionImportSourceRegister: mocks.registerSource,
  projectsList: mocks.projects,
  pickAttachFiles: mocks.pickFiles,
  pickDirectory: mocks.pickFolder,
}));

vi.mock("@/lib/api/sessionMigration", () => ({
  sessionImportEntries: mocks.entries,
  sessionImportEntry: mocks.entry,
  sessionImportWorkspacePreview: mocks.workspacePreview,
  sessionImportWorkspaceSave: mocks.workspaceSave,
  sessionImportPrepareStart: mocks.prepareStart,
  sessionImportPrepareGet: mocks.prepareGet,
  sessionImportPrepareItems: mocks.prepareItems,
  sessionImportPrepareCancel: mocks.prepareCancel,
  sessionImportPrepareRetry: mocks.prepareRetry,
}));

vi.mock("@/lib/api/sessionMigrationIdentityAssociation", () => ({
  sessionImportPreparedAssociationInspect: mocks.associationInspect,
}));

vi.mock("./useSessionImportJob", async () => {
  const React = await import("react");
  const importJobActive = (job?: { state?: string } | null) =>
    job?.state === "running" || job?.state === "queued";
  return {
    importJobActive,
    useSessionImportJob: () => {
      const [job, setJob] = React.useState<unknown>(null);
      return {
        job,
        setJob,
        jobActive: importJobActive(job as { state?: string } | null),
        mutateJob: async (operation: (id: string, request: { expectedStateVersion: number; requestId: string }) => Promise<unknown>) => {
          if (job) {
            const current = job as { jobId: string; stateVersion: number };
            setJob(await operation(current.jobId, { expectedStateVersion: current.stateVersion, requestId: "synthetic-job-action" }));
          }
        },
      };
    },
  };
});

// These tests own wizard/recovery/publication, not selection-domain behavior.
// The real scan/group/prepare bridge is exercised in useSessionImport.selection.test.tsx.
vi.mock("./useSessionImportSelection", () => ({
  useSessionImportSelection: () => ({
    scan: {
    candidates: [{ candidateId: "cand-1", sourceRevision: "rev-1", title: "One" }],
    selected: mocks.selected,
    scanGeneration: { current: 1 },
    scanState: { scanId: "scan-1", epoch: 1 },
    selection: { selectionId: "selection-1", selectedCount: 1, stateVersion: 1 },
    scanBusy: false,
    startScan: mocks.startScan,
    stopIncompleteScan: mocks.stopScan,
    changePage: vi.fn(),
    controlScan: vi.fn(),
    },
    candidates: [{ candidateId: "cand-1", sourceRevision: "rev-1", title: "One" }],
    selectionRows: [],
    selectionReady: true,
    selected: mocks.selected,
    selectedCount: mocks.selected.size,
    close: mocks.closeSelection,
    clearSource: mocks.closeSelection,
    reopen: mocks.reopenSelection,
    startScan: mocks.startScan,
    prepare: async (
      slots: import("@/lib/api/sessionMigration").PrepareOverride[],
      run: (view: ImportSelectionView, overrides: import("@/lib/api/sessionMigration").PrepareOverride[]) => Promise<PrepareBatchStatus | null>,
    ) => run({ domain: "independent-candidates", scope: { scanId: "scan-1", epoch: 1, generation: 1 }, ready: true,
      descriptor: { version: 1, mode: "explicit", scanId: "scan-1", epoch: 1,
        selectionId: "selection-1", selectedCount: mocks.selected.size, stateVersion: 1 } },
    slots.filter((slot) => mocks.selected.has(slot.candidateId))),
  }),
}));

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  mocks.entries.mockReset().mockResolvedValue([]);
  mocks.jobGet.mockReset();
  mocks.pickFiles.mockReset().mockResolvedValue([]);
  mocks.pickFolder.mockReset().mockResolvedValue(null);
  mocks.workspacePreview.mockReset();
  mocks.workspaceSave.mockReset();
  mocks.roots.mockReset().mockResolvedValue([]);
  mocks.sources.mockReset().mockResolvedValue([]);
  mocks.projects.mockReset().mockResolvedValue([]);
  mocks.registerSource.mockReset();
  mocks.selected = new Set(["candidate-without-preview"]);
  mocks.legacyPlan.mockResolvedValue({ planId: "legacy-plan", planHash: "legacy-hash" });
  mocks.preparePlan.mockResolvedValue({ planId: "plan-1", planHash: "hash-1" });
  mocks.prepareMappings.mockResolvedValue([]);
  mocks.prepareStart.mockResolvedValue(prepareStatus());
  mocks.prepareItems.mockResolvedValue({ items: [], nextCursor: null });
  mocks.prepareCancel.mockImplementation(async (batchId: string) =>
    prepareStatus("cancelled", batchId, 2),
  );
  mocks.prepareRetry.mockImplementation(async (batchId: string) =>
    prepareStatus("preparing", batchId, 2),
  );
  mocks.startScan.mockResolvedValue(true);
  mocks.commit.mockResolvedValue({ jobId: "j1", state: "queued" });
  mocks.recovery.mockResolvedValue({
    plan: null,
    job: null,
    draft: null,
    expired: false,
    failed: null,
  });
});

test("explicit completed-job viewing after restart is read-only and supersedes late recovery", async () => {
  const recovery = deferred<unknown>();
  mocks.recovery.mockReturnValueOnce(recovery.promise);
  mocks.jobGet.mockResolvedValue({ jobId: "older", state: "completed", stateVersion: 7 });
  const { result } = renderHook(useSessionImportUi);
  act(() => result.current.setOpen(true));
  await waitFor(() => expect(result.current.canOpenJobHistory).toBe(true));
  await act(async () => expect(result.current.openExistingJob("older", () => true)).resolves.toBe(true));
  expect(result.current.job).toMatchObject({ jobId: "older", stateVersion: 7 });
  expect(result.current.step).toBe("result");
  await act(async () => recovery.resolve({ job: { jobId: "startup", state: "running" } }));
  expect(result.current.job?.jobId).toBe("older");
  expect(result.current.pendingPlan).toBeNull();
  expect(mocks.commit).not.toHaveBeenCalled();
  expect(mocks.retryFailed).not.toHaveBeenCalled();
  expect(mocks.continueCreate).not.toHaveBeenCalled();
});

test.each(["close", "source", "cancel", "newer", "reopen", "unmount"])("late job view cannot replace %s intent", async (intent) => {
  const pending = deferred<unknown>();
  mocks.jobGet.mockReturnValueOnce(pending.promise).mockResolvedValueOnce({ jobId: "newer", state: "completed" });
  const { result, unmount } = renderHook(useSessionImportUi);
  act(() => result.current.setOpen(true));
  await waitFor(() => expect(result.current.canOpenJobHistory).toBe(true));
  let current = true;
  let flight!: Promise<boolean>;
  act(() => { flight = result.current.openExistingJob("older", () => current); });
  if (intent === "close") act(() => result.current.setOpen(false));
  if (intent === "source") act(() => result.current.chooseListedRoot("codex", "C:/newer"));
  if (intent === "cancel") current = false;
  if (intent === "reopen") act(() => openSessionImport({ projectId: "new-project" }));
  if (intent === "unmount") unmount();
  if (intent === "newer") await act(async () => { await result.current.openExistingJob("newer", () => true); });
  await act(async () => {
    pending.resolve({ jobId: "older", state: "completed" });
    await expect(flight).resolves.toBe(false);
  });
  expect(result.current.job?.jobId).toBe(intent === "newer" ? "newer" : undefined);
  expect(mocks.commit).not.toHaveBeenCalled();
});

test("a completed publication can browse other jobs without discarding its submitted prepare batch", async () => {
  mocks.prepareStart.mockResolvedValueOnce(prepareStatus());
  mocks.commit.mockResolvedValueOnce({ jobId: "finished", state: "completed", stateVersion: 3 });
  mocks.jobGet.mockResolvedValueOnce({ jobId: "older", state: "completed", stateVersion: 5 });
  const { result } = renderHook(useSessionImportUi);
  act(() => result.current.setOpen(true));
  await waitFor(() => expect(result.current.busy).toBe(false));
  await act(async () => { await result.current.previewSelected([]); });
  await act(async () => { await result.current.finishPrepare(); });
  await act(async () => { await result.current.commit(); });
  expect(result.current.step).toBe("result");
  expect(result.current.prepareBatch?.batchId).toBe("batch-1");
  expect(result.current.canOpenJobHistory).toBe(true);
  await act(async () => expect(result.current.openExistingJob("older", () => true)).resolves.toBe(true));
  expect(result.current.job?.jobId).toBe("older");
  expect(mocks.prepareCancel).not.toHaveBeenCalled();
  expect(mocks.commit).toHaveBeenCalledTimes(1);
  // A newer prepared batch must remain protected even if Results is visited.
  mocks.prepareStart.mockResolvedValueOnce(prepareStatus("completed", "batch-2"));
  await act(async () => { await result.current.previewSelected([]); });
  act(() => result.current.setStep("result"));
  expect(result.current.prepareBatch?.batchId).toBe("batch-2");
  expect(result.current.canOpenJobHistory).toBe(false);
  await act(async () => expect(result.current.openExistingJob("older", () => true)).resolves.toBe(false));
  expect(mocks.jobGet).toHaveBeenCalledTimes(1);
});

test("recovered pending plan and active job cannot be replaced by job history", async () => {
  mocks.recovery.mockResolvedValueOnce({ plan: { planId: "pending", planHash: "hash" } });
  const planHook = renderHook(useSessionImportUi);
  act(() => planHook.result.current.setOpen(true));
  await waitFor(() => expect(planHook.result.current.pendingPlan?.planId).toBe("pending"));
  await act(async () => expect(planHook.result.current.openExistingJob("old", () => true)).resolves.toBe(false));
  expect(planHook.result.current.canOpenJobHistory).toBe(false);
  planHook.unmount();
  mocks.recovery.mockResolvedValueOnce({ job: { jobId: "active", state: "running" } });
  const activeHook = renderHook(useSessionImportUi);
  act(() => activeHook.result.current.setOpen(true));
  await waitFor(() => expect(activeHook.result.current.jobActive).toBe(true));
  await act(async () => expect(activeHook.result.current.openExistingJob("old", () => true)).resolves.toBe(false));
  expect(mocks.jobGet).not.toHaveBeenCalled();
});

function workspaceFixture(): WorkspaceImportPreview {
  return {
    source: { path: "C:/empty.code-workspace", kind: "file" },
    workspace: { sourceWorkspaceId: "ws:empty", displayName: "empty", roots: [], evidence: [] },
    fingerprint: "fp", savedRevision: null,
  };
}

const registeredSource: SourceRegistration = {
  sourceId: "source-1", provider: "codex", rootPath: "C:/old", profile: "default",
};

test("late relocation picker cannot register after a newer source choice", async () => {
  const picker = deferred<string>();
  mocks.pickFolder.mockReturnValueOnce(picker.promise);
  const { result } = renderHook(useSessionImportUi);
  let relocating!: Promise<boolean>;
  act(() => { relocating = result.current.relocateRegistered(registeredSource); });
  act(() => result.current.chooseListedRoot("claude", "C:/new"));
  await act(async () => { picker.resolve("C:/relocated"); await expect(relocating).resolves.toBe(false); });
  expect(mocks.registerSource).not.toHaveBeenCalled();
  expect(result.current.choice).toMatchObject({ provider: "claude", path: "C:/new" });
  expect(result.current.busy).toBe(false);
});

test("late relocation registration cannot replace a new choice or clear its picker", async () => {
  const registering = deferred<SourceRegistration>();
  const picker = deferred<string>();
  mocks.pickFolder.mockResolvedValueOnce("C:/relocated").mockReturnValueOnce(picker.promise);
  mocks.registerSource.mockReturnValueOnce(registering.promise);
  const { result } = renderHook(useSessionImportUi);
  let relocating!: Promise<boolean>;
  let picking!: Promise<boolean>;
  await act(async () => { relocating = result.current.relocateRegistered(registeredSource); });
  act(() => result.current.chooseListedRoot("claude", "C:/new"));
  act(() => { picking = result.current.pickCustomRoot("cursor"); });
  await act(async () => {
    registering.resolve({ ...registeredSource, rootPath: "C:/relocated" });
    await expect(relocating).resolves.toBe(false);
  });
  expect(result.current.choice).toMatchObject({ provider: "claude", path: "C:/new" });
  expect(result.current.busy).toBe(true);
  await act(async () => { picker.resolve("C:/latest"); await expect(picking).resolves.toBe(true); });
  expect(result.current.choice?.path).toBe("C:/latest");
});

test("old roots and projects cannot overwrite a reopened wizard", async () => {
  const roots = deferred<DiscoveredRoot[]>();
  const projects = deferred<Array<{ id: string; name: string }>>();
  const nextRoots: DiscoveredRoot[] = [{ provider: "codex", path: "C:/new", kind: "default", exists: true, readable: true }];
  mocks.roots.mockReturnValueOnce(roots.promise).mockResolvedValueOnce(nextRoots);
  mocks.projects.mockReturnValueOnce(projects.promise).mockResolvedValueOnce([{ id: "new", name: "New" }]);
  const { result } = renderHook(useSessionImportUi);
  await act(async () => result.current.setOpen(true));
  act(() => result.current.setOpen(false));
  await act(async () => result.current.setOpen(true));
  expect(result.current.roots).toEqual(nextRoots);
  await act(async () => {
    roots.resolve([]);
    projects.resolve([{ id: "old", name: "Old" }]);
  });
  expect(result.current.roots).toEqual(nextRoots);
  expect(result.current.projects).toEqual([{ id: "new", name: "New" }]);
});

test("old root failure cannot release a reopened wizard loading state", async () => {
  const oldRoots = deferred<DiscoveredRoot[]>();
  const currentRoots = deferred<DiscoveredRoot[]>();
  mocks.roots.mockReturnValueOnce(oldRoots.promise).mockReturnValueOnce(currentRoots.promise);
  const { result } = renderHook(useSessionImportUi);
  await act(async () => result.current.setOpen(true));
  act(() => result.current.setOpen(false));
  await act(async () => result.current.setOpen(true));
  await act(async () => oldRoots.reject(new Error("old discovery failure")));
  expect(result.current.error).toBeNull();
  expect(result.current.busy).toBe(true);
  await act(async () => currentRoots.resolve([]));
  expect(result.current.busy).toBe(false);
});

test("initial discovery cannot restore the old registration after relocation", async () => {
  const roots = deferred<DiscoveredRoot[]>();
  const relocated = { ...registeredSource, rootPath: "C:/relocated" };
  mocks.roots.mockReturnValueOnce(roots.promise);
  mocks.sources.mockResolvedValueOnce([registeredSource]).mockResolvedValueOnce([relocated]);
  mocks.pickFolder.mockResolvedValueOnce(relocated.rootPath);
  mocks.registerSource.mockResolvedValueOnce(relocated);
  const { result } = renderHook(useSessionImportUi);
  await act(async () => result.current.setOpen(true));
  await act(() => result.current.relocateRegistered(registeredSource));
  expect(result.current.registrations).toEqual([relocated]);
  await act(async () => roots.resolve([]));
  expect(result.current.registrations).toEqual([relocated]);
  expect(result.current.choice).toMatchObject({ kind: "relocate", path: relocated.rootPath });
});

test("workspace file picker previews without registering or scanning default histories", async () => {
  mocks.pickFiles.mockResolvedValue(["C:/empty.code-workspace"]);
  mocks.workspacePreview.mockResolvedValue(workspaceFixture());
  const { result } = renderHook(useSessionImportUi);
  await act(async () => {
    await expect(result.current.pickCustomFile()).resolves.toBe(true);
  });
  expect(mocks.workspacePreview).toHaveBeenCalledWith({ path: "C:/empty.code-workspace", kind: "file" });
  expect(result.current.workspaceUi.mode).toBe("preview");
  expect(result.current.choice).toBeNull();
  await act(() => result.current.scan());
  expect(mocks.startScan).not.toHaveBeenCalled();
  expect(mocks.workspaceSave).not.toHaveBeenCalled();
});

test("workspace folder picker has a distinct intent from transcript folder", async () => {
  mocks.pickFolder.mockResolvedValue("C:/project");
  mocks.workspacePreview.mockResolvedValue(workspaceFixture());
  const { result } = renderHook(useSessionImportUi);
  await act(() => result.current.pickWorkspaceFolder());
  expect(mocks.workspacePreview).toHaveBeenCalledWith({ path: "C:/project", kind: "folder" });
  await act(() => result.current.scan());
  expect(mocks.startScan).not.toHaveBeenCalled();
  await act(() => result.current.pickCustomRoot("codex"));
  expect(result.current.workspaceUi.mode).toBeNull();
  expect(result.current.choice).toMatchObject({ kind: "custom-folder", provider: "codex" });
});

test("custom folder scan preserves the explicitly selected provider", async () => {
  mocks.pickFolder.mockResolvedValue("C:/portable-cursor-data");
  const { result } = renderHook(useSessionImportUi);
  await act(() => result.current.pickCustomRoot("cursor"));
  await act(() => result.current.scan());
  expect(mocks.startScan).toHaveBeenCalledWith([
    { provider: "cursor", path: "C:/portable-cursor-data" },
  ]);
});

test("closing while picker is open ignores its eventual file selection", async () => {
  const picker = deferred<string[]>();
  mocks.pickFiles.mockReturnValue(picker.promise);
  const { result } = renderHook(useSessionImportUi);
  let picking!: Promise<boolean>;
  act(() => { picking = result.current.pickCustomFile(); });
  act(() => result.current.setOpen(false));
  await act(async () => { picker.resolve(["C:/empty.code-workspace"]); await expect(picking).resolves.toBe(false); });
  expect(mocks.workspacePreview).not.toHaveBeenCalled();
  expect(result.current.busy).toBe(false);
});

test("source switch invalidates an earlier picker result", async () => {
  const picker = deferred<string[]>();
  mocks.pickFiles.mockReturnValue(picker.promise);
  const { result } = renderHook(useSessionImportUi);
  let picking!: Promise<boolean>;
  act(() => { picking = result.current.pickCustomFile(); });
  act(() => result.current.chooseListedRoot("codex", "C:/copy"));
  await act(async () => { picker.resolve(["C:/empty.code-workspace"]); await expect(picking).resolves.toBe(false); });
  expect(mocks.workspacePreview).not.toHaveBeenCalled();
  expect(result.current.choice?.provider).toBe("codex");
});

test("back from loading workspace ignores late data and releases picker busy", async () => {
  const loading = deferred<WorkspaceImportPreview>();
  mocks.pickFiles.mockResolvedValue(["C:/empty.code-workspace"]);
  mocks.workspacePreview.mockReturnValue(loading.promise);
  const { result } = renderHook(useSessionImportUi);
  let picking!: Promise<boolean>;
  await act(async () => { picking = result.current.pickCustomFile(); });
  act(() => result.current.workspaceUi.reset());
  expect(result.current.busy).toBe(false);
  await act(async () => { loading.resolve(workspaceFixture()); await picking; });
  expect(result.current.workspaceUi.mode).toBeNull();
});

test("ordinary JSON file still uses the explicit transcript scan path", async () => {
  mocks.pickFiles.mockResolvedValue(["C:/export.json"]);
  const { result } = renderHook(useSessionImportUi);
  await act(async () => {
    await expect(result.current.pickCustomFile()).resolves.toBe(true);
  });
  await act(() => result.current.scan());
  expect(mocks.startScan).toHaveBeenCalledWith([{ provider: "generic-file", path: "C:/export.json" }]);
  expect(mocks.workspacePreview).not.toHaveBeenCalled();
});

test("commit fails closed before IPC when no prepare batch exists", async () => {
  const { result } = renderHook(() => useSessionImportUi());

  await act(async () => {
    await result.current.commit();
  });

  expect(mocks.legacyPlan).not.toHaveBeenCalled();
  expect(mocks.preparePlan).not.toHaveBeenCalled();
  expect(result.current.error).toBe("PLAN_STALE: prepare batch is unavailable");
  expect(result.current.busy).toBe(false);
});

test("selected content follows prepare, mappings, durable plan, then commit", async () => {
  mocks.selected = new Set(["cand-1"]);
  const { result } = renderHook(() => useSessionImportUi());
  await act(async () => {
    await result.current.previewSelected([
      {
        candidateId: "cand-1",
        previewId: "kept-preview",
        leafId: "leaf-a",
        selection: {
          version: 2,
          eventIds: ["e1"],
          pinnedEventIds: [],
          assetIds: [],
          fields: {
            messageText: true,
            toolCallName: true,
            toolCallInput: true,
            toolResultContent: true,
            compactionSummary: true,
            visibleReasoning: false,
          },
          includeSubagents: false,
        },
      },
    ]);
  });
  expect(mocks.prepareStart).toHaveBeenCalledWith(
    "scan-1",
    1,
    "selection-1",
    [expect.objectContaining({ candidateId: "cand-1", previewId: "kept-preview" })],
  );
  expect(result.current.step).toBe("prepare");
  expect(result.current.error).toBeNull();
  await act(async () => {
    await result.current.finishPrepare();
  });
  expect(mocks.prepareMappings).toHaveBeenCalledWith("batch-1", null);
  expect(result.current.step).toBe("mapping");
  await act(async () => {
    await result.current.commit();
  });
  expect(mocks.preparePlan).toHaveBeenCalledWith(
    "batch-1",
    [expect.objectContaining({ rootId: "unassigned", unassigned: true })],
  );
  expect(mocks.legacyPlan).not.toHaveBeenCalled();
  // Undo tombstones stay in force unless the user explicitly opts in.
  expect(mocks.commit).toHaveBeenCalledWith("plan-1", "hash-1", expect.any(String), false);
});

test("an explicit re-import choice reaches commit once and then resets", async () => {
  mocks.selected = new Set(["cand-1"]);
  const { result } = renderHook(() => useSessionImportUi());
  await act(async () => {
    await result.current.previewSelected([{ candidateId: "cand-1", previewId: "kept-preview", leafId: "leaf-a",
      selection: { version: 2, eventIds: ["e1"], pinnedEventIds: [], assetIds: [], includeSubagents: false,
        fields: { messageText: true, toolCallName: true, toolCallInput: true, toolResultContent: true,
          compactionSummary: true, visibleReasoning: false } } }]);
  });
  await act(async () => { await result.current.finishPrepare(); });
  expect(result.current.explicitReimport).toBe(false);
  act(() => result.current.setExplicitReimport(true));
  await act(async () => { await result.current.commit(); });
  expect(mocks.commit).toHaveBeenLastCalledWith("plan-1", "hash-1", expect.any(String), true);
  expect(result.current.explicitReimport).toBe(false);
});

test("a failed explicit re-import retries with the same choice and then disarms it", async () => {
  mocks.selected = new Set(["cand-1"]);
  const { result } = renderHook(() => useSessionImportUi());
  await act(async () => {
    await result.current.previewSelected([{ candidateId: "cand-1", previewId: "kept-preview", leafId: "leaf-a",
      selection: { version: 2, eventIds: ["e1"], pinnedEventIds: [], assetIds: [], includeSubagents: false,
        fields: { messageText: true, toolCallName: true, toolCallInput: true, toolResultContent: true,
          compactionSummary: true, visibleReasoning: false } } }]);
  });
  await act(async () => { await result.current.finishPrepare(); });
  act(() => result.current.setExplicitReimport(true));
  mocks.commit.mockRejectedValueOnce(new Error("SOURCE_BUSY: synthetic"));
  await act(async () => { await result.current.commit(); });
  expect(result.current.pendingPlan).not.toBeNull();
  const request = mocks.commit.mock.calls.at(-1)![2];
  await act(async () => { await result.current.commit(); });
  expect(mocks.commit).toHaveBeenLastCalledWith("plan-1", "hash-1", request, true);
  expect(result.current.explicitReimport).toBe(false);
});

test("publication reports both startup phases and stops reporting once the job is adopted", async () => {
  const plan = deferred<{ planId: string; planHash: string }>();
  const job = deferred<{ jobId: string; state: string }>();
  const entries = deferred<{ items: [] }>();
  mocks.prepareStart.mockResolvedValueOnce(prepareStatus());
  mocks.preparePlan.mockReturnValueOnce(plan.promise);
  mocks.commit.mockReturnValueOnce(job.promise);
  const { result } = renderHook(useSessionImportUi);
  await act(async () => { await result.current.previewSelected([]); });
  await act(async () => { await result.current.finishPrepare(); });
  expect(result.current.commitPhase).toBeNull();
  let publishing!: Promise<void>;
  act(() => { publishing = result.current.commit(); });
  expect(result.current.commitPhase).toBe("planning");
  expect(mocks.commit).not.toHaveBeenCalled();
  act(() => result.current.setOpen(false));
  act(() => openSessionImport({}));
  expect(result.current.open).toBe(true);
  expect(result.current.commitPhase).toBe("planning");
  await act(async () => plan.resolve({ planId: "plan-1", planHash: "hash-1" }));
  expect(result.current.commitPhase).toBe("submitting");
  expect(result.current.pendingPlan?.planId).toBe("plan-1");
  act(() => result.current.setOpen(false));
  act(() => openSessionImport({}));
  expect(result.current.commitPhase).toBe("submitting");
  mocks.entries.mockReturnValue(entries.promise);
  await act(async () => job.resolve({ jobId: "j1", state: "queued" }));
  expect(result.current.job?.jobId).toBe("j1");
  expect(result.current.step).toBe("import");
  expect(result.current.commitPhase).toBeNull();
  expect(result.current.busy).toBe(true);
  await act(async () => { entries.resolve({ items: [] }); await publishing; });
  expect(result.current.busy).toBe(false);
  expect(result.current.commitPhase).toBeNull();
});

test.each(["plan", "commit", "recovered"])(
  "mapping publication owns navigation while %s is pending",
  async (stage) => {
    if (stage === "recovered") {
      mocks.recovery.mockResolvedValueOnce({ plan: { planId: "plan-1", planHash: "hash-1" } });
    }
    mocks.prepareStart.mockReset().mockResolvedValue(prepareStatus());
    const pending = deferred<{ planId: string; planHash: string } | { jobId: string; state: string }>();
    const { result } = renderHook(useSessionImportUi);
    if (stage === "recovered") {
      await waitFor(() => expect(result.current.pendingPlan).not.toBeNull());
    } else {
      await act(async () => { await result.current.previewSelected([]); });
      await act(async () => { await result.current.finishPrepare(); });
    }
    if (stage === "plan") mocks.preparePlan.mockReturnValueOnce(pending.promise);
    else mocks.commit.mockReturnValueOnce(pending.promise);
    let publishing!: Promise<void>;
    act(() => { publishing = result.current.commit(); });
    if (stage === "commit") await waitFor(() => expect(mocks.commit).toHaveBeenCalled());
    expect(result.current.commitPhase).toBe(stage === "plan" ? "planning" : "submitting");
    let abandoned: boolean | undefined;
    await act(async () => { abandoned = await result.current.abandonPrepare("content"); });
    const stepWhilePublishing = result.current.step;
    await act(async () => {
      pending.resolve(stage === "plan"
        ? { planId: "plan-1", planHash: "hash-1" }
        : { jobId: "j1", state: "queued" });
      await publishing;
    });
    expect(abandoned).toBe(false);
    expect(stepWhilePublishing).toBe("mapping");
    expect(result.current.commitPhase).toBeNull();
    expect(mocks.commit).toHaveBeenCalledTimes(1);
    expect(result.current.step).toBe("import");
    expect(result.current.busy).toBe(false);
  },
);

test.each(["prepared", "recovered"])(
  "mapping publication coalesces same-tick duplicate %s submissions",
  async (mode) => {
    if (mode === "recovered") {
      mocks.recovery.mockResolvedValueOnce({ plan: { planId: "plan-1", planHash: "hash-1" } });
    }
    mocks.prepareStart.mockReset().mockResolvedValue(prepareStatus());
    const pending = deferred<{ jobId: string; state: string }>();
    const { result } = renderHook(useSessionImportUi);
    if (mode === "recovered") {
      await waitFor(() => expect(result.current.pendingPlan).not.toBeNull());
    } else {
      await act(async () => { await result.current.previewSelected([]); });
      await act(async () => { await result.current.finishPrepare(); });
    }
    mocks.commit.mockReturnValue(pending.promise);
    let first!: Promise<void>;
    let second!: Promise<void>;
    act(() => {
      first = result.current.commit();
      second = result.current.commit();
    });
    await act(async () => {
      pending.resolve({ jobId: "j1", state: "queued" });
      await Promise.all([first, second]);
    });
    expect(mocks.preparePlan).toHaveBeenCalledTimes(mode === "prepared" ? 1 : 0);
    expect(mocks.commit).toHaveBeenCalledTimes(1);
    expect(result.current.step).toBe("import");
    expect(result.current.busy).toBe(false);
  },
);

test("mapping publication cannot overtake an in-flight prepare discard", async () => {
  const cancellation = deferred<PrepareBatchStatus>();
  mocks.prepareStart.mockReset().mockResolvedValue(prepareStatus("preparing"));
  mocks.prepareCancel.mockReturnValueOnce(cancellation.promise);
  const { result } = renderHook(useSessionImportUi);
  await act(async () => { await result.current.previewSelected([]); });
  let leaving!: Promise<boolean>;
  let publishing!: Promise<void>;
  act(() => {
    leaving = result.current.abandonPrepare("content");
    publishing = result.current.commit();
  });
  await act(async () => {
    cancellation.resolve(prepareStatus("cancelling", "batch-1", 2));
    await Promise.all([leaving, publishing]);
  });
  expect(await leaving).toBe(true);
  expect(mocks.preparePlan).not.toHaveBeenCalled();
  expect(mocks.commit).not.toHaveBeenCalled();
  expect(result.current.step).toBe("content");
});

test.each(["plan", "commit"])("mapping publication releases ownership after %s failure and retries safely", async (stage) => {
  mocks.prepareStart.mockResolvedValueOnce(prepareStatus());
  const error = stage === "plan" ? "PLAN_STALE: synthetic plan" : "SOURCE_BUSY: synthetic writer";
  (stage === "plan" ? mocks.preparePlan : mocks.commit).mockRejectedValueOnce(new Error(error));
  const { result } = renderHook(useSessionImportUi);
  await act(async () => { await result.current.previewSelected([]); });
  await act(async () => { await result.current.finishPrepare(); });
  await act(async () => { await result.current.commit(); });
  expect(result.current.busy).toBe(false);
  expect(result.current.error).toBe(error);
  expect(result.current.commitPhase).toBeNull();
  expect(result.current.step).toBe("mapping");
  expect(result.current.pendingPlan !== null).toBe(stage === "commit");
  expect(mocks.commit).toHaveBeenCalledTimes(stage === "commit" ? 1 : 0);
  const firstRequest = mocks.commit.mock.calls[0];
  await act(async () => { await result.current.commit(); });
  expect(mocks.preparePlan).toHaveBeenCalledTimes(stage === "plan" ? 2 : 1);
  if (stage === "commit") expect(mocks.commit.mock.calls[1]).toEqual(firstRequest);
  expect(result.current.step).toBe("import");
  expect(result.current.commitPhase).toBeNull();
  expect(result.current.error).toBeNull();
});

test.each(["scan", "file"])("mapping publication cannot be replaced by a new %s intent", async (intent) => {
  const pending = deferred<{ jobId: string; state: string }>();
  mocks.prepareStart.mockReset().mockResolvedValue(prepareStatus());
  mocks.commit.mockReturnValueOnce(pending.promise);
  const { result } = renderHook(useSessionImportUi);
  await act(async () => { await result.current.previewSelected([]); });
  await act(async () => { await result.current.finishPrepare(); });
  let publishing!: Promise<void>;
  act(() => { publishing = result.current.commit(); });
  await waitFor(() => expect(mocks.commit).toHaveBeenCalledTimes(1));
  act(() => result.current.setOpen(false));
  await act(async () => {
    if (intent === "scan") await result.current.scan();
    else openSessionImport({ mode: "file", projectId: "different-project" });
  });
  expect(result.current.step).toBe("mapping");
  expect(result.current.projectId).toBeNull();
  expect(mocks.startScan).not.toHaveBeenCalled();
  expect(mocks.pickFiles).not.toHaveBeenCalled();
  await act(async () => {
    pending.resolve({ jobId: "j1", state: "queued" });
    await publishing;
  });
  expect(result.current.step).toBe("import");
  expect(result.current.job?.jobId).toBe("j1");
});

test("mapping publication is not replaced by late startup recovery", async () => {
  const recovery = deferred<unknown>();
  const pending = deferred<{ jobId: string; state: string }>();
  mocks.recovery.mockReturnValueOnce(recovery.promise);
  mocks.prepareStart.mockReset().mockResolvedValue(prepareStatus());
  mocks.commit.mockReturnValueOnce(pending.promise);
  const { result } = renderHook(useSessionImportUi);
  await act(async () => { await result.current.previewSelected([]); });
  await act(async () => { await result.current.finishPrepare(); });
  let publishing!: Promise<void>;
  act(() => { publishing = result.current.commit(); });
  await waitFor(() => expect(mocks.commit).toHaveBeenCalledTimes(1));
  await act(async () => recovery.resolve({
    plan: { planId: "obsolete-plan", planHash: "obsolete-hash" },
  }));
  expect(result.current.pendingPlan?.planId).toBe("plan-1");
  expect(result.current.restoreStatus).toBeNull();
  await act(async () => {
    pending.resolve({ jobId: "j1", state: "queued" });
    await publishing;
  });
  expect(result.current.step).toBe("import");
});

test.each(["start", "retry"])("closing the wizard stops an unacknowledged prepare %s", async (stage) => {
  const accepted = deferred<PrepareBatchStatus>();
  mocks.prepareStart.mockReset().mockResolvedValue(prepareStatus("cancelled"));
  mocks.prepareCancel.mockResolvedValueOnce(prepareStatus("cancelled", "batch-1", 3));
  const { result } = renderHook(useSessionImportUi);
  if (stage === "retry") {
    await act(async () => { await result.current.previewSelected([]); });
    mocks.prepareRetry.mockReturnValueOnce(accepted.promise);
  } else {
    mocks.prepareStart.mockReturnValueOnce(accepted.promise);
  }
  let preparing!: Promise<unknown>;
  act(() => {
    preparing = stage === "retry"
      ? result.current.retryPrepare()
      : result.current.previewSelected([]);
  });
  act(() => result.current.setOpen(false));
  await act(async () => {
    accepted.resolve(prepareStatus("preparing", "batch-1", 2));
    await preparing;
  });
  expect(mocks.prepareCancel).toHaveBeenCalledTimes(1);
  expect(mocks.prepareCancel).toHaveBeenCalledWith("batch-1");
  expect(result.current.prepareBatch?.state).toBe("cancelled");
  expect(result.current.prepareActive).toBe(false);
  expect(result.current.open).toBe(false);
  expect(result.current.busy).toBe(false);
  expect(mocks.commit).not.toHaveBeenCalled();
});

test("closing the wizard requests cancellation for an active prepare batch", async () => {
  mocks.prepareStart.mockResolvedValueOnce(prepareStatus("preparing"));
  const { result } = renderHook(() => useSessionImportUi());
  await act(async () => {
    await result.current.previewSelected([]);
  });

  act(() => result.current.setOpen(false));
  await waitFor(() => expect(mocks.prepareCancel).toHaveBeenCalledWith("batch-1"));
  expect(mocks.stopScan).toHaveBeenCalledTimes(1);
});

test("a new scan waits for active prepare cancellation acknowledgement", async () => {
  const cancellation = deferred<PrepareBatchStatus>();
  mocks.prepareStart.mockResolvedValueOnce(prepareStatus("preparing"));
  mocks.prepareCancel.mockReturnValueOnce(cancellation.promise);
  const { result } = renderHook(() => useSessionImportUi());
  await act(async () => {
    await result.current.previewSelected([]);
  });

  let scanPromise!: Promise<void>;
  act(() => {
    scanPromise = result.current.scan();
  });
  expect(mocks.startScan).not.toHaveBeenCalled();
  await act(async () => cancellation.resolve(prepareStatus("cancelling", "batch-1", 2)));
  await scanPromise;

  expect(mocks.startScan).toHaveBeenCalledTimes(1);
  expect(result.current.prepareBatch).toBeNull();
  expect(result.current.step).toBe("content");
});

test("late prepare mappings cannot reopen a discarded batch", async () => {
  const mappingRequest = deferred<[]>();
  mocks.prepareMappings.mockReturnValueOnce(mappingRequest.promise);
  const { result } = renderHook(() => useSessionImportUi());
  await act(async () => {
    await result.current.previewSelected([]);
  });

  let finishPromise!: Promise<void>;
  act(() => {
    finishPromise = result.current.finishPrepare();
  });
  await act(async () => {
    await result.current.abandonPrepare("content");
  });
  await act(async () => mappingRequest.resolve([]));
  await finishPromise;

  expect(result.current.step).toBe("content");
  expect(result.current.mappings).toEqual([]);
});

test("remounted wizard commits the persisted plan without calling plan again", async () => {
  mocks.selected = new Set(["cand-1"]);
  mocks.commit.mockRejectedValueOnce(new Error("host remounted"));
  const first = renderHook(() => useSessionImportUi());
  await act(async () => {
    await first.result.current.previewSelected([
      {
        candidateId: "cand-1",
        previewId: "kept-preview",
        leafId: "leaf-a",
        selection: {
          version: 2,
          eventIds: ["e1"],
          pinnedEventIds: [],
          assetIds: [],
          fields: {
            messageText: true,
            toolCallName: true,
            toolCallInput: true,
            toolResultContent: true,
            compactionSummary: true,
            visibleReasoning: false,
          },
          includeSubagents: false,
        },
      },
    ]);
  });
  await act(async () => {
    await first.result.current.finishPrepare();
  });
  await act(async () => {
    await first.result.current.commit();
  });
  expect(mocks.preparePlan).toHaveBeenCalledTimes(1);
  expect(mocks.commit).toHaveBeenCalledTimes(1);
  const requestId = mocks.commit.mock.calls[0][2] as string;
  expect(requestId).toBeTruthy();
  expect(first.result.current.busy).toBe(false);
  first.unmount();

  mocks.commit.mockResolvedValue({ jobId: "j1", state: "queued" });
  mocks.recovery.mockResolvedValue({
    plan: { planId: "plan-1", planHash: "hash-1" },
    job: null,
    draft: null,
    expired: false,
    failed: null,
  });
  const remounted = renderHook(() => useSessionImportUi());
  await waitFor(() => {
    expect(remounted.result.current.restoreStatus).toBe("plan");
  });
  await act(async () => openSessionImport());
  expect(remounted.result.current.open).toBe(true);
  expect(remounted.result.current.step).toBe("mapping");
  await act(async () => {
    await remounted.result.current.commit();
  });
  expect(mocks.preparePlan).toHaveBeenCalledTimes(1);
  expect(mocks.commit).toHaveBeenCalledTimes(2);
  expect(mocks.commit.mock.calls[1][0]).toBe("plan-1");
  expect(mocks.commit.mock.calls[1][1]).toBe("hash-1");
  expect(mocks.commit.mock.calls[1][2]).not.toBe(requestId);
  expect(remounted.result.current.error).toBeNull();
  expect(remounted.result.current.busy).toBe(false);
});

test("remount restores a pending job from durable recovery", async () => {
  mocks.recovery.mockResolvedValueOnce({
    plan: null,
    job: { jobId: "job-1", state: "running", stateVersion: 3 },
    draft: null,
    expired: false,
    failed: null,
  });
  const { result } = renderHook(() => useSessionImportUi());
  await waitFor(() => {
    expect(result.current.restoreStatus).toBe("job");
  });
  expect(result.current.job).toMatchObject({ jobId: "job-1" });
  expect(result.current.step).toBe("import");
  await act(async () => openSessionImport());
  expect(result.current.step).toBe("import");
});

test.each(["cancelled", "failed", "completed_with_issues"])(
  "recovered %s job shows results and retries the original job",
  async (state) => {
    mocks.recovery.mockResolvedValueOnce({
      plan: null, job: { jobId: "original-job", state, stateVersion: 4 },
      draft: null, expired: false, failed: null,
    });
    mocks.retryFailed.mockResolvedValueOnce({ jobId: "original-job", state: "queued", stateVersion: 5 });
    const { result } = renderHook(() => useSessionImportUi());
    await waitFor(() => expect(result.current.step).toBe("result"));
    await act(async () => openSessionImport());
    expect(result.current.open).toBe(true);
    expect(result.current.step).toBe("result");
    expect(result.current.pendingPlan).toBeNull();
    await act(async () => { await result.current.retryFailed(); });
    expect(mocks.retryFailed).toHaveBeenCalledWith("original-job", {
      expectedStateVersion: 4, requestId: "synthetic-job-action",
    });
    expect(result.current.job?.jobId).toBe("original-job");
    expect(result.current.step).toBe("import");
    expect(mocks.preparePlan).not.toHaveBeenCalled();
    expect(mocks.commit).not.toHaveBeenCalled();
  },
);

test.each(["scan", "abandon", "file"])("%s discards a recovered plan before a different import", async (action) => {
  mocks.recovery.mockResolvedValueOnce({
    plan: { planId: "old-plan", planHash: "old-hash" }, job: null,
    draft: null, expired: false, failed: null,
  });
  const { result } = renderHook(() => useSessionImportUi());
  await waitFor(() => expect(result.current.pendingPlan?.planId).toBe("old-plan"));
  await act(async () => {
    if (action === "scan") await result.current.scan();
    else if (action === "file") openSessionImport({ mode: "file" });
    else await result.current.abandonPrepare("source");
  });
  expect(result.current.pendingPlan).toBeNull();
  expect(result.current.restoreStatus).toBeNull();
  await act(async () => { await result.current.commit(); });
  expect(mocks.commit).not.toHaveBeenCalled();
  expect(result.current.error).toContain("prepare batch is unavailable");
});

test("same-cwd sources keep independent project choices through plan submission", async () => {
  const rows = ["a", "b"].map((id) => ({
    rootId: `source-${id}`, source: { sourceId: id, provider: "codex" },
    suggestedTarget: "/same/repo", unassigned: true, userChoiceVersion: 1,
  }));
  mocks.prepareMappings.mockResolvedValueOnce(rows);
  const { result } = renderHook(() => useSessionImportUi());
  await act(async () => { await result.current.previewSelected([]); });
  await act(async () => { await result.current.finishPrepare(); });
  act(() => result.current.setMappingProject("source-a", "project-a"));
  act(() => result.current.setMappingProject("source-b", "project-b"));
  expect(result.current.mappings.map((row) => row.targetProjectId)).toEqual(["project-a", "project-b"]);
  await act(async () => { await result.current.commit(); });
  expect(mocks.preparePlan).toHaveBeenCalledWith("batch-1", [
    expect.objectContaining({ source: rows[0].source, rootId: "source-a", targetProjectId: "project-a" }),
    expect.objectContaining({ source: rows[1].source, rootId: "source-b", targetProjectId: "project-b" }),
  ]);
});

test("late startup recovery cannot replace a newly started scan", async () => {
  const recovery = deferred<unknown>();
  mocks.recovery.mockReturnValueOnce(recovery.promise);
  const { result } = renderHook(() => useSessionImportUi());
  await act(async () => { await result.current.scan(); });
  await act(async () => recovery.resolve({
    plan: { planId: "old-plan", planHash: "old-hash" }, job: null,
    draft: null, expired: false, failed: null,
  }));
  expect(result.current.pendingPlan).toBeNull();
  expect(result.current.restoreStatus).toBeNull();
  expect(result.current.step).toBe("content");
});

test("remount restores a pending draft from durable recovery", async () => {
  mocks.recovery.mockResolvedValueOnce({
    plan: null,
    job: null,
    draft: { draftId: "d1", draftHash: "h1", text: "hi", importId: "imp:1" },
    expired: false,
    failed: null,
  });
  const { result } = renderHook(() => useSessionImportUi());
  await waitFor(() => {
    expect(result.current.restoreStatus).toBe("draft");
  });
  expect(result.current.draft).toMatchObject({ draftId: "d1", draftHash: "h1" });
});

test("failed recovery is fail-closed and does not commit", async () => {
  mocks.recovery.mockResolvedValueOnce({
    plan: null,
    job: null,
    draft: null,
    expired: false,
    failed: "PLAN_STALE: durable plan record was modified",
  });
  const { result } = renderHook(() => useSessionImportUi());
  await waitFor(() => {
    expect(result.current.restoreStatus).toBe("failed");
  });
  expect(result.current.error).toMatch(/modified/i);
  await act(async () => {
    await result.current.commit();
  });
  expect(mocks.commit).not.toHaveBeenCalled();
});

test("expired recovery is fail-closed and does not commit", async () => {
  mocks.recovery.mockResolvedValueOnce({
    plan: null,
    job: null,
    draft: null,
    expired: true,
    failed: null,
  });
  const { result } = renderHook(() => useSessionImportUi());
  await waitFor(() => {
    expect(result.current.restoreStatus).toBe("expired");
  });
  expect(result.current.error).toMatch(/expired/i);
  await act(async () => {
    await result.current.commit();
  });
  expect(mocks.commit).not.toHaveBeenCalled();
});

test("hashchange restores the same imported revision, leaf, and event anchor", async () => {
  const { result } = renderHook(() => useSessionImportUi());
  act(() => {
    window.location.hash =
      "#/imported/imp:00000000-0000-4000-8000-000000000001/rev/rev-a/leaf/a3/event/e5000";
    window.dispatchEvent(new Event("hashchange"));
  });
  expect(result.current.readerId).toBe("imp:00000000-0000-4000-8000-000000000001");
  expect(result.current.readerRevision).toBe("rev-a");
  expect(result.current.readerLeaf).toBe("a3");
  expect(result.current.readerAnchor).toBe("e5000");
});

test("popstate / back-forward restores the same imported revision and leaf", async () => {
  const { result } = renderHook(() => useSessionImportUi());
  act(() => {
    window.location.hash =
      "#/imported/imp:00000000-0000-4000-8000-000000000001/rev/rev-b/leaf/leaf-2";
    window.dispatchEvent(new PopStateEvent("popstate"));
  });
  expect(result.current.readerId).toBe("imp:00000000-0000-4000-8000-000000000001");
  expect(result.current.readerRevision).toBe("rev-b");
  expect(result.current.readerLeaf).toBe("leaf-2");
  expect(result.current.readerAnchor).toBeNull();
});

test("continuation create failure keeps the draft and a retryable error", async () => {
  mocks.recovery.mockResolvedValueOnce({
    plan: null,
    job: null,
    draft: { draftId: "d1", draftHash: "h1", text: "hi", importId: "imp:1" },
    expired: false,
    failed: null,
  });
  mocks.continueCreate.mockRejectedValueOnce(new Error("handoff failed"));
  const { result } = renderHook(() => useSessionImportUi());
  await waitFor(() => {
    expect(result.current.draft).toMatchObject({ draftId: "d1" });
  });
  let nativeId: string | null = "sentinel";
  await act(async () => {
    nativeId = await result.current.createContinue();
  });
  expect(nativeId).toBeNull();
  expect(result.current.draft).toMatchObject({ draftId: "d1", draftHash: "h1" });
  expect(result.current.continuationError).toMatch(/handoff failed/i);
  expect(result.current.error).toBeNull();
  expect(mocks.continueCreate).toHaveBeenCalledTimes(1);
});

test("expired recovery stays separate from continuation errors and retries", async () => {
  mocks.recovery.mockResolvedValueOnce({
    plan: null, job: null, draft: null, expired: true, failed: null,
  });
  mocks.continuePrepare.mockRejectedValueOnce(new Error("HISTORY_ACCESS_UNAVAILABLE: hidden"));
  const { result } = renderHook(() => useSessionImportUi());
  await waitFor(() => expect(result.current.restoreStatus).toBe("expired"));
  expect(result.current.continuationError).toBeNull();
  act(() => result.current.setReaderId("imp:one"));
  await act(async () => { await result.current.prepareContinue("imp:one"); });
  expect(result.current.continuationError).toBe("HISTORY_ACCESS_UNAVAILABLE: hidden");
  expect(result.current.error).toBe("PLAN_STALE: durable plan expired");
  mocks.continuePrepare.mockResolvedValueOnce({ draftId: "d1", text: "ready" });
  await act(async () => { await result.current.prepareContinue("imp:one"); });
  expect(result.current.continuationError).toBeNull();
  expect(result.current.draft).toMatchObject({ draftId: "d1" });
  expect(result.current.error).toBe("PLAN_STALE: durable plan expired");
});

test("confirmed prepared history uses the batch plan route and preserves its request on retry", async () => {
  const confirmation = { version: 1, importId: "imp:old", expectedRevision: "old-rev", previewId: "prepared-preview", reviewDigest: "a".repeat(64) };
  mocks.entries.mockResolvedValue({ items: [], total: 0, nextCursor: null });
  mocks.entry.mockResolvedValue({ importId: "imp:old", provider: "cursor", title: "Old", hidden: false, currentRevision: "old-rev" });
  mocks.associationInspect.mockResolvedValue({ batchId: "batch-1", stateVersion: 1, candidateId: "cursor-one", leafId: "leaf",
    review: { ...confirmation, sourceId: "registered", oldNativeSessionId: "old", currentNativeSessionId: "new", selectionHash: "selection" } });
  mocks.commit.mockRejectedValueOnce(new Error("response lost"));
  const { result } = renderHook(useSessionImportUi);
  act(() => result.current.setOpen(true));
  await act(async () => { await result.current.previewSelected([]); });
  await act(async () => { await result.current.finishPrepare(); });
  act(() => result.current.associations.open({ candidateId: "cursor-one", provider: "cursor", title: "New",
    sourceRevision: "rev", state: "prepared", attempt: 1, previewId: "prepared-preview" }));
  await waitFor(() => expect(result.current.associations.busy).toBe(false));
  await act(async () => { await result.current.associations.choose("imp:old"); });
  act(() => result.current.associations.markReady(confirmation.reviewDigest, true));
  act(() => { expect(result.current.associations.confirm()).toBe(true); });
  await act(async () => { await result.current.commit(); });
  expect(mocks.associationPlan).toHaveBeenCalledWith({ version: 1, batchId: "batch-1", expectedStateVersion: 1,
    mappings: [{ rootId: "unassigned", targetProjectId: null, unassigned: true, userChoiceVersion: 1 }], identityAssociations: [confirmation] });
  expect(mocks.preparePlan).not.toHaveBeenCalled();
  const request = mocks.commit.mock.calls[0][2];
  await act(async () => { await result.current.commit(); });
  expect(mocks.commit.mock.calls[1]).toEqual(["linked-plan", "linked-hash", request, false]);
  expect(mocks.associationPlan).toHaveBeenCalledOnce();
});

test("a recovered plan keeps its review summary and first request identity after a lost reply", async () => {
  const targets = [{ version: 1, importId: "imp:old", expectedRevision: "rev", previewId: "preview", reviewDigest: "a".repeat(64) }];
  mocks.recovery.mockResolvedValueOnce({ plan: { planId: "recovered", planHash: "hash", previewCount: 2, associationTargets: targets }, job: null, draft: null, expired: false, failed: null });
  mocks.commit.mockRejectedValueOnce(new Error("response lost"));
  const { result } = renderHook(useSessionImportUi);
  await waitFor(() => expect(result.current.pendingPlan?.planId).toBe("recovered"));
  expect(result.current.pendingPlan?.associationTargets).toEqual(targets);
  await act(async () => { await result.current.commit(); });
  expect(result.current.pendingPlan?.requestId).toBe(mocks.commit.mock.calls[0][2]);
  await act(async () => { await result.current.commit(); });
  expect(mocks.commit.mock.calls[1]).toEqual(mocks.commit.mock.calls[0]);
  expect(mocks.associationPlan).not.toHaveBeenCalled(); expect(mocks.preparePlan).not.toHaveBeenCalled();
});
