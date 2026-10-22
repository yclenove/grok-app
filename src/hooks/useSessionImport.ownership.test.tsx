/** @vitest-environment jsdom */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { HandoffDraft } from "@/lib/api/sessionMigration";
import { openImportedHistory, openSessionImport } from "@/lib/sessionImport/open";
import { defaultSelection } from "@/lib/sessionImport/selection";
import { useSessionImportUi } from "./useSessionImport";

const EDITED = "交接🙂\n第二行";
const IMP = "imp:00000000-0000-4000-8000-000000000153";
const mocks = vi.hoisted(() => ({
  entries: vi.fn(),
  recovery: vi.fn(),
  continuePrepare: vi.fn(),
  workspace: vi.fn(),
  selection: vi.fn(),
  create: vi.fn(),
  restore: vi.fn(),
  discard: vi.fn(),
  listRoots: vi.fn(),
  sources: vi.fn(),
  projectsList: vi.fn(),
  listModels: vi.fn(),
}));
vi.mock("@/lib/api", () => ({
  sessionImportEntries: mocks.entries,
  sessionImportRecovery: mocks.recovery,
  sessionImportContinuePrepare: mocks.continuePrepare,
  sessionImportContinueWorkspace: mocks.workspace,
  sessionImportContinueSelection: mocks.selection,
  sessionImportContinueCreate: mocks.create,
  sessionImportContinueRestore: mocks.restore,
  sessionImportContinueDiscard: mocks.discard,
  sessionImportListRoots: mocks.listRoots,
  sessionImportSources: mocks.sources,
  projectsList: mocks.projectsList,
  listModels: mocks.listModels,
}));
vi.mock("@/lib/api/workspace", () => ({ workspacesList: vi.fn().mockResolvedValue([]) }));

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (cause: unknown) => void;
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  promise.catch(() => undefined);
  return { promise, resolve, reject };
}
function draftFor(projectId: string | null, leaf: string, text: string): HandoffDraft {
  return {
    draftId: `d:${projectId ?? "none"}:${leaf}`, draftHash: `h:${projectId ?? "none"}`, importId: IMP,
    revisionId: "rev-1", leaf, text, coverage: "full", estimatedTokens: 4, runtimeCapability: "fragment-only",
    creationVersion: 1, workspaceProjectConstraint: projectId, projectId,
    selection: { ...defaultSelection(), eventIds: ["message-205"] },
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.continuePrepare.mockReset();
  mocks.recovery.mockReset();
  mocks.create.mockReset();
  mocks.restore.mockReset();
  mocks.recovery.mockResolvedValue({ draft: null, expired: false });
  mocks.entries.mockResolvedValue({ items: [] });
  mocks.listRoots.mockResolvedValue([]);
  mocks.sources.mockResolvedValue([]);
  mocks.projectsList.mockResolvedValue([]);
  mocks.discard.mockResolvedValue(undefined);
  mocks.restore.mockRejectedValue(new Error("offline"));
  mocks.create.mockResolvedValue({ appSessionId: "native-session" });
  window.history.replaceState(null, "", "/");
});
afterEach(() => {
  expect(mocks.listModels).not.toHaveBeenCalled();
  cleanup();
  window.history.replaceState(null, "", "/");
});

async function openReader(result: { current: ReturnType<typeof useSessionImportUi> }) {
  await act(async () => openImportedHistory({ importId: IMP, revision: "rev-1", leaf: "viewed-leaf" }));
  expect(result.current.readerId).toBe(IMP);
  expect(result.current.readerLeaf).toBe("viewed-leaf");
  expect(result.current.readerRevision).toBe("rev-1");
}

