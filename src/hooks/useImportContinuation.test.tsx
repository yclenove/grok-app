/** @vitest-environment jsdom */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { HandoffDraft } from "@/lib/api/sessionMigration";
import { useImportContinuation } from "./useImportContinuation";

const api = vi.hoisted(() => ({
  sessionImportContinuePrepare: vi.fn(), sessionImportContinueWorkspace: vi.fn(),
  sessionImportContinueCreate: vi.fn(), sessionImportContinueRestore: vi.fn(),
  sessionImportContinueDiscard: vi.fn(),
}));
vi.mock("@/lib/api", () => api);
const activity = vi.fn();
const draft: HandoffDraft = { draftId: "d1", draftHash: "h1", importId: "imp:1", revisionId: "r1",
  leaf: "leaf", text: "reviewed text", coverage: "full", estimatedTokens: 3, runtimeCapability: "fragment-only" };
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}
beforeEach(() => { vi.resetAllMocks(); api.sessionImportContinueDiscard.mockResolvedValue(undefined); });
afterEach(cleanup);

test("duplicate clicks share a single request and retries freeze input when the outcome is unknown", async () => {
  const { result } = renderHook(() => useImportContinuation(null, activity));
  act(() => result.current.setDraft(draft));
  api.sessionImportContinueCreate.mockRejectedValueOnce(new Error("lost reply"));
  api.sessionImportContinueRestore.mockRejectedValueOnce(new Error("offline"));
  await act(async () => {
    const first = result.current.createContinue();
    expect(await result.current.createContinue()).toBeNull();
    await first;
  });
  expect(api.sessionImportContinueCreate).toHaveBeenCalledTimes(1);
  expect(result.current.continuationLocked).toBe(true);
  act(() => result.current.setDraft({ ...draft, text: "new text" }));
  expect(result.current.draft?.text).toBe(draft.text);
  api.sessionImportContinueCreate.mockResolvedValueOnce({ appSessionId: "native" });
  await act(async () => { expect(await result.current.createContinue()).toBe("native"); });
  expect(api.sessionImportContinueCreate.mock.calls[1]).toEqual(api.sessionImportContinueCreate.mock.calls[0]);
  expect(result.current.draft).toBeNull();
  expect(result.current.continuationBusy).toBe(false);
});

test("Host-proven pre-claim errors unlock editing but committed errors restore the durable request", async () => {
  const { result } = renderHook(() => useImportContinuation(null, activity));
  act(() => result.current.setDraft(draft));
  api.sessionImportContinueCreate.mockRejectedValue(new Error("workspace changed"));
  api.sessionImportContinueRestore.mockResolvedValueOnce(draft);
  await act(async () => { await result.current.createContinue(); });
  expect(result.current.continuationLocked).toBe(false);
  act(() => result.current.setDraft({ ...draft, text: "revised" }));
  api.sessionImportContinueRestore.mockResolvedValueOnce({ ...draft, text: "durable", creationRequestId: "persisted" });
  await act(async () => { await result.current.createContinue(); });
  expect(result.current.draft?.text).toBe("durable");
  expect(result.current.continuationLocked).toBe(true);
  api.sessionImportContinueCreate.mockResolvedValueOnce({ appSessionId: "native" });
  await act(async () => { await result.current.createContinue(); });
  expect(api.sessionImportContinueCreate).toHaveBeenLastCalledWith("d1", "h1", "persisted", "durable");
});

test("target review retains edited handoff text and obtains a new draft identity", async () => {
  const { result } = renderHook(() => useImportContinuation("project", activity));
  api.sessionImportContinuePrepare.mockResolvedValueOnce(draft);
  await act(async () => { await result.current.prepareContinue("imp:1", "leaf", "r1"); });
  expect(api.sessionImportContinuePrepare).toHaveBeenCalledWith("imp:1", { projectId: "project", leaf: "leaf", revision: "r1" });
  act(() => result.current.setDraft({ ...draft, text: "user edit" }));
  api.sessionImportContinueWorkspace.mockResolvedValueOnce({ ...draft, draftId: "d2", draftHash: "h2" });
  await act(async () => { await result.current.selectContinuationWorkspace("workspace"); });
  expect(result.current.draft).toMatchObject({ draftId: "d2", draftHash: "h2", text: "user edit" });
});

test("recovered pending creation reuses its request and cancellation failure keeps the draft", async () => {
  const { result } = renderHook(() => useImportContinuation(null, activity));
  act(() => result.current.setDraft({ ...draft, creationRequestId: "durable-request" }));
  await act(async () => { await result.current.selectContinuationWorkspace("different"); });
  expect(api.sessionImportContinueWorkspace).not.toHaveBeenCalled();
  api.sessionImportContinueDiscard.mockRejectedValueOnce(new Error("already published"));
  await act(async () => { expect(await result.current.discardContinuation()).toBe(false); });
  expect(result.current.draft).not.toBeNull();
  api.sessionImportContinueCreate.mockResolvedValueOnce({ appSessionId: "native" });
  await act(async () => { await result.current.createContinue(); });
  expect(api.sessionImportContinueCreate).toHaveBeenLastCalledWith("d1", "h1", "durable-request", draft.text);
});

test("late prepare, workspace and create replies do not reopen a closed or replaced reader", async () => {
  const { result } = renderHook(() => useImportContinuation(null, activity));
  const preparing = deferred<HandoffDraft>();
  api.sessionImportContinuePrepare.mockReturnValueOnce(preparing.promise);
  let pending!: Promise<void>;
  act(() => { pending = result.current.prepareContinue("imp:1"); });
  act(() => result.current.invalidateContinuation());
  await act(async () => { preparing.resolve(draft); await pending; });
  expect(result.current.draft).toBeNull();
  act(() => result.current.setDraft(draft));
  const selecting = deferred<HandoffDraft>();
  api.sessionImportContinueWorkspace.mockReturnValueOnce(selecting.promise);
  act(() => { pending = result.current.selectContinuationWorkspace("workspace"); });
  act(() => result.current.invalidateContinuation());
  await act(async () => { selecting.resolve(draft); await pending; });
  expect(result.current.draft).toBeNull();
  act(() => result.current.setDraft(draft));
  const creating = deferred<{ appSessionId: string }>();
  api.sessionImportContinueCreate.mockReturnValueOnce(creating.promise);
  let creation!: Promise<string | null>;
  act(() => { creation = result.current.createContinue(); });
  act(() => result.current.invalidateContinuation());
  await act(async () => { creating.resolve({ appSessionId: "native" }); expect(await creation).toBeNull(); });
  expect(result.current.draft).toBeNull();
});

test("successful cancellation clears the durable draft and empty content cannot create", async () => {
  const { result } = renderHook(() => useImportContinuation(null, activity));
  act(() => result.current.setDraft({ ...draft, text: "  " }));
  await act(async () => { expect(await result.current.createContinue()).toBeNull(); });
  expect(api.sessionImportContinueCreate).not.toHaveBeenCalled();
  await act(async () => { expect(await result.current.discardContinuation()).toBe(true); });
  expect(api.sessionImportContinueDiscard).toHaveBeenCalledWith("d1", "h1");
  expect(result.current.draft).toBeNull();
});
