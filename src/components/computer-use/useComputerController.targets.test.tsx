/** @vitest-environment jsdom */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "@/lib/api/computerUse";
import * as system from "@/lib/api/system";
import { useComputerController } from "./useComputerController";

vi.mock("@/lib/api/computerUse", () => ({
  computerStatus: vi.fn(), computerListTargets: vi.fn(), computerListSharedTabs: vi.fn(),
  computerAuthorizeSurface: vi.fn(), computerCancelAuthorization: vi.fn(), computerStop: vi.fn(),
}));
vi.mock("@/lib/api/system", () => ({ sideBrowserList: vi.fn() }));

const state: api.ComputerStatus = {
  featureEnabled: true, enabled: true, runId: "run", targetId: "window", targetName: "Editor",
  paused: false, stopState: "running", backend: "test", notes: [], traces: [],
  recovery: null, timings: null, targetAlive: true, mcpCatalog: null,
};
const target: api.ComputerTarget = { targetId: "window", title: "Editor", appName: "Editor", kind: "window" };
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (cause: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const advance = (ms: number) => act(async () => { await vi.advanceTimersByTimeAsync(ms); });

beforeEach(() => {
  vi.resetAllMocks(); vi.useFakeTimers();
  vi.mocked(api.computerStatus).mockResolvedValue(state);
  vi.mocked(api.computerListTargets).mockResolvedValue([target]);
  vi.mocked(api.computerListSharedTabs).mockResolvedValue([]);
  vi.mocked(api.computerStop).mockResolvedValue();
  vi.mocked(system.sideBrowserList).mockResolvedValue([{ label: "view" }]);
});
afterEach(() => { cleanup(); vi.useRealTimers(); });

describe("target discovery reply ownership", () => {
  it.each(["desktop", "managed-browser", "existing-tabs", "app-webview"] as const)(
    "%s has a bounded loading state without blocking current-run Stop", async surface => {
      vi.mocked(api.computerListTargets).mockImplementation(() => new Promise(() => {}));
      vi.mocked(system.sideBrowserList).mockImplementation(() => new Promise(() => {}));
      const { result } = renderHook(() => useComputerController("chat", "run", surface));
      await act(async () => {});
      expect(result.current.loadingTargets).toBe(true);
      await advance(9_999);
      expect(result.current.loadingTargets).toBe(true);
      await advance(1);
      expect(result.current.loadingTargets).toBe(false);
      expect(result.current.targetError).toBeTruthy();
      expect(result.current.targets).toEqual([]);
      expect(result.current.controlsLocked).toBe(false);
      await act(async () => { await result.current.stop(); });
      expect(api.computerStop).toHaveBeenCalledExactlyOnceWith({ sessionId: "chat", runId: "run" });
      expect(api.computerAuthorizeSurface).not.toHaveBeenCalled();
    },
  );

  it.each(["resolve", "reject"] as const)("ignores a timed-out request's late %s during a new attempt", async reply => {
    const old = deferred<api.ComputerTarget[]>();
    const current = deferred<api.ComputerTarget[]>();
    const { result } = renderHook(() => useComputerController("chat", "run", "desktop"));
    await act(async () => {});
    act(() => result.current.setCandidateId(target.targetId));
    vi.mocked(api.computerListTargets).mockReturnValueOnce(old.promise).mockReturnValueOnce(current.promise);
    act(() => result.current.refreshTargets());
    await advance(10_000);
    expect(result.current.candidateId).toBe("");
    expect(result.current.loadingTargets).toBe(false);
    expect(result.current.targetError).toBeTruthy();
    act(() => result.current.refreshTargets());
    expect(result.current.loadingTargets).toBe(true);
    await act(async () => {
      if (reply === "resolve") old.resolve([target]);
      else old.reject(new Error("late discovery failure"));
    });
    expect(result.current.loadingTargets).toBe(true);
    expect(result.current.targets).toEqual([]);
    expect(result.current.targetError).toBeNull();
    const replacement = { ...target, targetId: "new-window", title: "New editor" };
    await act(async () => current.resolve([replacement]));
    expect(result.current.loadingTargets).toBe(false);
    expect(result.current.targets).toEqual([replacement]);
    expect(result.current.candidateId).toBe("");
    expect(result.current.targetError).toBeNull();
    await advance(10_000);
    expect(result.current.targets).toEqual([replacement]);
    expect(result.current.targetError).toBeNull();
    expect(api.computerAuthorizeSurface).not.toHaveBeenCalled();
  });

  it("cannot revive an expired list even before a retry starts", async () => {
    const old = deferred<api.ComputerTarget[]>();
    vi.mocked(api.computerListTargets).mockReturnValueOnce(old.promise);
    const { result } = renderHook(() => useComputerController("chat", "run", "desktop"));
    await act(async () => {});
    await advance(10_000);
    await act(async () => old.resolve([target]));
    expect(result.current.targets).toEqual([]);
    expect(result.current.targetError).toBeTruthy();
    expect(result.current.loadingTargets).toBe(false);
  });

  it("retires the discovery deadline on unmount without authorizing or cancelling an action", async () => {
    const old = deferred<api.ComputerTarget[]>();
    vi.mocked(api.computerListTargets).mockReturnValueOnce(old.promise);
    const { unmount } = renderHook(() => useComputerController("chat", "run", "desktop"));
    await act(async () => {});
    unmount();
    expect(vi.getTimerCount()).toBe(0);
    await act(async () => old.reject(new Error("late reply after unmount")));
    expect(api.computerAuthorizeSurface).not.toHaveBeenCalled();
    expect(api.computerCancelAuthorization).not.toHaveBeenCalled();
    expect(api.computerStop).not.toHaveBeenCalled();
  });
});
