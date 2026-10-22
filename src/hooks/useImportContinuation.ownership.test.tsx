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

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (cause: unknown) => void;
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  promise.catch(() => undefined);
  return { promise, resolve, reject };
}
function hostDraft(over: Partial<HandoffDraft> = {}): HandoffDraft {
  return {
    draftId: "d1", draftHash: "h1", importId: "imp:1", revisionId: "auth-rev", leaf: "auth-leaf",
    text: "解析后", coverage: "full", estimatedTokens: 4, runtimeCapability: "fragment-only",
    creationVersion: 1, workspaceProjectConstraint: "proj-a", projectId: "proj-a",
    selection: { ...defaultSelection(), eventIds: ["keep", "drop-me"] },
    ...over,
  };
}
function mount(project: string | null = "proj-a") {
  return renderHook(
    ({ project: current }: { project: string | null }) => useImportContinuation(current, activity),
    { initialProps: { project } },
  );
}

beforeEach(() => {
  vi.resetAllMocks();
  activity.mockReset();
  api.sessionImportContinuePrepare.mockImplementation(async (_importId: string, opts: { projectId?: string | null; leaf?: string | null }) => hostDraft({
    text: `正文:${opts.projectId ?? "none"}`,
    leaf: opts.leaf ? opts.leaf : "auth-leaf",
    revisionId: "auth-rev",
    workspaceProjectConstraint: opts.projectId ?? null,
    projectId: opts.projectId ?? null,
    draftId: `d:${opts.projectId ?? "none"}`,
    draftHash: `h:${opts.projectId ?? "none"}`,
  }));
  api.sessionImportContinueDiscard.mockResolvedValue(undefined);
  api.sessionImportContinueRestore.mockRejectedValue(new Error("offline"));
  api.sessionImportContinueCreate.mockResolvedValue({ appSessionId: "native" });
});
afterEach(() => {
  expect(api.listModels).not.toHaveBeenCalled();
  cleanup();
});

test("a project change clears busy before the first prepare has a draft", async () => {
  const pending = deferred<HandoffDraft>();
  api.sessionImportContinuePrepare.mockReturnValueOnce(pending.promise);
  const hook = mount("proj-a");
  let task!: Promise<void>;
  act(() => { task = hook.result.current.prepareContinue("imp:1", "anchor-a", "rev-1"); });
  expect(hook.result.current.draft).toBeNull();
  expect(hook.result.current.continuationBusy).toBe(true);
  hook.rerender({ project: "proj-b" });
  expect(hook.result.current.continuationBusy).toBe(false);
  expect(hook.result.current.draft).toBeNull();
  expect(hook.result.current.continuationVisible).toBe(false);
  expect(api.sessionImportContinueDiscard).not.toHaveBeenCalled();
  await act(async () => { pending.reject(new Error("unused")); await task; });
  expect(hook.result.current.continuationError).toBeNull();
  expect(hook.result.current.draft).toBeNull();
});

