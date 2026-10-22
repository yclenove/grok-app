/** @vitest-environment jsdom */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { HandoffDraft } from "@/lib/api/sessionMigration";
import { defaultSelection } from "@/lib/sessionImport/selection";
import { useImportContinuation } from "./useImportContinuation";

const EDITED = "交接🙂\n第二行";
const api = vi.hoisted(() => ({
  sessionImportContinuePrepare: vi.fn(),
  sessionImportContinueWorkspace: vi.fn(),
  sessionImportContinueSelection: vi.fn(),
  sessionImportContinueCreate: vi.fn(),
  sessionImportContinueRestore: vi.fn(),
  sessionImportContinueDiscard: vi.fn(),
  listModels: vi.fn(),
}));
vi.mock("@/lib/api", () => api);
const activity = vi.fn();
const base: HandoffDraft = {
  draftId: "d1", draftHash: "h1", importId: "imp:1", revisionId: "rev-1", leaf: "resolved-leaf",
  text: "原始", coverage: "full", estimatedTokens: 4, runtimeCapability: "fragment-only",
  selection: { ...defaultSelection(), eventIds: ["message-205", "message-206"] },
};
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}
function fresh(text: string, leaf = "message-205"): HandoffDraft {
  return { ...base, text, leaf, draftId: `d:${leaf}:${text}` };
}
beforeEach(() => {
  vi.resetAllMocks();
  activity.mockReset();
  api.sessionImportContinuePrepare.mockImplementation(async (_importId: string, opts: { leaf?: string | null }) => fresh(`原始:${opts.leaf ?? ""}`, opts.leaf ?? ""));
  api.sessionImportContinueDiscard.mockResolvedValue(undefined);
  api.sessionImportContinueRestore.mockRejectedValue(new Error("offline"));
  api.sessionImportContinueCreate.mockResolvedValue({ appSessionId: "native" });
});
afterEach(() => {
  expect(api.listModels).not.toHaveBeenCalled();
  cleanup();
});

test("the same requested intent keeps the edit and the response leaf does not own it", async () => {
  const { result } = renderHook(() => useImportContinuation("proj-a", activity));
  await act(async () => { await result.current.prepareContinue("imp:1", "message-205", "rev-1"); });
  expect(api.sessionImportContinuePrepare).toHaveBeenCalledWith("imp:1", { projectId: "proj-a", leaf: "message-205", revision: "rev-1" });
  act(() => result.current.setDraft({ ...result.current.draft!, text: EDITED }));
  api.sessionImportContinueWorkspace.mockResolvedValueOnce({ ...result.current.draft!, draftId: "d2", draftHash: "h2", text: "SERVER", leaf: "resolved-leaf" });
  await act(async () => { await result.current.selectContinuationWorkspace("w1"); });
  expect(result.current.draft?.text).toBe(EDITED);
  const calls = activity.mock.calls.length;
  act(() => result.current.hideContinuation());
  expect(result.current.continuationVisible).toBe(false);
  expect(result.current.draft?.text).toBe(EDITED);
  await act(async () => { await result.current.prepareContinue("imp:1", "message-205", "rev-1"); });
  expect(api.sessionImportContinuePrepare).toHaveBeenCalledTimes(1);
  expect(activity).toHaveBeenCalledTimes(calls);
  expect(result.current.continuationVisible).toBe(true);
  expect(result.current.draft).toMatchObject({ draftId: "d2", text: EDITED });
  await act(async () => { await result.current.prepareContinue("imp:1", "resolved-leaf", "rev-1"); });
  expect(api.sessionImportContinuePrepare).toHaveBeenCalledTimes(2);
  expect(result.current.draft?.text).toBe("原始:resolved-leaf");
  expect(api.sessionImportContinueDiscard).not.toHaveBeenCalled();
  expect(api.sessionImportContinueCreate).not.toHaveBeenCalled();
});

