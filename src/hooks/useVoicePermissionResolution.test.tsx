/** @vitest-environment jsdom */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { VoicePermissionPrompt } from "@/lib/voiceOverlay";
import { useVoicePermissionResolution } from "./useVoicePermissionResolution";

const api = vi.hoisted(() => ({ sessionResolvePermission: vi.fn() }));
vi.mock("@/lib/api", () => api);
const prompt: VoicePermissionPrompt = {
  sessionId: "delegated", rpcId: 81, requestGeneration: "old", toolCallId: "tool", toolName: "shell",
  title: "python 示例.py", preview: "python 示例.py", scopeKey: "shell:python", options: [],
};
function deferred() {
  let resolve!: () => void; let reject!: (error: unknown) => void;
  const promise = new Promise<void>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}
function mount() {
  const onResolved = vi.fn(); const onFailure = vi.fn();
  const hook = renderHook(({ open }) => useVoicePermissionResolution({ open, onResolved, onFailure }),
    { initialProps: { open: true } });
  act(() => { hook.result.current.setPendingPermission(prompt); });
  return { hook, onResolved, onFailure };
}
beforeEach(() => vi.resetAllMocks());
afterEach(cleanup);

test.each(["success", "failure"])("old voice reply cannot replace the next prompt: %s", async (outcome) => {
  const view = mount(); const old = deferred(); const nextReply = deferred();
  api.sessionResolvePermission.mockReturnValueOnce(old.promise).mockReturnValueOnce(nextReply.promise);
  let first!: Promise<void>;
  act(() => { first = view.hook.result.current.resolveVoicePermission(prompt, "allow_once", "allow-once"); });
  const next = { ...prompt, requestGeneration: "next" };
  act(() => { view.hook.result.current.setPendingPermission(next); });
  expect(view.hook.result.current.resolvingPermission).toBe(false);
  let second!: Promise<void>;
  act(() => { second = view.hook.result.current.resolveVoicePermission(next, "deny", "reject-once"); });
  await act(async () => { if (outcome === "success") old.resolve(); else old.reject(new Error("late")); await first; });
  expect(view.hook.result.current.pendingPermission).toBe(next);
  expect(view.hook.result.current.resolvingPermission).toBe(true);
  expect(view.onResolved).not.toHaveBeenCalled(); expect(view.onFailure).not.toHaveBeenCalled();
  await act(async () => { nextReply.resolve(); await second; });
  expect(view.onResolved).toHaveBeenCalledExactlyOnceWith(next, "deny");
  expect(view.hook.result.current.pendingPermission).toBeNull();
  expect(view.hook.result.current.resolvingPermission).toBe(false);
  expect(api.sessionResolvePermission.mock.calls[0][0].requestGeneration).toBe(prompt.requestGeneration);
  expect(api.sessionResolvePermission.mock.calls[1][0].requestGeneration).toBe(next.requestGeneration);
});

test.each(["success", "failure"])("close and reopen cannot revive a previous voice reply: %s", async (outcome) => {
  const view = mount(); const pending = deferred(); api.sessionResolvePermission.mockReturnValue(pending.promise);
  let reply!: Promise<void>;
  act(() => { reply = view.hook.result.current.resolveVoicePermission(prompt, "allow_once", "allow-once"); });
  view.hook.rerender({ open: false }); view.hook.rerender({ open: true });
  act(() => { view.hook.result.current.setPendingPermission(prompt); });
  await act(async () => { if (outcome === "success") pending.resolve(); else pending.reject(new Error("late")); await reply; });
  expect(view.hook.result.current.pendingPermission).toBe(prompt);
  expect(view.hook.result.current.resolvingPermission).toBe(false);
  expect(view.onResolved).not.toHaveBeenCalled(); expect(view.onFailure).not.toHaveBeenCalled();
});

test("a current failure preserves retry and a duplicate click sends only one IPC", async () => {
  const view = mount(); const pending = deferred(); api.sessionResolvePermission.mockReturnValueOnce(pending.promise);
  let reply!: Promise<void>;
  act(() => { reply = view.hook.result.current.resolveVoicePermission(prompt, "deny", "reject-once"); });
  await act(async () => { await view.hook.result.current.resolveVoicePermission(prompt, "deny", "reject-once"); });
  expect(api.sessionResolvePermission).toHaveBeenCalledTimes(1);
  const error = new Error("writer closed");
  await act(async () => { pending.reject(error); await reply; });
  expect(view.onFailure).toHaveBeenCalledExactlyOnceWith(prompt, error);
  expect(view.hook.result.current.pendingPermission).toBe(prompt);
  expect(view.hook.result.current.resolvingPermission).toBe(false);
  await act(async () => { await view.hook.result.current.resolveVoicePermission(prompt, "allow_once", "allow-once"); });
  expect(api.sessionResolvePermission).toHaveBeenCalledTimes(2);
  expect(view.hook.result.current.pendingPermission).toBeNull();
});

test("a resolved callback that synchronously raises another prompt cannot clear it", async () => {
  const view = mount(); const next = { ...prompt, requestGeneration: "next" };
  view.onResolved.mockImplementation(() => { view.hook.result.current.setPendingPermission(next); });
  await act(async () => { await view.hook.result.current.resolveVoicePermission(prompt, "deny", "reject-once"); });
  expect(view.hook.result.current.pendingPermission).toBe(next);
  expect(view.hook.result.current.resolvingPermission).toBe(false);
});

test("unmounted overlay does not emit late reply effects", async () => {
  const view = mount(); const pending = deferred(); api.sessionResolvePermission.mockReturnValue(pending.promise);
  let reply!: Promise<void>;
  act(() => { reply = view.hook.result.current.resolveVoicePermission(prompt, "deny", "reject-once"); });
  view.hook.unmount(); pending.resolve(); await reply;
  expect(view.onResolved).not.toHaveBeenCalled(); expect(view.onFailure).not.toHaveBeenCalled();
});