test("the new project starts its prepare while the first request is still pending", async () => {
  const pendingA = deferred<HandoffDraft>();
  const pendingB = deferred<HandoffDraft>();
  api.sessionImportContinuePrepare.mockReturnValueOnce(pendingA.promise).mockReturnValueOnce(pendingB.promise);
  const hook = mount("proj-a");
  let first!: Promise<void>;
  act(() => { first = hook.result.current.prepareContinue("imp:1", "anchor-a", "rev-1"); });
  hook.rerender({ project: "proj-b" });
  let second!: Promise<void>;
  act(() => { second = hook.result.current.prepareContinue("imp:1", "anchor-b", "rev-1"); });
  expect(api.sessionImportContinuePrepare).toHaveBeenCalledTimes(2);
  expect(api.sessionImportContinuePrepare).toHaveBeenLastCalledWith("imp:1", { projectId: "proj-b", leaf: "anchor-b", revision: "rev-1" });
  expect(hook.result.current.continuationBusy).toBe(true);
  expect(hook.result.current.draft).toBeNull();
  await act(async () => { pendingA.resolve(hostDraft({ text: "A迟到🙂", draftId: "da", draftHash: "ha" })); await first; });
  expect(hook.result.current.draft).toBeNull();
  expect(hook.result.current.continuationError).toBeNull();
  expect(hook.result.current.continuationVisible).toBe(false);
  expect(hook.result.current.continuationBusy).toBe(true);
  expect(api.sessionImportContinueCreate).not.toHaveBeenCalled();
  await act(async () => {
    pendingB.resolve(hostDraft({
      text: "B正文🙂", draftId: "db", draftHash: "hb", leaf: "anchor-b", revisionId: "rev-1",
      workspaceProjectConstraint: "proj-b", projectId: "proj-b",
    }));
    await second;
  });
  expect(hook.result.current.draft).toMatchObject({ draftId: "db", draftHash: "hb", text: "B正文🙂", leaf: "anchor-b" });
  expect(hook.result.current.continuationBusy).toBe(false);
  expect(hook.result.current.continuationVisible).toBe(true);
  act(() => hook.result.current.setDraft({ ...hook.result.current.draft!, text: EDITED }));
  act(() => hook.result.current.hideContinuation());
  await act(async () => { await hook.result.current.prepareContinue("imp:1", "anchor-b", "rev-1"); });
  expect(api.sessionImportContinuePrepare).toHaveBeenCalledTimes(2);
  expect(hook.result.current.draft?.text).toBe(EDITED);
  expect(hook.result.current.continuationVisible).toBe(true);
  expect(api.sessionImportContinueDiscard).not.toHaveBeenCalled();
});

test("a late prepare failure does not write an error into the next project", async () => {
  const pendingA = deferred<HandoffDraft>();
  const pendingB = deferred<HandoffDraft>();
  api.sessionImportContinuePrepare.mockReturnValueOnce(pendingA.promise).mockReturnValueOnce(pendingB.promise);
  const hook = mount("proj-a");
  let first!: Promise<void>;
  act(() => { first = hook.result.current.prepareContinue("imp:1", "anchor-a", "rev-1"); });
  hook.rerender({ project: "proj-b" });
  let second!: Promise<void>;
  act(() => { second = hook.result.current.prepareContinue("imp:1", "anchor-b", "rev-1"); });
  await act(async () => { pendingA.reject(new Error("A失败")); await first; });
  expect(hook.result.current.continuationError).toBeNull();
  expect(hook.result.current.continuationBusy).toBe(true);
  expect(hook.result.current.draft).toBeNull();
  expect(hook.result.current.continuationVisible).toBe(false);
  await act(async () => {
    pendingB.resolve(hostDraft({ text: "B正文🙂", draftId: "db", draftHash: "hb", leaf: "anchor-b", workspaceProjectConstraint: "proj-b", projectId: "proj-b" }));
    await second;
  });
  expect(hook.result.current.draft?.text).toBe("B正文🙂");
  expect(hook.result.current.continuationError).toBeNull();
  expect(api.sessionImportContinueCreate).not.toHaveBeenCalled();
});

test("returning through A does not resurrect the superseded in-flight prepare", async () => {
  const pendingA = deferred<HandoffDraft>();
  const pendingB = deferred<HandoffDraft>();
  api.sessionImportContinuePrepare.mockReturnValueOnce(pendingA.promise).mockReturnValueOnce(pendingB.promise);
  const hook = mount("proj-a");
  let first!: Promise<void>;
  act(() => { first = hook.result.current.prepareContinue("imp:1", "anchor-a", "rev-1"); });
  hook.rerender({ project: "proj-b" });
  let second!: Promise<void>;
  act(() => { second = hook.result.current.prepareContinue("imp:1", "anchor-b", "rev-1"); });
  hook.rerender({ project: "proj-a" });
  await act(async () => { pendingA.resolve(hostDraft({ text: "A迟到🙂" })); await first; });
  await act(async () => { pendingB.resolve(hostDraft({ text: "B迟到🙂", draftId: "db" })); await second; });
  expect(hook.result.current.draft).toBeNull();
  expect(hook.result.current.continuationVisible).toBe(false);
  expect(hook.result.current.continuationBusy).toBe(false);
  await act(async () => { await hook.result.current.prepareContinue("imp:1", "anchor-a", "rev-1"); });
  expect(api.sessionImportContinuePrepare).toHaveBeenCalledTimes(3);
  expect(api.sessionImportContinuePrepare).toHaveBeenLastCalledWith("imp:1", { projectId: "proj-a", leaf: "anchor-a", revision: "rev-1" });
  expect(hook.result.current.draft?.text).toBe("正文:proj-a");
  expect(hook.result.current.draft?.text).not.toBe("A迟到🙂");
});

