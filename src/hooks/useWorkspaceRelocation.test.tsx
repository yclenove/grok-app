/** @vitest-environment jsdom */
import { StrictMode } from "react";
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { SourceWorkspace } from "@/lib/api/sessionMigration";
import type { WorkspaceRelocationPreview } from "@/lib/api/workspaceRelocation";
import { readRelocationDraft, writeRelocationDraft } from "@/lib/sessionImport/relocationDraft";
import { useWorkspaceRelocation } from "./useWorkspaceRelocation";

const mocks = vi.hoisted(() => ({ get: vi.fn(), preview: vi.fn(), save: vi.fn(), file: vi.fn(), folder: vi.fn() }));
vi.mock("@/lib/api/sessionMigration", () => ({ sessionImportWorkspaceGet: mocks.get }));
vi.mock("@/lib/api/workspaceRelocation", () => ({ workspaceRelocationPreview: mocks.preview, workspaceRelocationSave: mocks.save }));
vi.mock("@/lib/api/fs", () => ({ pickAttachFiles: mocks.file, pickDirectory: mocks.folder }));

const workspace: SourceWorkspace = { sourceWorkspaceId: "ws:one", displayName: "one", workspaceFile: "/old.code-workspace", roots: [], evidence: [] };
const preview: WorkspaceRelocationPreview = {
  before: workspace, after: { ...workspace, workspaceFile: "/new.code-workspace" },
  request: { sourceWorkspaceId: workspace.sourceWorkspaceId, expectedRevision: "a".repeat(64), path: "/new.code-workspace" },
  fingerprint: "b".repeat(64), proposalHash: "c".repeat(64), retainedRootIds: [], addedRootIds: [], removedRoots: [],
};

beforeEach(() => {
  mocks.file.mockResolvedValue([preview.request.path]);
  mocks.folder.mockResolvedValue("/new");
  mocks.get.mockResolvedValue({ workspace, revision: preview.request.expectedRevision });
  mocks.preview.mockResolvedValue(preview);
  mocks.save.mockResolvedValue({ workspace: preview.after, revision: "d".repeat(64) });
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.resetAllMocks(); localStorage.clear(); });

function deferred<T>() {
  let resolve!: (result: T) => void;
  const promise = new Promise<T>((yes) => { resolve = yes; });
  return { resolve, promise };
}

test("explicit review uses the current revision and never saves until confirmed", async () => {
  const onSaved = vi.fn();
  const { result } = renderHook(() => useWorkspaceRelocation(workspace, onSaved), { wrapper: StrictMode });
  act(() => result.current.start());
  expect(mocks.get).not.toHaveBeenCalled();
  await act(() => result.current.review());
  expect(mocks.preview).toHaveBeenCalledWith(preview.request);
  expect(mocks.save).not.toHaveBeenCalled();
  await act(() => result.current.save());
  expect(mocks.save.mock.calls[0][0]).toMatchObject({ preview: preview.request, fingerprint: preview.fingerprint, proposalHash: preview.proposalHash });
  expect(onSaved).toHaveBeenCalledWith(preview.after);
  expect(result.current.open).toBe(false);
  expect(readRelocationDraft(workspace.sourceWorkspaceId)).toBeNull();
});

test("cancelled file or folder pickers perform no preview or save", async () => {
  mocks.file.mockResolvedValue([]); mocks.folder.mockResolvedValue(null);
  const file = renderHook(() => useWorkspaceRelocation(workspace, vi.fn()));
  await act(() => file.result.current.review());
  const folder = renderHook(() => useWorkspaceRelocation({ ...workspace, workspaceFile: undefined }, vi.fn()));
  await act(() => folder.result.current.review());
  expect(mocks.preview).not.toHaveBeenCalled(); expect(mocks.save).not.toHaveBeenCalled();
  expect(file.result.current.busy).toBe(false); expect(folder.result.current.busy).toBe(false);
});

test("multi-file selection is rejected before loading metadata", async () => {
  mocks.file.mockResolvedValue(["/one", "/two"]);
  const { result } = renderHook(() => useWorkspaceRelocation(workspace, vi.fn()));
  await act(() => result.current.review());
  expect(result.current.error).toContain("FORMAT_UNSUPPORTED");
  expect(mocks.get).not.toHaveBeenCalled(); expect(mocks.save).not.toHaveBeenCalled();
});