test("openSessionImport moves a pending prepare to the new project without touching the reader", async () => {
  const { result } = renderHook(() => useSessionImportUi());
  await openReader(result);
  await act(async () => openSessionImport({ projectId: "proj-a" }));
  const pendingA = deferred<HandoffDraft>();
  const pendingB = deferred<HandoffDraft>();
  mocks.continuePrepare.mockReturnValueOnce(pendingA.promise).mockReturnValueOnce(pendingB.promise);
  let first!: Promise<void>;
  act(() => { first = result.current.prepareContinue(IMP, "anchor-a", "rev-1"); });
  expect(mocks.continuePrepare).toHaveBeenCalledWith(IMP, { projectId: "proj-a", leaf: "anchor-a", revision: "rev-1" });
  await act(async () => openSessionImport({ projectId: "proj-b" }));
  expect(result.current.projectId).toBe("proj-b");
  expect(result.current.readerId).toBe(IMP);
  expect(result.current.readerLeaf).toBe("viewed-leaf");
  expect(result.current.readerRevision).toBe("rev-1");
  let second!: Promise<void>;
  act(() => { second = result.current.prepareContinue(IMP, "anchor-b", "rev-1"); });
  expect(mocks.continuePrepare).toHaveBeenCalledTimes(2);
  expect(mocks.continuePrepare).toHaveBeenLastCalledWith(IMP, { projectId: "proj-b", leaf: "anchor-b", revision: "rev-1" });
  expect(result.current.continuationBusy).toBe(true);
  await act(async () => { pendingA.resolve(draftFor("proj-a", "anchor-a", "A迟到🙂")); await first; });
  expect(result.current.draft).toBeNull();
  expect(result.current.continuationError).toBeNull();
  expect(result.current.continuationVisible).toBe(false);
  expect(result.current.continuationBusy).toBe(true);
  expect(mocks.create).not.toHaveBeenCalled();
  await act(async () => { pendingB.resolve(draftFor("proj-b", "anchor-b", "B正文🙂")); await second; });
  expect(result.current.draft).toMatchObject({ text: "B正文🙂", leaf: "anchor-b" });
  expect(result.current.readerId).toBe(IMP);
  act(() => result.current.setDraft({ ...result.current.draft!, text: EDITED }));
  act(() => result.current.hideContinuation());
  await act(async () => result.current.prepareContinue(IMP, "anchor-b", "rev-1"));
  expect(mocks.continuePrepare).toHaveBeenCalledTimes(2);
  expect(result.current.draft?.text).toBe(EDITED);
  expect(result.current.continuationVisible).toBe(true);
  expect(mocks.discard).not.toHaveBeenCalled();
});

test("a rejected prepare from the previous project does not surface on the new one", async () => {
  const { result } = renderHook(() => useSessionImportUi());
  await openReader(result);
  await act(async () => openSessionImport({ projectId: "proj-a" }));
  const pendingA = deferred<HandoffDraft>();
  const pendingB = deferred<HandoffDraft>();
  mocks.continuePrepare.mockReturnValueOnce(pendingA.promise).mockReturnValueOnce(pendingB.promise);
  let first!: Promise<void>;
  act(() => { first = result.current.prepareContinue(IMP, "anchor-a", "rev-1"); });
  await act(async () => openSessionImport({ projectId: "proj-b" }));
  let second!: Promise<void>;
  act(() => { second = result.current.prepareContinue(IMP, "anchor-b", "rev-1"); });
  await act(async () => { pendingA.reject(new Error("A失败")); await first; });
  expect(result.current.continuationError).toBeNull();
  expect(result.current.continuationBusy).toBe(true);
  expect(result.current.readerLeaf).toBe("viewed-leaf");
  await act(async () => { pendingB.resolve(draftFor("proj-b", "anchor-b", "B正文🙂")); await second; });
  expect(result.current.draft?.text).toBe("B正文🙂");
  expect(result.current.continuationError).toBeNull();
  expect(mocks.create).not.toHaveBeenCalled();
});

test("A to B to A does not adopt either superseded prepare", async () => {
  const { result } = renderHook(() => useSessionImportUi());
  await openReader(result);
  await act(async () => openSessionImport({ projectId: "proj-a" }));
  const pendingA = deferred<HandoffDraft>();
  const pendingB = deferred<HandoffDraft>();
  mocks.continuePrepare.mockReturnValueOnce(pendingA.promise).mockReturnValueOnce(pendingB.promise);
  let first!: Promise<void>;
  act(() => { first = result.current.prepareContinue(IMP, "anchor-a", "rev-1"); });
  await act(async () => openSessionImport({ projectId: "proj-b" }));
  let second!: Promise<void>;
  act(() => { second = result.current.prepareContinue(IMP, "anchor-b", "rev-1"); });
  await act(async () => openSessionImport({ projectId: "proj-a" }));
  expect(result.current.readerId).toBe(IMP);
  await act(async () => { pendingA.resolve(draftFor("proj-a", "anchor-a", "A迟到🙂")); await first; });
  await act(async () => { pendingB.resolve(draftFor("proj-b", "anchor-b", "B迟到🙂")); await second; });
  expect(result.current.draft).toBeNull();
  expect(result.current.continuationVisible).toBe(false);
  mocks.continuePrepare.mockResolvedValueOnce(draftFor("proj-a", "anchor-a", "A新正文"));
  await act(async () => result.current.prepareContinue(IMP, "anchor-a", "rev-1"));
  expect(mocks.continuePrepare).toHaveBeenCalledTimes(3);
  expect(result.current.draft?.text).toBe("A新正文");
});