test("null and a concrete project supersede each other's pending prepare", async () => {
  const fromNull = deferred<HandoffDraft>();
  const fromProject = deferred<HandoffDraft>();
  api.sessionImportContinuePrepare.mockReturnValueOnce(fromNull.promise).mockReturnValueOnce(fromProject.promise);
  const hook = mount(null);
  let first!: Promise<void>;
  act(() => { first = hook.result.current.prepareContinue("imp:1", null, null); });
  hook.rerender({ project: "proj-a" });
  let second!: Promise<void>;
  act(() => { second = hook.result.current.prepareContinue("imp:1", "anchor-a", "rev-1"); });
  expect(api.sessionImportContinuePrepare).toHaveBeenNthCalledWith(1, "imp:1", { projectId: null, leaf: null, revision: null });
  expect(api.sessionImportContinuePrepare).toHaveBeenNthCalledWith(2, "imp:1", { projectId: "proj-a", leaf: "anchor-a", revision: "rev-1" });
  await act(async () => { fromNull.resolve(hostDraft({ text: "空项目迟到", workspaceProjectConstraint: null, projectId: null })); await first; });
  expect(hook.result.current.draft).toBeNull();
  expect(hook.result.current.continuationBusy).toBe(true);
  hook.rerender({ project: null });
  await act(async () => { fromProject.resolve(hostDraft({ text: "项目迟到" })); await second; });
  expect(hook.result.current.draft).toBeNull();
  expect(hook.result.current.continuationError).toBeNull();
  await act(async () => { await hook.result.current.prepareContinue("imp:1", null, null); });
  expect(hook.result.current.draft?.text).toBe("正文:none");
  expect(api.sessionImportContinuePrepare).toHaveBeenLastCalledWith("imp:1", { projectId: null, leaf: null, revision: null });
});

test("unmount rejects a late prepare without writing the draft", async () => {
  const pending = deferred<HandoffDraft>();
  api.sessionImportContinuePrepare.mockReturnValueOnce(pending.promise);
  const hook = mount("proj-a");
  let task!: Promise<void>;
  act(() => { task = hook.result.current.prepareContinue("imp:1", "anchor-a", "rev-1"); });
  hook.unmount();
  await act(async () => { pending.resolve(hostDraft({ text: "卸载后" })); await task; });
  expect(api.sessionImportContinuePrepare).toHaveBeenCalledTimes(1);
  expect(api.sessionImportContinueCreate).not.toHaveBeenCalled();
});

test("a pending workspace change drops the unlocked draft and ignores the late reply", async () => {
  const hook = mount("proj-a");
  await act(async () => { await hook.result.current.prepareContinue("imp:1", "anchor-a", "rev-1"); });
  act(() => hook.result.current.setDraft({ ...hook.result.current.draft!, text: EDITED }));
  const pending = deferred<HandoffDraft>();
  api.sessionImportContinueWorkspace.mockReturnValueOnce(pending.promise);
  let task!: Promise<void>;
  act(() => { task = hook.result.current.selectContinuationWorkspace("w-a"); });
  hook.rerender({ project: "proj-b" });
  expect(hook.result.current.continuationVisible).toBe(false);
  await act(async () => { pending.resolve(hostDraft({ text: "工作区迟到", draftId: "dw", draftHash: "hw" })); await task; });
  expect(hook.result.current.draft).toBeNull();
  expect(api.sessionImportContinueDiscard).not.toHaveBeenCalled();
  await act(async () => { await hook.result.current.prepareContinue("imp:1", "anchor-a", "rev-1"); });
  expect(hook.result.current.draft?.text).toBe("正文:proj-b");
  expect(hook.result.current.draft?.text).not.toContain("交接");
});

