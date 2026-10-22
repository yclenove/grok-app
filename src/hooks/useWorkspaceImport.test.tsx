/** @vitest-environment jsdom */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { SavedWorkspace, SourceWorkspace, WorkspaceImportPreview } from "@/lib/api/sessionMigration";
import { useWorkspaceImport } from "./useWorkspaceImport";

const mocks = vi.hoisted(() => ({ preview: vi.fn(), save: vi.fn(), list: vi.fn(), get: vi.fn(), update: vi.fn(), pick: vi.fn() }));
vi.mock("@/lib/api/sessionMigration", () => ({
  sessionImportWorkspacePreview: mocks.preview,
  sessionImportWorkspaceSave: mocks.save,
  sessionImportWorkspaces: mocks.list,
  sessionImportWorkspaceGet: mocks.get,
  sessionImportWorkspaceUpdate: mocks.update,
}));
vi.mock("@/lib/api/fs", () => ({ pickDirectory: mocks.pick }));
afterEach(() => { cleanup(); vi.resetAllMocks(); });

function fixture(roots = true): WorkspaceImportPreview {
  return {
    source: { kind: "file", path: "/test.code-workspace" },
    fingerprint: "fingerprint-1", savedRevision: null,
    workspace: {
      sourceWorkspaceId: "ws:one", displayName: "one", evidence: [],
      roots: roots ? ["a", "b"].map((id) => ({
        rootId: id, originalPath: `/project/${id}`, pathSyntax: "posix" as const,
        host: "local" as const, availability: "missing" as const, evidence: [],
      })) : [],
    },
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

test("source selection reloads scoped mappings only after a successful preview and saves the chosen identity", async () => {
  const first = fixture();
  const second: WorkspaceImportPreview = { ...first, source: { ...first.source, sourceId: "source-b" },
    workspace: { ...first.workspace, sourceWorkspaceId: "ws:other", sourceIdentity: { sourceId: "source-b", provider: "codex", locator: "file:/test.code-workspace" } } };
  mocks.preview.mockResolvedValueOnce(first).mockRejectedValueOnce("MAPPING_CONFLICT")
    .mockResolvedValueOnce(second);
  mocks.save.mockResolvedValue(second.workspace);
  const { result } = renderHook(useWorkspaceImport);
  await act(() => result.current.load(first.source));
  act(() => result.current.setProject("a", "draft-project"));
  await act(() => result.current.chooseSource("source-b"));
  expect(result.current.workspace?.sourceWorkspaceId).toBe("ws:one");
  expect(result.current.choices[0].targetProjectId).toBe("draft-project");
  expect(result.current.sourceId).toBeNull();
  await act(() => result.current.chooseSource("source-b"));
  expect(result.current.workspace?.sourceWorkspaceId).toBe("ws:other");
  expect(result.current.choices[0].targetProjectId).toBeNull();
  await act(() => result.current.save());
  expect(mocks.save.mock.calls[0][0].source.sourceId).toBe("source-b");
});

test("legacy source adoption is an editable draft, survives save failure, and assigned sources cannot be changed", async () => {
  const old = fixture().workspace;
  mocks.get.mockResolvedValue({ workspace: old, revision: "old" });
  const adopted = { ...old, sourceIdentity: { sourceId: "source-a", provider: "codex" as const, locator: "file:/test.code-workspace" } };
  mocks.update.mockRejectedValueOnce("PLAN_STALE").mockResolvedValueOnce({ workspace: adopted, revision: "new" });
  const { result } = renderHook(useWorkspaceImport);
  act(() => result.current.showSaved(old));
  await act(() => result.current.edit());
  await act(() => result.current.chooseSource("source-a"));
  expect(mocks.update).not.toHaveBeenCalled();
  await act(() => result.current.save());
  expect(result.current.sourceId).toBe("source-a");
  expect(result.current.mode).toBe("edit");
  await act(() => result.current.save());
  expect(mocks.update.mock.calls[1][0]).toMatchObject({ sourceId: "source-a", expectedRevision: "old" });
  mocks.get.mockResolvedValue({ workspace: adopted, revision: "new" });
  await act(() => result.current.edit());
  await act(() => result.current.chooseSource("source-b"));
  expect(result.current.sourceId).toBe("source-a");
});

test("source preview cannot write after Back, or duplicate an in-flight request", async () => {
  const pending = deferred<WorkspaceImportPreview>();
  mocks.preview.mockResolvedValueOnce(fixture()).mockReturnValueOnce(pending.promise);
  const { result } = renderHook(useWorkspaceImport);
  await act(() => result.current.load(fixture().source));
  let task!: Promise<void>;
  act(() => { task = result.current.chooseSource("source-b"); void result.current.chooseSource("source-b"); });
  expect(mocks.preview).toHaveBeenCalledTimes(2);
  act(() => result.current.reset());
  await act(async () => { pending.resolve(fixture()); await task; });
  expect(result.current.mode).toBeNull();
  expect(result.current.workspace).toBeNull();
  expect(result.current.busy).toBe(false);
});

test("preview is read-only; explicit save sends all choices including primary", async () => {
  const data = fixture();
  mocks.preview.mockResolvedValue(data);
  mocks.save.mockImplementation(async (request) => ({ ...data.workspace, mappingChoices: request.choices }));
  const { result } = renderHook(useWorkspaceImport);
  await act(() => result.current.load(data.source));
  expect(mocks.save).not.toHaveBeenCalled();
  act(() => { result.current.setProject("b", "p1"); result.current.setPrimary("b"); });
  await act(() => result.current.save());
  expect(mocks.save).toHaveBeenCalledWith({
    source: data.source, expectedFingerprint: "fingerprint-1", expectedSavedRevision: null,
    choices: [
      { rootId: "a", targetProjectId: null, primary: false },
      { rootId: "b", targetProjectId: "p1", primary: true },
    ],
  });
  expect(result.current.mode).toBe("saved");
});

test("empty workspace can save without adding a parent root or fake session", async () => {
  const data = fixture(false);
  mocks.preview.mockResolvedValue(data);
  mocks.save.mockResolvedValue(data.workspace);
  const { result } = renderHook(useWorkspaceImport);
  await act(() => result.current.load(data.source));
  await act(() => result.current.save());
  expect(mocks.save.mock.calls[0][0].choices).toEqual([]);
  expect(result.current.workspace?.roots).toEqual([]);
});

test("reset drops a late preview and its error without reopening the panel", async () => {
  const pending = deferred<WorkspaceImportPreview>();
  mocks.preview.mockReturnValue(pending.promise);
  const { result } = renderHook(useWorkspaceImport);
  let loading!: Promise<void>;
  act(() => { loading = result.current.load(fixture().source); });
  act(() => result.current.reset());
  await act(async () => { pending.reject(new Error("late error")); await loading; });
  expect(result.current.mode).toBeNull();
  expect(result.current.error).toBeNull();
  expect(result.current.busy).toBe(false);
});

test("replacing source ignores an earlier successful preview", async () => {
  const pending = deferred<WorkspaceImportPreview>();
  const next = fixture(false);
  mocks.preview.mockReturnValueOnce(pending.promise).mockResolvedValueOnce(next);
  const { result } = renderHook(useWorkspaceImport);
  let loading!: Promise<void>;
  act(() => { loading = result.current.load(fixture().source); });
  await act(() => result.current.load(next.source));
  await act(async () => { pending.resolve(fixture()); await loading; });
  expect(result.current.workspace?.roots).toEqual([]);
});

test("failed save keeps choices for retry; duplicate clicks have one in-flight save", async () => {
  mocks.preview.mockResolvedValue(fixture());
  const pending = deferred<SourceWorkspace>();
  mocks.save.mockReturnValue(pending.promise);
  const { result } = renderHook(useWorkspaceImport);
  await act(() => result.current.load(fixture().source));
  act(() => result.current.setPrimary("b"));
  let saving!: Promise<void>;
  act(() => { saving = result.current.save(); void result.current.save(); });
  expect(mocks.save).toHaveBeenCalledTimes(1);
  await act(async () => { pending.reject("SOURCE_CHANGED"); await saving; });
  expect(result.current.mode).toBe("preview");
  expect(result.current.choices[1].primary).toBe(true);
  expect(result.current.error).toBe("SOURCE_CHANGED");
});

test("late confirmed save cannot overwrite a replacement workspace view", async () => {
  mocks.preview.mockResolvedValue(fixture());
  const pending = deferred<SourceWorkspace>();
  mocks.save.mockReturnValue(pending.promise);
  const { result } = renderHook(useWorkspaceImport);
  await act(() => result.current.load(fixture().source));
  let saving!: Promise<void>;
  act(() => { saving = result.current.save(); });
  act(() => result.current.showSaved({ ...fixture(false).workspace, sourceWorkspaceId: "ws:other" }));
  await act(async () => { pending.resolve(fixture().workspace); await saving; });
  expect(result.current.workspace?.sourceWorkspaceId).toBe("ws:other");
});

test("saved metadata reopens without accessing an unavailable source", async () => {
  const data = fixture().workspace;
  mocks.list.mockResolvedValue({ items: [data], nextCursor: null });
  const { result } = renderHook(useWorkspaceImport);
  await act(() => result.current.list());
  act(() => result.current.showSaved(result.current.items[0]));
  expect(result.current.mode).toBe("saved");
  expect(result.current.workspace).toEqual(data);
  expect(mocks.preview).not.toHaveBeenCalled();
  await act(() => result.current.save());
  expect(mocks.save).not.toHaveBeenCalled();
});

test("catalog pagination preserves earlier rows on failure and retries same cursor", async () => {
  mocks.list.mockResolvedValueOnce({ items: [fixture().workspace], nextCursor: "ws:one" })
    .mockRejectedValueOnce("catalog locked")
    .mockResolvedValueOnce({ items: [{ ...fixture().workspace, sourceWorkspaceId: "ws:two" }], nextCursor: null });
  const { result } = renderHook(useWorkspaceImport);
  await act(() => result.current.list());
  await act(() => result.current.list(true));
  expect(result.current.items).toHaveLength(1);
  expect(result.current.cursor).toBe("ws:one");
  await act(() => result.current.list(true));
  expect(mocks.list.mock.calls.map(([cursor]) => cursor)).toEqual([null, "ws:one", "ws:one"]);
  expect(result.current.items).toHaveLength(2);
  expect(result.current.error).toBeNull();
});

test("saved mappings edit offline with a revision and preserve source paths", async () => {
  const data = fixture().workspace;
  mocks.get.mockResolvedValue({ workspace: data, revision: "r1" });
  mocks.pick.mockResolvedValue("/relocated folder");
  mocks.update.mockImplementation(async (request) => ({ workspace: { ...data, mappingChoices: request.choices }, revision: "r2" }));
  const { result } = renderHook(useWorkspaceImport);
  act(() => result.current.showSaved(data));
  await act(() => result.current.edit());
  await act(() => result.current.chooseTarget("a"));
  act(() => result.current.setPrimary("b"));
  await act(() => result.current.save());
  expect(mocks.update).toHaveBeenCalledWith({ sourceWorkspaceId: data.sourceWorkspaceId, expectedRevision: "r1", choices: [
    { rootId: "a", targetProjectId: null, explicitTarget: "/relocated folder", primary: false },
    { rootId: "b", targetProjectId: null, primary: true },
  ] });
  expect(result.current.workspace?.roots).toEqual(data.roots);
  expect(result.current.mode).toBe("saved");
  expect(mocks.preview).not.toHaveBeenCalled();
  expect(mocks.save).not.toHaveBeenCalled();
});

test("edit conflict retains draft until explicit refresh obtains a new revision", async () => {
  const data = fixture().workspace;
  mocks.get.mockResolvedValueOnce({ workspace: data, revision: "r1" }).mockResolvedValueOnce({ workspace: data, revision: "r2" });
  mocks.update.mockRejectedValueOnce("PLAN_STALE").mockResolvedValueOnce({ workspace: data, revision: "r2" });
  const { result } = renderHook(useWorkspaceImport);
  act(() => result.current.showSaved(data));
  await act(() => result.current.edit());
  act(() => result.current.setProject("a", "p1"));
  await act(() => result.current.save());
  expect(result.current.choices[0].targetProjectId).toBe("p1");
  expect(result.current.error).toBe("PLAN_STALE");
  expect(result.current.mode).toBe("edit");
  await act(() => result.current.edit());
  expect(result.current.choices[0].targetProjectId).toBeNull();
  await act(() => result.current.save());
  expect(mocks.update.mock.calls.map(([request]) => request.expectedRevision)).toEqual(["r1", "r2"]);
});

test("cancel edit restores the last saved snapshot without persisting changes", async () => {
  const data = fixture().workspace;
  mocks.get.mockResolvedValue({ workspace: data, revision: "r1" });
  const { result } = renderHook(useWorkspaceImport);
  act(() => result.current.showSaved(data));
  await act(() => result.current.edit());
  act(() => result.current.setPrimary("b"));
  act(() => result.current.cancelEdit());
  expect(result.current.mode).toBe("saved");
  expect(result.current.choices[0].primary).toBe(true);
  expect(mocks.update).not.toHaveBeenCalled();
});

test("failed edit load blocks mutations and save until refresh succeeds", async () => {
  const data = fixture().workspace;
  mocks.get.mockRejectedValueOnce("catalog locked").mockResolvedValueOnce({ workspace: data, revision: "r1" });
  const { result } = renderHook(useWorkspaceImport);
  act(() => result.current.showSaved(data));
  await act(() => result.current.edit());
  act(() => result.current.setProject("a", "p1"));
  await act(() => result.current.chooseTarget("a"));
  await act(() => result.current.save());
  expect(result.current.editable).toBe(false);
  expect(mocks.pick).not.toHaveBeenCalled();
  expect(mocks.update).not.toHaveBeenCalled();
  await act(() => result.current.edit());
  expect(result.current.editable).toBe(true);
  expect(result.current.error).toBeNull();
});

test("picker cancellation and failure preserve mapping; choosing a project clears relocation", async () => {
  const data = fixture().workspace;
  mocks.get.mockResolvedValue({ workspace: data, revision: "r1" });
  mocks.pick.mockResolvedValueOnce("/target").mockResolvedValueOnce(null).mockRejectedValueOnce("picker failed");
  const { result } = renderHook(useWorkspaceImport);
  act(() => result.current.showSaved(data));
  await act(() => result.current.edit());
  await act(async () => expect(await result.current.chooseTarget("a")).toBe(true));
  await act(async () => expect(await result.current.chooseTarget("a")).toBe(true));
  await act(async () => expect(await result.current.chooseTarget("a")).toBe(true));
  expect(result.current.choices[0].explicitTarget).toBe("/target");
  expect(result.current.error).toBe("picker failed");
  expect(result.current.busy).toBe(false);
  act(() => result.current.setProject("a", "p1"));
  expect(result.current.choices[0].explicitTarget).toBeUndefined();
  expect(result.current.choices[0].targetProjectId).toBe("p1");
});

test.each(["get", "pick", "update"] as const)("late %s cannot overwrite a replacement workspace", async (operation) => {
  const data = fixture().workspace;
  const pending = deferred<SavedWorkspace | string>();
  mocks.get.mockResolvedValue({ workspace: data, revision: "r1" });
  mocks[operation].mockReturnValueOnce(pending.promise);
  const { result } = renderHook(useWorkspaceImport);
  act(() => result.current.showSaved(data));
  let waiting!: Promise<void | boolean>;
  if (operation === "get") act(() => { waiting = result.current.edit(); });
  else {
    await act(() => result.current.edit());
    act(() => { waiting = operation === "pick" ? result.current.chooseTarget("a") : result.current.save(); });
  }
  act(() => result.current.showSaved({ ...fixture(false).workspace, sourceWorkspaceId: "ws:replacement" }));
  await act(async () => {
    pending.resolve(operation === "pick" ? "/late" : { workspace: data, revision: "r2" });
    const outcome = await waiting;
    if (operation === "pick") expect(outcome).toBe(false);
  });
  expect(result.current.workspace?.sourceWorkspaceId).toBe("ws:replacement");
  expect(result.current.choices).toEqual([]);
  expect(result.current.busy).toBe(false);
  expect(result.current.error).toBeNull();
});

test("edit save guards duplicate clicks, draft changes and cancellation while persisting", async () => {
  const data = fixture().workspace;
  const pending = deferred<SavedWorkspace>();
  mocks.get.mockResolvedValue({ workspace: data, revision: "r1" });
  mocks.update.mockReturnValue(pending.promise);
  const { result } = renderHook(useWorkspaceImport);
  act(() => result.current.showSaved(data));
  await act(() => result.current.edit());
  let saving!: Promise<void>;
  act(() => { saving = result.current.save(); void result.current.save(); result.current.cancelEdit(); result.current.setPrimary("b"); });
  expect(mocks.update).toHaveBeenCalledTimes(1);
  expect(result.current.mode).toBe("edit");
  expect(result.current.choices[0].primary).toBe(true);
  await act(async () => { pending.resolve({ workspace: data, revision: "r2" }); await saving; });
  expect(result.current.mode).toBe("saved");
});
