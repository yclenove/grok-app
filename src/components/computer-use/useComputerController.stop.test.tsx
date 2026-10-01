/** @vitest-environment jsdom */
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "@/lib/api/computerUse";
import { useComputerController } from "./useComputerController";

vi.mock("@/lib/api/computerUse", () => ({
  computerStatus: vi.fn(), computerListTargets: vi.fn(), computerAuthorizeSurface: vi.fn(),
  computerCancelAuthorization: vi.fn(), computerStop: vi.fn(),
}));
vi.mock("@/lib/api/system", () => ({ sideBrowserList: vi.fn() }));

const idle: api.ComputerStatus = {
  featureEnabled: true, enabled: false, runId: null, targetId: null, targetName: null,
  paused: false, stopState: "stopped", backend: "test", notes: [], traces: [],
  recovery: null, timings: null, targetAlive: false, mcpCatalog: null,
};
const target = { targetId: "window", title: "Editor", appName: "Editor", kind: "window" };
const deferred = <T,>() => {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
};

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(api.computerStatus).mockResolvedValue(idle);
  vi.mocked(api.computerListTargets).mockResolvedValue([target]);
});
afterEach(cleanup);

async function pendingConsent() {
  const authorization = deferred<Awaited<ReturnType<typeof api.computerAuthorizeSurface>>>();
  const cancellation = deferred<boolean>();
  vi.mocked(api.computerAuthorizeSurface).mockReturnValue(authorization.promise);
  vi.mocked(api.computerCancelAuthorization).mockReturnValue(cancellation.promise);
  const view = renderHook(() => useComputerController("chat", null, "desktop"));
  await waitFor(() => expect(view.result.current.targets).toHaveLength(1));
  act(() => view.result.current.setCandidateId("window"));
  act(() => { void view.result.current.authorize(); });
  expect(view.result.current.authorizing).toBe(true);
  const attempt = vi.mocked(api.computerAuthorizeSurface).mock.calls[0][0];
  return { ...view, authorization, cancellation, attempt };
}

describe("Stop priority over ancillary authorization cancellation", () => {
  it.each(["acknowledged", "failed"])("releases Stop after its own %s reply even if cancellation never replies", async outcome => {
    const view = await pendingConsent();
    const stopped = deferred<void>();
    vi.mocked(api.computerStop).mockReturnValueOnce(stopped.promise).mockResolvedValue();
    vi.mocked(api.computerStatus).mockReturnValue(new Promise(() => {}));
    act(() => { void view.result.current.stop(); void view.result.current.stop(); });
    expect(api.computerStop).toHaveBeenCalledTimes(1);
    expect(api.computerStop).toHaveBeenCalledWith({ sessionId: "chat", runId: null });
    expect(api.computerCancelAuthorization).toHaveBeenCalledWith({
      sessionId: "chat", attemptId: view.attempt.attemptId, selectorRevision: view.attempt.selectorRevision,
    });
    expect(view.result.current.stopping).toBe(true);
    await act(async () => {
      if (outcome === "failed") stopped.reject(new Error("Host disconnected"));
      else stopped.resolve();
    });
    await waitFor(() => expect(view.result.current.stopping).toBe(false));
    expect(view.result.current.authorizing).toBe(false);
    expect(view.result.current.fresh).toBe(false);
    expect(view.result.current.controlsLocked).toBe(true);
    if (outcome === "failed") expect(view.result.current.error).toContain("Host disconnected");
    await act(async () => { void view.result.current.stop(); });
    expect(api.computerStop).toHaveBeenCalledTimes(2);

    const reads = vi.mocked(api.computerStatus).mock.calls.length;
    await act(async () => {
      view.authorization.resolve({ attemptId: view.attempt.attemptId,
        selectorRevision: view.attempt.selectorRevision, runId: "late-run", target });
      view.cancellation.reject(new Error("late auxiliary failure"));
    });
    expect(view.result.current.activeRun).toBe(null);
    expect(api.computerStatus).toHaveBeenCalledTimes(reads);
    expect(view.result.current.error ?? "").not.toContain("late auxiliary failure");
  });

  it("retains Host stop_requested after Stop acknowledgement", async () => {
    const view = await pendingConsent();
    vi.mocked(api.computerStop).mockResolvedValue();
    vi.mocked(api.computerStatus).mockResolvedValue({ ...idle, runId: "run", stopState: "stop_requested" });
    act(() => { void view.result.current.stop(); });
    await waitFor(() => expect(view.result.current.status?.stopState).toBe("stop_requested"));
    expect(view.result.current.stopping).toBe(false);
    expect(view.result.current.controlsLocked).toBe(true);
    expect(view.result.current.status?.stopState).not.toBe("stopped");
  });
});