test("a pending scope stays frozen across a project change and rejects the late reply", async () => {
  const hook = mount("proj-a");
  await act(async () => { await hook.result.current.prepareContinue("imp:1", "anchor-a", "rev-1"); });
  act(() => hook.result.current.setDraft({ ...hook.result.current.draft!, text: EDITED }));
  const pending = deferred<HandoffDraft>();
  api.sessionImportContinueSelection.mockReturnValueOnce(pending.promise);
  const narrow = { ...defaultSelection(), eventIds: ["keep"] };
  let task!: Promise<boolean>;
  act(() => { task = hook.result.current.selectContinuationScope(narrow); });
  narrow.eventIds.push("drop-me");
  hook.rerender({ project: "proj-b" });
  expect(hook.result.current.continuationVisible).toBe(false);
  expect(hook.result.current.draft?.text).toBe(EDITED);
  await act(async () => { pending.resolve(hostDraft({ text: "范围迟到", draftId: "ds", draftHash: "hs" })); expect(await task).toBe(false); });
  expect(hook.result.current.draft?.text).toBe(EDITED);
  expect(hook.result.current.continuationVisible).toBe(false);
  expect(hook.result.current.continuationScopePending).toBe(true);
  await act(async () => { await hook.result.current.prepareContinue("imp:1", "anchor-a", "rev-1"); });
  expect(api.sessionImportContinuePrepare).toHaveBeenCalledTimes(1);
  expect(hook.result.current.continuationVisible).toBe(false);
  expect(api.sessionImportContinueDiscard).not.toHaveBeenCalled();
  hook.rerender({ project: "proj-a" });
  await act(async () => { await hook.result.current.prepareContinue("imp:1", "anchor-a", "rev-1"); });
  expect(hook.result.current.continuationVisible).toBe(true);
  expect(hook.result.current.draft?.text).toBe(EDITED);
  expect(hook.result.current.continuationScopePending).toBe(true);
  expect(api.sessionImportContinueSelection.mock.calls[0][2]).toEqual({ ...defaultSelection(), eventIds: ["keep"] });
});

test("a pending create stays frozen, and its late session id does not open or clear it", async () => {
  const hook = mount("proj-a");
  await act(async () => { await hook.result.current.prepareContinue("imp:1", "anchor-a", "rev-1"); });
  act(() => hook.result.current.setDraft({ ...hook.result.current.draft!, text: EDITED }));
  const pending = deferred<{ appSessionId: string }>();
  api.sessionImportContinueCreate.mockReturnValueOnce(pending.promise);
  let task!: Promise<string | null>;
  act(() => { task = hook.result.current.createContinue(); });
  hook.rerender({ project: "proj-b" });
  expect(hook.result.current.continuationVisible).toBe(false);
  expect(hook.result.current.continuationLocked).toBe(true);
  await act(async () => { pending.resolve({ appSessionId: "native-late" }); expect(await task).toBeNull(); });
  expect(hook.result.current.draft?.text).toBe(EDITED);
  expect(hook.result.current.continuationLocked).toBe(true);
  expect(hook.result.current.continuationVisible).toBe(false);
  await act(async () => { await hook.result.current.prepareContinue("imp:1", "anchor-b", "rev-1"); });
  expect(api.sessionImportContinuePrepare).toHaveBeenCalledTimes(1);
  expect(hook.result.current.draft?.text).toBe(EDITED);
  expect(api.sessionImportContinueDiscard).not.toHaveBeenCalled();
  expect(api.sessionImportContinueCreate).toHaveBeenCalledTimes(1);
  hook.rerender({ project: "proj-a" });
  await act(async () => { await hook.result.current.prepareContinue("imp:1", "anchor-a", "rev-1"); });
  expect(hook.result.current.continuationVisible).toBe(true);
  api.sessionImportContinueCreate.mockResolvedValueOnce({ appSessionId: "native" });
  await act(async () => { expect(await hook.result.current.createContinue()).toBe("native"); });
  expect(api.sessionImportContinueCreate.mock.calls[0]).toEqual(api.sessionImportContinueCreate.mock.calls[1]);
  expect(api.sessionImportContinueCreate).toHaveBeenLastCalledWith(expect.any(String), expect.any(String), expect.stringMatching(/^continuation:/), EDITED);
});

