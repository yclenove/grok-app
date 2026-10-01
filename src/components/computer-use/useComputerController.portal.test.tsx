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

const idle: api.ComputerStatus = {
  featureEnabled: true, enabled: false, runId: null, targetId: null, targetName: null,
  paused: false, stopState: "stopped", backend: "wayland-portal", desktopSelection: "portal",
  notes: [], traces: [], recovery: null, timings: null, targetAlive: false, mcpCatalog: null,
};
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (cause: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
beforeEach(() => {
  vi.resetAllMocks(); vi.useFakeTimers();
  vi.mocked(api.computerStatus).mockResolvedValue(idle);
  vi.mocked(api.computerListTargets).mockResolvedValue([]);
  vi.mocked(api.computerListSharedTabs).mockResolvedValue([]);
  vi.mocked(api.computerStop).mockResolvedValue();
  vi.mocked(api.computerCancelAuthorization).mockResolvedValue(true);
  vi.mocked(system.sideBrowserList).mockResolvedValue([]);
});
afterEach(() => { cleanup(); vi.useRealTimers(); });

describe("Host-selected native system consent", () => {
  it("discovery and refresh cannot open consent or publish a fictional candidate", async () => {
    const { result } = renderHook(() => useComputerController("chat", null, "desktop"));
    await act(async () => {});
    expect(result.current.usesSystemPicker).toBe(true);
    expect(result.current.targets).toEqual([]);
    expect(result.current.candidateId).toBe("");
    act(() => result.current.refreshTargets());
    await act(async () => { await vi.advanceTimersByTimeAsync(800); });
    expect(api.computerListTargets).not.toHaveBeenCalled();
    expect(api.computerAuthorizeSurface).not.toHaveBeenCalled();
    expect(result.current.loadingTargets).toBe(false);
  });

  it("only an explicit action starts a revision-owned picker, with no caller target", async () => {
    const pending = deferred<api.ComputerAuthorization>();
    vi.mocked(api.computerAuthorizeSurface).mockReturnValue(pending.promise);
    const { result } = renderHook(() => useComputerController("chat", null, "desktop"));
    await act(async () => {});
    // Even a stale renderer candidate is never passed as native authority.
    act(() => result.current.setCandidateId("forged-monitor"));
    let authorization!: Promise<boolean | undefined>;
    act(() => { authorization = result.current.authorize(); });
    const request = vi.mocked(api.computerAuthorizeSurface).mock.calls[0][0];
    expect(request).toMatchObject({ sessionId: "chat", runId: null, surface: "desktop", targetId: "", selectorRevision: 1 });
    expect(result.current.authorizing).toBe(true);
    await act(async () => { await result.current.authorize(); });
    expect(api.computerAuthorizeSurface).toHaveBeenCalledTimes(1);
    vi.mocked(api.computerStatus).mockResolvedValue({ ...idle, enabled: true, runId: "native-run", targetId: "wayland:owned", targetAlive: true, stopState: "running" });
    await act(async () => {
      pending.resolve({ attemptId: request.attemptId, selectorRevision: request.selectorRevision, runId: "native-run", target: { targetId: "wayland:owned", title: "System selected screen", appName: "", kind: "monitor" } });
      expect(await authorization).toBe(true);
    });
    expect(result.current.activeRun).toBe("native-run");
    expect(result.current.status?.targetId).toBe("wayland:owned");
    expect(api.computerStatus).toHaveBeenLastCalledWith({ sessionId: "chat", runId: "native-run" });
  });

  it("Stop fences a pending picker without waiting for cancellation and ignores its late grant", async () => {
    const pending = deferred<api.ComputerAuthorization>();
    vi.mocked(api.computerAuthorizeSurface).mockReturnValue(pending.promise);
    vi.mocked(api.computerCancelAuthorization).mockReturnValue(new Promise(() => {}));
    const { result } = renderHook(() => useComputerController("chat", null, "desktop"));
    await act(async () => {});
    let authorization!: Promise<boolean | undefined>;
    act(() => { authorization = result.current.authorize(); });
    const request = vi.mocked(api.computerAuthorizeSurface).mock.calls[0][0];
    await act(async () => { await result.current.stop(); });
    expect(api.computerStop).toHaveBeenCalledExactlyOnceWith({ sessionId: "chat", runId: null });
    expect(api.computerCancelAuthorization).toHaveBeenCalledExactlyOnceWith({ sessionId: "chat", attemptId: request.attemptId, selectorRevision: 1 });
    await act(async () => {
      pending.resolve({ ...request, runId: "late-run", target: { targetId: "wayland:late", title: "Late", appName: "", kind: "monitor" } });
      await authorization;
    });
    expect(result.current.activeRun).toBeNull();
    expect(result.current.status?.targetId).toBeNull();
    expect(result.current.stopping).toBe(false);
  });

  it("unmount cancels the exact attempt without adopting a late error", async () => {
    const pending = deferred<api.ComputerAuthorization>();
    vi.mocked(api.computerAuthorizeSurface).mockReturnValue(pending.promise);
    const { result, unmount } = renderHook(() => useComputerController("chat", null, "desktop"));
    await act(async () => {});
    let authorization!: Promise<boolean | undefined>;
    act(() => { authorization = result.current.authorize(); });
    const request = vi.mocked(api.computerAuthorizeSurface).mock.calls[0][0];
    unmount();
    expect(api.computerCancelAuthorization).toHaveBeenCalledExactlyOnceWith({ sessionId: "chat", attemptId: request.attemptId, selectorRevision: 1 });
    await act(async () => { pending.reject(new Error("cancelled portal")); await authorization; });
    expect(vi.getTimerCount()).toBe(0);
  });

  it.each(["managed-browser", "existing-tabs", "app-webview"] as const)("does not exempt %s from selecting a target", async surface => {
    const { result } = renderHook(() => useComputerController("chat", null, surface));
    await act(async () => {});
    expect(result.current.usesSystemPicker).toBe(false);
    await act(async () => { await result.current.authorize(); });
    expect(api.computerAuthorizeSurface).not.toHaveBeenCalled();
  });

  it.each(["off", "stale", "cleanup"] as const)("cannot open the picker when %s", async condition => {
    if (condition === "off") vi.mocked(api.computerStatus).mockResolvedValue({ ...idle, featureEnabled: false });
    if (condition === "cleanup") vi.mocked(api.computerStatus).mockResolvedValue({ ...idle, mcpCatalog: { desiredGeneration: 1, appliedGeneration: null, desiredPresent: false, pending: true, cleanupPending: true, lastError: null } });
    const { result } = renderHook(() => useComputerController("chat", null, "desktop"));
    await act(async () => {});
    if (condition === "stale") {
      vi.mocked(api.computerStatus).mockRejectedValue(new Error("status unavailable"));
      await act(async () => { await vi.advanceTimersByTimeAsync(800); });
    }
    await act(async () => { await result.current.authorize(); });
    expect(api.computerAuthorizeSurface).not.toHaveBeenCalled();
  });
});
