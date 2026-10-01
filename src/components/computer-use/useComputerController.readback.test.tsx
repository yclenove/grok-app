/** @vitest-environment jsdom */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "@/lib/api/computerUse";
import { useComputerController } from "./useComputerController";

vi.mock("@/lib/api/computerUse", () => ({
  computerStatus: vi.fn(), computerListTargets: vi.fn(), computerAuthorizeSurface: vi.fn(),
  computerCancelAuthorization: vi.fn(), computerStop: vi.fn(), computerRetryCleanup: vi.fn(),
}));
vi.mock("@/lib/api/system", () => ({ sideBrowserList: vi.fn() }));

const running: api.ComputerStatus = {
  runId: "run", targetId: "window", targetName: "Editor", featureEnabled: true,
  enabled: true, paused: false, stopState: "running", backend: "windows",
  notes: [], traces: [], recovery: null, timings: null, targetAlive: true, mcpCatalog: null,
};
const deferred = () => {
  let resolve!: () => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<void>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
};
const poll = () => act(async () => { await vi.advanceTimersByTimeAsync(800); });

beforeEach(() => {
  vi.resetAllMocks(); vi.useFakeTimers();
  vi.mocked(api.computerListTargets).mockResolvedValue([]);
});
afterEach(() => { cleanup(); vi.useRealTimers(); });

async function start(status: api.ComputerStatus = running) {
  const stop = deferred();
  vi.mocked(api.computerStatus).mockResolvedValue(status);
  vi.mocked(api.computerStop).mockReturnValue(stop.promise);
  const view = renderHook(() => useComputerController("chat", null, "desktop"));
  await act(async () => {});
  act(() => { void view.result.current.stop(); });
  expect(view.result.current.stopping).toBe(true);
  return { ...view, stop };
}

describe("controller Stop readback", () => {
  it.each(["resolve", "reject"] as const)("retires the old %s reply when a same-run poll confirms native Stop", async reply => {
    const view = await start();
    vi.mocked(api.computerStatus).mockResolvedValue({ ...running, enabled: false, stopState: "stopped" });
    await poll();
    expect(view.result.current.stopping).toBe(false);
    expect(view.result.current.fresh).toBe(true);
    expect(view.result.current.controlsLocked).toBe(false);
    const reads = vi.mocked(api.computerStatus).mock.calls.length;
    await act(async () => {
      if (reply === "resolve") view.stop.resolve();
      else view.stop.reject(new Error("late transport failure"));
    });
    expect(view.result.current.error).toBeNull();
    expect(view.result.current.fresh).toBe(true);
    expect(api.computerStatus).toHaveBeenCalledTimes(reads);
    expect(api.computerStop).toHaveBeenCalledExactlyOnceWith({ sessionId: "chat", runId: "run" });
  });

  it("unblocks MCP cleanup without reporting tool removal complete", async () => {
    const view = await start();
    const catalog: api.ComputerMcpCatalogStatus = {
      desiredGeneration: 3, appliedGeneration: 2, desiredPresent: false,
      pending: true, cleanupPending: true, lastError: "cleanup timeout",
    };
    vi.mocked(api.computerStatus).mockResolvedValue({ ...running,
      enabled: false, stopState: "stopped", mcpCatalog: catalog,
    });
    await poll();
    expect(view.result.current.stopping).toBe(false);
    expect(view.result.current.cleanupPending).toBe(true);
    expect(view.result.current.controlsLocked).toBe(true);
    vi.mocked(api.computerRetryCleanup).mockImplementation(() => new Promise(() => {}));
    act(() => { void view.result.current.retryCleanup(); });
    expect(api.computerRetryCleanup).toHaveBeenCalledExactlyOnceWith({ sessionId: "chat" });
    await act(async () => { view.stop.reject(new Error("late transport failure")); });
    expect(view.result.current.retryingCleanup).toBe(true);
    expect(view.result.current.error).toBeNull();
  });

  it.each(["stop_requested", "running"] as const)("retains pending Stop for Host %s readback", async stopState => {
    const view = await start();
    vi.mocked(api.computerStatus).mockResolvedValue({ ...running, stopState });
    await poll();
    expect(view.result.current.stopping).toBe(true);
    expect(view.result.current.controlsLocked).toBe(true);
    act(() => { void view.result.current.stop(); });
    expect(api.computerStop).toHaveBeenCalledTimes(1);
  });

  it("does not take an empty idle readback as confirmation of a session-scoped Stop", async () => {
    const idle = { ...running, runId: null, targetId: null, targetName: null,
      enabled: false, stopState: "stopped" as const, targetAlive: false,
    };
    const view = await start(idle);
    await poll();
    expect(view.result.current.stopping).toBe(true);
    expect(api.computerStop).toHaveBeenCalledExactlyOnceWith({ sessionId: "chat", runId: null });
    await act(async () => { view.stop.resolve(); });
    expect(view.result.current.stopping).toBe(false);
  });

  it("never retires a pending exact-run Stop from a different stopped run", async () => {
    const view = await start();
    vi.mocked(api.computerStatus).mockResolvedValue({ ...running, runId: "other", stopState: "stopped" });
    await poll();
    expect(view.result.current.stopping).toBe(true);
    expect(view.result.current.controlsLocked).toBe(true);
    expect(api.computerStop).toHaveBeenCalledTimes(1);
  });

  it("does not settle Stop from a status read started before that request", async () => {
    let finishRead!: (status: api.ComputerStatus) => void;
    vi.mocked(api.computerStatus).mockResolvedValueOnce(running)
      .mockImplementation(() => new Promise(resolve => { finishRead = resolve; }));
    vi.mocked(api.computerStop).mockReturnValue(deferred().promise);
    const view = renderHook(() => useComputerController("chat", null, "desktop"));
    await act(async () => {});
    await poll();
    expect(api.computerStatus).toHaveBeenCalledTimes(2);
    act(() => { void view.result.current.stop(); });
    await act(async () => { finishRead({ ...running, enabled: false, stopState: "stopped" }); });
    expect(view.result.current.stopping).toBe(true);
    expect(view.result.current.fresh).toBe(false);
    expect(view.result.current.status?.stopState).toBe("running");
    expect(api.computerStop).toHaveBeenCalledTimes(1);
  });

  it("keeps an explicit retry pending when the retired request later rejects", async () => {
    const view = await start();
    vi.mocked(api.computerStatus).mockResolvedValue({ ...running, enabled: false, stopState: "stopped" });
    await poll();
    vi.mocked(api.computerStatus).mockRejectedValue(new Error("readback unavailable"));
    await poll();
    expect(view.result.current.fresh).toBe(false);
    const retry = deferred();
    vi.mocked(api.computerStop).mockReturnValue(retry.promise);
    act(() => { void view.result.current.stop(); });
    const reads = vi.mocked(api.computerStatus).mock.calls.length;
    await act(async () => { view.stop.reject(new Error("retired reply")); });
    expect(view.result.current.stopping).toBe(true);
    expect(view.result.current.error).toBeNull();
    expect(api.computerStatus).toHaveBeenCalledTimes(reads);
    expect(api.computerStop).toHaveBeenCalledTimes(2);
    expect(api.computerStop).toHaveBeenLastCalledWith({ sessionId: "chat", runId: "run" });
  });
});