test("a second prepare while the first is running does not replace it", async () => {
  const { result } = renderHook(() => useImportContinuation(null, activity));
  const pending = deferred<HandoffDraft>();
  api.sessionImportContinuePrepare.mockReturnValueOnce(pending.promise);
  let first!: Promise<void>;
  act(() => { first = result.current.prepareContinue("imp:1", "message-205", "rev-1"); });
  act(() => result.current.hideContinuation());
  await act(async () => { await result.current.prepareContinue("imp:1", "message-206", "rev-1"); });
  expect(api.sessionImportContinuePrepare).toHaveBeenCalledTimes(1);
  await act(async () => { pending.resolve(fresh(EDITED)); await first; });
  expect(result.current.draft?.text).toBe(EDITED);
  expect(result.current.continuationVisible).toBe(true);
  act(() => result.current.hideContinuation());
  await act(async () => { await result.current.prepareContinue("imp:1", "message-205", "rev-1"); });
  expect(api.sessionImportContinuePrepare).toHaveBeenCalledTimes(1);
  expect(result.current.draft?.text).toBe(EDITED);
});

test("an older prepare reply cannot overwrite a newer intent or a closed reader", async () => {
  const { result } = renderHook(() => useImportContinuation(null, activity));
  const older = deferred<HandoffDraft>();
  const newer = deferred<HandoffDraft>();
  api.sessionImportContinuePrepare.mockReturnValueOnce(older.promise).mockReturnValueOnce(newer.promise);
  let first!: Promise<void>;
  act(() => { first = result.current.prepareContinue("imp:1", "message-205", "rev-1"); });
  act(() => result.current.invalidateContinuation());
  let second!: Promise<void>;
  act(() => { second = result.current.prepareContinue("imp:1", "message-206", "rev-1"); });
  await act(async () => { older.resolve(fresh("旧回复", "message-205")); await first; });
  expect(result.current.draft).toBeNull();
  await act(async () => { newer.resolve(fresh("新回复", "message-206")); await second; });
  expect(result.current.draft?.text).toBe("新回复");
  expect(result.current.continuationVisible).toBe(true);
});

test("late prepare, workspace, scope and create replies do nothing after unmount", async () => {
  const preparing = deferred<HandoffDraft>();
  api.sessionImportContinuePrepare.mockReturnValueOnce(preparing.promise);
  const prepared = renderHook(() => useImportContinuation(null, activity));
  let prepareTask!: Promise<void>;
  act(() => { prepareTask = prepared.result.current.prepareContinue("imp:1", "message-205", "rev-1"); });
  prepared.unmount();
  const workspace = deferred<HandoffDraft>();
  api.sessionImportContinueWorkspace.mockReturnValueOnce(workspace.promise);
  const selecting = renderHook(() => useImportContinuation(null, activity));
  act(() => selecting.result.current.setDraft({ ...base, text: EDITED }));
  let workspaceTask!: Promise<void>;
  act(() => { workspaceTask = selecting.result.current.selectContinuationWorkspace("w1"); });
  selecting.unmount();
  const scope = deferred<HandoffDraft>();
  api.sessionImportContinueSelection.mockReturnValueOnce(scope.promise);
  const scoping = renderHook(() => useImportContinuation(null, activity));
  act(() => scoping.result.current.setDraft(base));
  let scopeTask!: Promise<boolean>;
  act(() => { scopeTask = scoping.result.current.selectContinuationScope(defaultSelection()); });
  scoping.unmount();
  const creating = deferred<{ appSessionId: string }>();
  api.sessionImportContinueCreate.mockReturnValueOnce(creating.promise);
  const created = renderHook(() => useImportContinuation(null, activity));
  act(() => created.result.current.setDraft({ ...base, text: EDITED }));
  let createTask!: Promise<string | null>;
  act(() => { createTask = created.result.current.createContinue(); });
  created.unmount();
  await act(async () => {
    preparing.resolve(fresh("late"));
    workspace.resolve({ ...base, text: "SERVER" });
    scope.resolve({ ...base, text: "regenerated" });
    creating.resolve({ appSessionId: "native" });
    await prepareTask;
    await workspaceTask;
    expect(await scopeTask).toBe(false);
    expect(await createTask).toBeNull();
  });
});