test("closing or unmounting invalidates delayed pickers and previews", async () => {
  const picked = deferred<string[]>(); mocks.file.mockReturnValue(picked.promise);
  const { result, unmount } = renderHook(() => useWorkspaceRelocation(workspace, vi.fn()));
  act(() => result.current.start());
  let task!: Promise<void>;
  act(() => { task = result.current.review(); });
  act(() => result.current.close());
  await act(async () => { picked.resolve([preview.request.path]); await task; });
  expect(mocks.get).not.toHaveBeenCalled(); expect(result.current.preview).toBeNull();
  const pending = deferred<WorkspaceRelocationPreview>(); mocks.preview.mockReturnValue(pending.promise);
  act(() => result.current.start());
  act(() => { task = result.current.review(preview.request.path); });
  await act(async () => { await Promise.resolve(); });
  unmount();
  await act(async () => { pending.resolve(preview); await task; });
  expect(mocks.save).not.toHaveBeenCalled();
});

test("double clicks dispatch once and ambiguous failures freeze the exact request across remount", async () => {
  const response = deferred<never>();
  mocks.save.mockRejectedValueOnce("transport lost").mockReturnValueOnce(response.promise);
  const onSaved = vi.fn();
  const first = renderHook(() => useWorkspaceRelocation(workspace, onSaved));
  await act(() => first.result.current.review());
  await act(() => first.result.current.save());
  const request = mocks.save.mock.calls[0][0];
  expect(readRelocationDraft(workspace.sourceWorkspaceId)).toEqual(request);
  first.unmount();
  const second = renderHook(() => useWorkspaceRelocation(workspace, onSaved));
  act(() => second.result.current.start());
  expect(second.result.current.attempt).toEqual(request);
  await act(() => second.result.current.review("/another"));
  expect(mocks.preview).toHaveBeenCalledTimes(1);
  act(() => { void second.result.current.save(); void second.result.current.save(); });
  expect(mocks.save).toHaveBeenCalledTimes(2);
  expect(mocks.save.mock.calls[1][0]).toEqual(request);
  act(() => second.result.current.close());
  expect(second.result.current.open).toBe(true);
});

test("definitive source change unlocks a fresh preview; a busy store keeps its retry", async () => {
  mocks.save.mockRejectedValueOnce("SOURCE_BUSY: database is locked").mockRejectedValueOnce("SOURCE_CHANGED: preview again");
  const { result } = renderHook(() => useWorkspaceRelocation(workspace, vi.fn()));
  await act(() => result.current.review());
  await act(() => result.current.save());
  expect(result.current.attempt).not.toBeNull();
  await act(() => result.current.save());
  expect(result.current.attempt).toBeNull(); expect(result.current.preview).toBeNull();
  expect(readRelocationDraft(workspace.sourceWorkspaceId)).toBeNull();
  await act(() => result.current.review());
  expect(mocks.preview).toHaveBeenCalledTimes(2);
});

test("storage failure does not dispatch and corrupted pending data never auto-saves", async () => {
  const { result } = renderHook(() => useWorkspaceRelocation(workspace, vi.fn()));
  await act(() => result.current.review());
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => { throw new Error("quota"); });
  await act(() => result.current.save());
  expect(mocks.save).not.toHaveBeenCalled();
  expect(result.current.error).toContain("quota");
});

test("an old save reply does not navigate after the workspace view has been left", async () => {
  const pending = deferred<{ workspace: SourceWorkspace; revision: string }>();
  mocks.save.mockReturnValue(pending.promise);
  const onSaved = vi.fn();
  const { result, unmount } = renderHook(() => useWorkspaceRelocation(workspace, onSaved));
  await act(() => result.current.review());
  let task!: Promise<void>;
  act(() => { task = result.current.save(); });
  const newerRequest = { ...mocks.save.mock.calls[0][0], requestId: "another-window" };
  writeRelocationDraft(newerRequest);
  unmount();
  await act(async () => { pending.resolve({ workspace: preview.after, revision: "ok" }); await task; });
  expect(onSaved).not.toHaveBeenCalled();
  expect(readRelocationDraft(workspace.sourceWorkspaceId)).toEqual(newerRequest);
});

test("recovered requests can return newer metadata without replacing it with the old proposal", async () => {
  const request = { requestId: "recover", preview: preview.request, fingerprint: preview.fingerprint, proposalHash: preview.proposalHash };
  writeRelocationDraft(request);
  const current = { ...preview.after, displayName: "newer edit" };
  mocks.save.mockResolvedValue({ workspace: current, revision: "current" });
  const onSaved = vi.fn();
  const { result } = renderHook(() => useWorkspaceRelocation(workspace, onSaved));
  act(() => result.current.start());
  await act(() => result.current.save());
  expect(mocks.preview).not.toHaveBeenCalled(); expect(mocks.get).not.toHaveBeenCalled();
  expect(onSaved).toHaveBeenCalledWith(current);
});