test("a Host-proven pre-claim restore keeps the edited body on the requested null identity", async () => {
  const hook = mount("proj-a");
  api.sessionImportContinuePrepare.mockResolvedValueOnce(hostDraft({
    leaf: "auth-leaf", revisionId: "auth-rev", text: "解析后",
    workspaceProjectConstraint: "proj-a", projectId: "proj-a",
  }));
  await act(async () => { await hook.result.current.prepareContinue("imp:1", null, null); });
  expect(api.sessionImportContinuePrepare).toHaveBeenCalledWith("imp:1", { projectId: "proj-a", leaf: null, revision: null });
  expect(hook.result.current.draft?.leaf).toBe("auth-leaf");
  expect(hook.result.current.draft?.revisionId).toBe("auth-rev");
  act(() => hook.result.current.setDraft({ ...hook.result.current.draft!, text: EDITED }));
  api.sessionImportContinueCreate.mockRejectedValueOnce(new Error("workspace changed"));
  api.sessionImportContinueRestore.mockResolvedValueOnce(hostDraft({
    leaf: "auth-leaf", revisionId: "auth-rev", text: "host-replaced",
    workspaceProjectConstraint: "proj-a", projectId: "proj-a",
  }));
  await act(async () => { expect(await hook.result.current.createContinue()).toBeNull(); });
  expect(hook.result.current.continuationLocked).toBe(false);
  expect(hook.result.current.draft?.text).toBe(EDITED);
  act(() => hook.result.current.hideContinuation());
  await act(async () => { await hook.result.current.prepareContinue("imp:1", null, null); });
  expect(api.sessionImportContinuePrepare).toHaveBeenCalledTimes(1);
  expect(hook.result.current.continuationVisible).toBe(true);
  expect(hook.result.current.draft?.text).toBe(EDITED);
  await act(async () => { await hook.result.current.prepareContinue("imp:1", "other-leaf", "auth-rev"); });
  expect(api.sessionImportContinuePrepare).toHaveBeenCalledTimes(2);
  expect(hook.result.current.draft?.text).not.toBe(EDITED);
});

test("a committed restore keeps the request, text, hash and id locked across hide and retry", async () => {
  const hook = mount("proj-a");
  api.sessionImportContinuePrepare.mockResolvedValueOnce(hostDraft());
  await act(async () => { await hook.result.current.prepareContinue("imp:1", null, null); });
  act(() => hook.result.current.setDraft({ ...hook.result.current.draft!, text: EDITED }));
  api.sessionImportContinueCreate.mockRejectedValueOnce(new Error("lost reply"));
  api.sessionImportContinueRestore.mockResolvedValueOnce(hostDraft({
    creationRequestId: "req-1", text: EDITED, draftId: "d1", draftHash: "h1",
    leaf: "auth-leaf", revisionId: "auth-rev",
  }));
  await act(async () => { expect(await hook.result.current.createContinue()).toBeNull(); });
  expect(hook.result.current.continuationLocked).toBe(true);
  expect(hook.result.current.draft).toMatchObject({ draftId: "d1", draftHash: "h1", text: EDITED, creationRequestId: "req-1" });
  act(() => hook.result.current.hideContinuation());
  await act(async () => { await hook.result.current.prepareContinue("imp:1", null, null); });
  expect(api.sessionImportContinuePrepare).toHaveBeenCalledTimes(1);
  expect(hook.result.current.continuationVisible).toBe(true);
  expect(hook.result.current.continuationLocked).toBe(true);
  expect(hook.result.current.draft?.text).toBe(EDITED);
  await act(async () => { await hook.result.current.prepareContinue("imp:1", "auth-leaf", "auth-rev"); });
  expect(api.sessionImportContinuePrepare).toHaveBeenCalledTimes(1);
  expect(hook.result.current.continuationVisible).toBe(false);
  await act(async () => { await hook.result.current.prepareContinue("imp:1", null, null); });
  api.sessionImportContinueCreate.mockResolvedValueOnce({ appSessionId: "native" });
  await act(async () => { expect(await hook.result.current.createContinue()).toBe("native"); });
  expect(api.sessionImportContinueCreate).toHaveBeenLastCalledWith("d1", "h1", "req-1", EDITED);
});