test("a busy workspace or create cannot be hidden, and a reader close rejects its late reply", async () => {
  const { result } = renderHook(() => useImportContinuation(null, activity));
  act(() => result.current.setDraft({ ...base, text: EDITED }));
  const workspace = deferred<HandoffDraft>();
  api.sessionImportContinueWorkspace.mockReturnValueOnce(workspace.promise);
  let workspaceTask!: Promise<void>;
  act(() => { workspaceTask = result.current.selectContinuationWorkspace("w1"); });
  act(() => result.current.hideContinuation());
  expect(result.current.continuationVisible).toBe(true);
  act(() => result.current.invalidateContinuation());
  await act(async () => { workspace.resolve({ ...base, draftId: "d2", draftHash: "h2", text: "SERVER" }); await workspaceTask; });
  expect(result.current.draft).toBeNull();
  expect(result.current.continuationVisible).toBe(false);
  act(() => result.current.setDraft({ ...base, text: EDITED }));
  const creating = deferred<{ appSessionId: string }>();
  api.sessionImportContinueCreate.mockReturnValueOnce(creating.promise);
  let createTask!: Promise<string | null>;
  act(() => { createTask = result.current.createContinue(); });
  act(() => result.current.hideContinuation());
  expect(result.current.continuationVisible).toBe(true);
  expect(result.current.continuationLocked).toBe(true);
  await act(async () => { creating.resolve({ appSessionId: "native" }); expect(await createTask).toBe("native"); });
  expect(result.current.draft).toBeNull();
});

test("an unknown scope stays frozen across hide, another anchor, and a project change", async () => {
  const { result, rerender } = renderHook(
    ({ project }: { project: string | null }) => useImportContinuation(project, activity),
    { initialProps: { project: "proj-a" as string | null } },
  );
  act(() => result.current.setDraft({ ...base, projectId: "proj-a" }));
  api.sessionImportContinueSelection.mockRejectedValueOnce(new Error("lost reply"));
  const frozen = { ...defaultSelection(), eventIds: ["message-205"] };
  const mutated = structuredClone(frozen);
  await act(async () => { expect(await result.current.selectContinuationScope(mutated)).toBe(false); });
  mutated.eventIds.push("message-206");
  expect(result.current.continuationScopePending).toBe(true);
  act(() => result.current.hideContinuation());
  expect(result.current.continuationVisible).toBe(false);
  expect(result.current.continuationScopePending).toBe(true);
  expect(result.current.draft?.text).toBe(base.text);
  await act(async () => { await result.current.prepareContinue("imp:1", "message-206", "rev-1"); });
  expect(api.sessionImportContinuePrepare).not.toHaveBeenCalled();
  expect(result.current.continuationVisible).toBe(false);
  rerender({ project: "proj-b" });
  expect(result.current.draft?.text).toBe(base.text);
  expect(result.current.continuationScopePending).toBe(true);
  expect(api.sessionImportContinueDiscard).not.toHaveBeenCalled();
  rerender({ project: "proj-a" });
  await act(async () => { await result.current.prepareContinue("imp:1", base.leaf, base.revisionId); });
  expect(result.current.continuationVisible).toBe(true);
  expect(api.sessionImportContinuePrepare).not.toHaveBeenCalled();
  api.sessionImportContinueSelection.mockResolvedValueOnce({ ...base, draftId: "d3", draftHash: "h3", text: "只保留选中范围" });
  await act(async () => { expect(await result.current.selectContinuationScope({ ...defaultSelection(), eventIds: ["other"] })).toBe(true); });
  expect(api.sessionImportContinueSelection.mock.calls[0]?.[2]).toEqual(frozen);
  expect(api.sessionImportContinueSelection.mock.calls[1]?.[2]).toEqual(frozen);
  expect(result.current.draft?.text).toBe("只保留选中范围");
});

test("an uncommitted scope review is dropped, including when the project changes", async () => {
  const { result, rerender } = renderHook(
    ({ project }: { project: string | null }) => useImportContinuation(project, activity),
    { initialProps: { project: "proj-a" as string | null } },
  );
  act(() => result.current.setDraft({ ...base, text: EDITED, projectId: "proj-a" }));
  act(() => result.current.beginContinuationScopeReview());
  act(() => result.current.hideContinuation());
  expect(result.current.continuationReviewingScope).toBe(false);
  expect(result.current.draft?.text).toBe(EDITED);
  expect(api.sessionImportContinueSelection).not.toHaveBeenCalled();
  await act(async () => { await result.current.prepareContinue("imp:1", base.leaf, base.revisionId); });
  expect(result.current.continuationVisible).toBe(true);
  expect(result.current.draft?.text).toBe(EDITED);
  act(() => result.current.beginContinuationScopeReview());
  rerender({ project: "proj-b" });
  expect(result.current.draft).toBeNull();
  expect(api.sessionImportContinueDiscard).not.toHaveBeenCalled();
  expect(api.sessionImportContinueSelection).not.toHaveBeenCalled();
});