test("a null project and a concrete project do not take each other's pending prepare", async () => {
  const { result } = renderHook(() => useSessionImportUi());
  await openReader(result);
  const pendingNull = deferred<HandoffDraft>();
  const pendingProject = deferred<HandoffDraft>();
  mocks.continuePrepare.mockReturnValueOnce(pendingNull.promise).mockReturnValueOnce(pendingProject.promise);
  let first!: Promise<void>;
  act(() => { first = result.current.prepareContinue(IMP, null, null); });
  expect(mocks.continuePrepare).toHaveBeenCalledWith(IMP, { projectId: null, leaf: null, revision: null });
  await act(async () => openSessionImport({ projectId: "proj-a" }));
  let second!: Promise<void>;
  act(() => { second = result.current.prepareContinue(IMP, "anchor-a", "rev-1"); });
  expect(mocks.continuePrepare).toHaveBeenLastCalledWith(IMP, { projectId: "proj-a", leaf: "anchor-a", revision: "rev-1" });
  await act(async () => { pendingNull.resolve(draftFor(null, "auth-leaf", "空项目迟到")); await first; });
  expect(result.current.draft).toBeNull();
  expect(result.current.continuationBusy).toBe(true);
  await act(async () => openSessionImport({ projectId: null }));
  await act(async () => { pendingProject.resolve(draftFor("proj-a", "anchor-a", "项目迟到")); await second; });
  expect(result.current.draft).toBeNull();
  expect(result.current.readerId).toBe(IMP);
});

test("closing the reader rejects a late prepare", async () => {
  const { result } = renderHook(() => useSessionImportUi());
  await openReader(result);
  await act(async () => openSessionImport({ projectId: "proj-a" }));
  const pending = deferred<HandoffDraft>();
  mocks.continuePrepare.mockReturnValueOnce(pending.promise);
  let task!: Promise<void>;
  act(() => { task = result.current.prepareContinue(IMP, "anchor-a", "rev-1"); });
  act(() => result.current.setReaderId(null));
  expect(result.current.readerId).toBeNull();
  await act(async () => { pending.resolve(draftFor("proj-a", "anchor-a", "关读者后")); await task; });
  expect(result.current.draft).toBeNull();
  expect(result.current.continuationVisible).toBe(false);
  expect(mocks.continuePrepare).toHaveBeenCalledTimes(1);
  expect(mocks.create).not.toHaveBeenCalled();
});

test("startup recovery with an explicit null constraint stays off the active wizard", async () => {
  const recovered: HandoffDraft = {
    ...draftFor(null, "auth-leaf", "空约束🙂"),
    draftId: "rec-1", draftHash: "rec-h", revisionId: "auth-rev", leaf: "auth-leaf",
    creationRequestId: "durable-null", workspaceProjectConstraint: null, projectId: "proj-a",
  };
  const gate = deferred<{ draft: HandoffDraft | null; expired: boolean }>();
  mocks.recovery.mockReturnValueOnce(gate.promise);
  const { result } = renderHook(() => useSessionImportUi());
  await act(async () => openImportedHistory({ importId: IMP, revision: "auth-rev", leaf: "viewed-leaf" }));
  await act(async () => openSessionImport({ projectId: "proj-a" }));
  await act(async () => { gate.resolve({ draft: recovered, expired: false }); });
  expect(result.current.restoreStatus).toBe("draft");
  expect(result.current.draft?.text).toBe("空约束🙂");
  expect(result.current.continuationVisible).toBe(false);
  expect(result.current.continuationLocked).toBe(true);
  expect(result.current.projectId).toBe("proj-a");
  expect(result.current.readerLeaf).toBe("viewed-leaf");
  await act(async () => result.current.prepareContinue(IMP, "auth-leaf", "auth-rev"));
  expect(mocks.continuePrepare).not.toHaveBeenCalled();
  expect(result.current.continuationVisible).toBe(false);
  await act(async () => openSessionImport({ projectId: null }));
  await act(async () => result.current.prepareContinue(IMP, "auth-leaf", "auth-rev"));
  expect(mocks.continuePrepare).not.toHaveBeenCalled();
  expect(result.current.continuationVisible).toBe(true);
  expect(result.current.draft?.text).toBe("空约束🙂");
  expect(mocks.discard).not.toHaveBeenCalled();
});