test("a scope error with a committed request stays reopenable and keeps the frozen exclusion", async () => {
  const hook = mount("proj-a");
  api.sessionImportContinuePrepare.mockResolvedValueOnce(hostDraft({ text: "全量🙂\n不要带上这段" }));
  await act(async () => { await hook.result.current.prepareContinue("imp:1", null, null); });
  const narrow = { ...defaultSelection(), eventIds: ["keep"] };
  api.sessionImportContinueSelection.mockRejectedValueOnce(new Error("scope failed"));
  api.sessionImportContinueRestore.mockResolvedValueOnce(hostDraft({
    creationRequestId: "req-scope", text: "只保留选中范围", draftId: "d1", draftHash: "h1",
    leaf: "auth-leaf", revisionId: "auth-rev",
    selection: { ...defaultSelection(), eventIds: ["keep"] },
  }));
  await act(async () => { expect(await hook.result.current.selectContinuationScope(narrow)).toBe(false); });
  narrow.eventIds.push("drop-me");
  expect(hook.result.current.continuationScopePending).toBe(true);
  expect(hook.result.current.continuationLocked).toBe(true);
  expect(hook.result.current.draft?.text).toBe("只保留选中范围");
  expect(hook.result.current.draft?.text).not.toContain("不要带上这段");
  expect(hook.result.current.draft?.selection?.eventIds).toEqual(["keep"]);
  expect(api.sessionImportContinueSelection.mock.calls[0][2]).toEqual({ ...defaultSelection(), eventIds: ["keep"] });
  act(() => hook.result.current.hideContinuation());
  await act(async () => { await hook.result.current.prepareContinue("imp:1", null, null); });
  expect(api.sessionImportContinuePrepare).toHaveBeenCalledTimes(1);
  expect(hook.result.current.continuationVisible).toBe(true);
  expect(hook.result.current.draft?.selection?.eventIds).toEqual(["keep"]);
  expect(hook.result.current.continuationScopePending).toBe(true);
  await act(async () => { expect(await hook.result.current.discardContinuation()).toBe(false); });
  expect(api.sessionImportContinueDiscard).not.toHaveBeenCalled();
  await act(async () => { expect(await hook.result.current.selectContinuationScope({ ...defaultSelection(), eventIds: ["keep", "drop-me"] })).toBe(false); });
  expect(api.sessionImportContinueSelection).toHaveBeenCalledTimes(1);
});

test("startup setDraft uses an explicit null constraint instead of the active wizard", async () => {
  const hook = mount("proj-a");
  act(() => hook.result.current.setDraft(hostDraft({
    text: "空约束🙂", creationRequestId: "durable-null", creationVersion: 1,
    workspaceProjectConstraint: null, projectId: "proj-a", leaf: "auth-leaf", revisionId: "auth-rev",
  })));
  expect(hook.result.current.draft?.text).toBe("空约束🙂");
  expect(hook.result.current.continuationLocked).toBe(true);
  expect(hook.result.current.continuationVisible).toBe(false);
  await act(async () => { await hook.result.current.prepareContinue("imp:1", "auth-leaf", "auth-rev"); });
  expect(api.sessionImportContinuePrepare).not.toHaveBeenCalled();
  expect(hook.result.current.continuationVisible).toBe(false);
  hook.rerender({ project: null });
  await act(async () => { await hook.result.current.prepareContinue("imp:1", "auth-leaf", "auth-rev"); });
  expect(api.sessionImportContinuePrepare).not.toHaveBeenCalled();
  expect(hook.result.current.continuationVisible).toBe(true);
  expect(hook.result.current.draft?.text).toBe("空约束🙂");
  expect(api.sessionImportContinueDiscard).not.toHaveBeenCalled();
});