test("duplicate create shares one request and a lost reply stays frozen while hidden", async () => {
  const { result } = renderHook(() => useImportContinuation(null, activity));
  act(() => result.current.setDraft({ ...base, text: EDITED }));
  const pending = deferred<{ appSessionId: string }>();
  api.sessionImportContinueCreate.mockReturnValueOnce(pending.promise);
  let first!: Promise<string | null>;
  let second!: Promise<string | null>;
  act(() => { first = result.current.createContinue(); second = result.current.createContinue(); });
  act(() => result.current.hideContinuation());
  expect(api.sessionImportContinueCreate).toHaveBeenCalledTimes(1);
  expect(result.current.continuationVisible).toBe(true);
  await act(async () => {
    pending.resolve({ appSessionId: "native" });
    expect(await first).toBe("native");
    expect(await second).toBeNull();
  });
  expect(result.current.draft).toBeNull();
  act(() => result.current.setDraft({ ...base, text: EDITED }));
  api.sessionImportContinueCreate.mockRejectedValueOnce(new Error("lost reply"));
  await act(async () => { expect(await result.current.createContinue()).toBeNull(); });
  expect(result.current.continuationLocked).toBe(true);
  act(() => result.current.hideContinuation());
  expect(result.current.continuationVisible).toBe(false);
  expect(result.current.continuationLocked).toBe(true);
  act(() => result.current.setDraft({ ...base, text: "replaced" }));
  expect(result.current.draft?.text).toBe(EDITED);
  await act(async () => { await result.current.prepareContinue("imp:1", "other-anchor", "rev-1"); });
  expect(api.sessionImportContinuePrepare).not.toHaveBeenCalled();
  expect(result.current.continuationVisible).toBe(false);
  await act(async () => { await result.current.prepareContinue(base.importId, base.leaf, base.revisionId); });
  expect(result.current.continuationVisible).toBe(true);
  expect(result.current.draft?.text).toBe(EDITED);
  api.sessionImportContinueCreate.mockResolvedValueOnce({ appSessionId: "native" });
  await act(async () => { expect(await result.current.createContinue()).toBe("native"); });
  expect(api.sessionImportContinueCreate.mock.calls[1]).toEqual(api.sessionImportContinueCreate.mock.calls[2]);
  expect(api.sessionImportContinueCreate).toHaveBeenCalledWith("d1", "h1", "continuation:d1", EDITED);
});

test("a recovered frozen creation survives hide and rejects a different intent", async () => {
  const { result } = renderHook(() => useImportContinuation("proj-a", activity));
  act(() => result.current.setDraft({ ...base, text: "冻结🙂正文", creationRequestId: "durable-request", creationVersion: 1, workspaceProjectConstraint: "proj-a" }));
  act(() => result.current.hideContinuation());
  expect(result.current.continuationLocked).toBe(true);
  expect(result.current.continuationVisible).toBe(false);
  await act(async () => { await result.current.prepareContinue("imp:1", "message-205", "rev-1"); });
  expect(api.sessionImportContinuePrepare).not.toHaveBeenCalled();
  expect(result.current.continuationVisible).toBe(false);
  await act(async () => { await result.current.prepareContinue(base.importId, base.leaf, base.revisionId); });
  expect(result.current.continuationVisible).toBe(true);
  expect(result.current.draft?.text).toBe("冻结🙂正文");
  expect(result.current.continuationLocked).toBe(true);
  api.sessionImportContinueCreate.mockResolvedValueOnce({ appSessionId: "native" });
  await act(async () => { expect(await result.current.createContinue()).toBe("native"); });
  expect(api.sessionImportContinueCreate).toHaveBeenCalledWith("d1", "h1", "durable-request", "冻结🙂正文");
});

test("only a successful cancel clears the slot; failure remains visible after hide", async () => {
  const { result } = renderHook(() => useImportContinuation(null, activity));
  act(() => result.current.setDraft({ ...base, text: EDITED }));
  api.sessionImportContinueDiscard.mockRejectedValueOnce(new Error("SNAPSHOT_UNAVAILABLE: disk locked"));
  await act(async () => { expect(await result.current.discardContinuation()).toBe(false); });
  expect(result.current.continuationError).toMatch(/SNAPSHOT_UNAVAILABLE/);
  expect(result.current.continuationVisible).toBe(true);
  act(() => result.current.hideContinuation());
  expect(result.current.draft?.text).toBe(EDITED);
  expect(result.current.continuationError).toMatch(/SNAPSHOT_UNAVAILABLE/);
  await act(async () => { await result.current.prepareContinue(base.importId, base.leaf, base.revisionId); });
  expect(api.sessionImportContinuePrepare).not.toHaveBeenCalled();
  expect(result.current.continuationVisible).toBe(true);
  expect(result.current.continuationError).toMatch(/SNAPSHOT_UNAVAILABLE/);
  api.sessionImportContinueDiscard.mockResolvedValueOnce(undefined);
  await act(async () => { expect(await result.current.discardContinuation()).toBe(true); });
  expect(result.current.draft).toBeNull();
  expect(result.current.continuationVisible).toBe(false);
  await act(async () => { await result.current.prepareContinue(base.importId, base.leaf, base.revisionId); });
  expect(api.sessionImportContinuePrepare).toHaveBeenCalledTimes(1);
  expect(result.current.draft?.text).toBe(`原始:${base.leaf}`);
});

test("project changes drop an unlocked hidden edit and keep the same project", async () => {
  const { result, rerender } = renderHook(
    ({ project }: { project: string | null }) => useImportContinuation(project, activity),
    { initialProps: { project: "proj-a" as string | null } },
  );
  await act(async () => { await result.current.prepareContinue("imp:1", "message-205", "rev-1"); });
  act(() => result.current.setDraft({ ...result.current.draft!, text: EDITED }));
  act(() => result.current.hideContinuation());
  rerender({ project: "proj-a" });
  expect(result.current.draft?.text).toBe(EDITED);
  rerender({ project: "proj-b" });
  expect(result.current.draft).toBeNull();
  expect(api.sessionImportContinueDiscard).not.toHaveBeenCalled();
  await act(async () => { await result.current.prepareContinue("imp:1", "message-205", "rev-1"); });
  expect(api.sessionImportContinuePrepare).toHaveBeenCalledTimes(2);
  expect(api.sessionImportContinuePrepare).toHaveBeenLastCalledWith("imp:1", { projectId: "proj-b", leaf: "message-205", revision: "rev-1" });
  expect(result.current.draft?.text).not.toContain("交接");
});

test("a locked draft stays concealed when the project changes and returns with the original project", async () => {
  const { result, rerender } = renderHook(
    ({ project }: { project: string | null }) => useImportContinuation(project, activity),
    { initialProps: { project: "proj-a" as string | null } },
  );
  act(() => result.current.setDraft({ ...base, text: EDITED, creationRequestId: "durable-request", projectId: "proj-a" }));
  act(() => result.current.hideContinuation());
  rerender({ project: "proj-b" });
  expect(result.current.draft?.text).toBe(EDITED);
  expect(result.current.continuationLocked).toBe(true);
  expect(result.current.continuationVisible).toBe(false);
  await act(async () => { await result.current.prepareContinue("imp:1", base.leaf, base.revisionId); });
  expect(api.sessionImportContinuePrepare).not.toHaveBeenCalled();
  rerender({ project: "proj-a" });
  await act(async () => { await result.current.prepareContinue(base.importId, base.leaf, base.revisionId); });
  expect(result.current.continuationVisible).toBe(true);
  expect(result.current.draft?.text).toBe(EDITED);
  expect(api.sessionImportContinuePrepare).not.toHaveBeenCalled();
  expect(api.sessionImportContinueDiscard).not.toHaveBeenCalled();
});

test("whitespace cannot be created before or after it is hidden", async () => {
  const { result } = renderHook(() => useImportContinuation(null, activity));
  act(() => result.current.setDraft({ ...base, text: " \n\t " }));
  await act(async () => { expect(await result.current.createContinue()).toBeNull(); });
  act(() => result.current.hideContinuation());
  expect(result.current.draft?.text).toBe(" \n\t ");
  await act(async () => { await result.current.prepareContinue(base.importId, base.leaf, base.revisionId); });
  await act(async () => { expect(await result.current.createContinue()).toBeNull(); });
  expect(api.sessionImportContinueCreate).not.toHaveBeenCalled();
  expect(api.sessionImportContinuePrepare).not.toHaveBeenCalled();
  expect(result.current.draft?.text).toBe(" \n\t ");
});
