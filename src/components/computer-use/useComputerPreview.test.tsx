/** @vitest-environment jsdom */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import * as api from "@/lib/api/computerUse";
import { useComputerPreview } from "./useComputerPreview";

vi.mock("@/lib/api/computerUse", () => ({
  computerObserve: vi.fn(),
  computerSetPreview: vi.fn(),
}));

const frame = (snapshotId: string): api.ComputerObservation => ({
  snapshotId, geometryRevision: 1, previewDataUrl: `data:image/png;base64,${snapshotId}`,
});

beforeEach(() => {
  vi.resetAllMocks();
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval", "Date", "performance"] });
  Object.defineProperty(document, "hidden", { configurable: true, value: false });
  vi.mocked(api.computerSetPreview).mockResolvedValue();
});

afterEach(() => { cleanup(); vi.useRealTimers(); });

it("keeps a skipped busy frame stale without showing an error, then refreshes", async () => {
  vi.mocked(api.computerObserve)
    .mockResolvedValueOnce(frame("first"))
    .mockResolvedValueOnce(null)
    .mockResolvedValue(frame("next"));
  const view = renderHook(() => useComputerPreview("session", "run", "target", true));
  await act(async () => {});
  expect(view.result.current.frame?.snapshotId).toBe("first");
  expect(view.result.current.stale).toBe(false);
  await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
  expect(view.result.current).toEqual({ frame: frame("first"), stale: true, error: null });
  await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
  expect(view.result.current).toEqual({ frame: frame("next"), stale: false, error: null });
});

it("does not turn real adapter errors into busy or stop retrying after failure", async () => {
  vi.mocked(api.computerObserve)
    .mockRejectedValueOnce(new Error("capture failed"))
    .mockResolvedValueOnce(null)
    .mockResolvedValue(frame("recovered"));
  const view = renderHook(() => useComputerPreview("session", "run", "target", true));
  await act(async () => {});
  expect(view.result.current.error).toContain("capture failed");
  await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
  expect(view.result.current).toEqual({ frame: null, stale: true, error: null });
  await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
  expect(view.result.current.frame?.snapshotId).toBe("recovered");
  expect(view.result.current.stale).toBe(false);
});

it("fences a late capture after hiding and stops the next poll", async () => {
  let finish!: (value: api.ComputerObservation | null) => void;
  vi.mocked(api.computerObserve).mockImplementation(() => new Promise((resolve) => { finish = resolve; }));
  const view = renderHook(() => useComputerPreview("session", "run", "target", true));
  await act(async () => {});
  await act(async () => {
    Object.defineProperty(document, "hidden", { configurable: true, value: true });
    document.dispatchEvent(new Event("visibilitychange"));
    finish(frame("late"));
  });
  await act(async () => { await vi.advanceTimersByTimeAsync(5000); });
  expect(view.result.current.frame).toBeNull();
  expect(api.computerObserve).toHaveBeenCalledTimes(1);
  expect(api.computerSetPreview).toHaveBeenLastCalledWith({ sessionId: "session", runId: "run", visible: false });
});

it("ignores a busy response from the old run after switching targets", async () => {
  let finish!: (value: api.ComputerObservation | null) => void;
  vi.mocked(api.computerObserve)
    .mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }))
    .mockResolvedValue(frame("new-run"));
  const view = renderHook(({ run }) => useComputerPreview("session", run, "target", true), {
    initialProps: { run: "old" },
  });
  await act(async () => {});
  view.rerender({ run: "new" });
  await act(async () => { finish(null); });
  expect(view.result.current).toEqual({ frame: frame("new-run"), stale: false, error: null });
});

it("removes another target's image and error synchronously, retaining only same-target stale frames", async () => {
  vi.mocked(api.computerObserve).mockResolvedValue(frame("private-old-target"));
  const view = renderHook(({ target, active }) => useComputerPreview("session", "run", target, active), {
    initialProps: { target: "first", active: true },
  });
  await act(async () => {});
  view.rerender({ target: "first", active: false });
  expect(view.result.current.frame?.snapshotId).toBe("private-old-target");
  expect(view.result.current.stale).toBe(true);
  view.rerender({ target: "second", active: false });
  expect(view.result.current.frame).toBeNull();
  expect(view.result.current.error).toBeNull();
});

it("expires the last successful frame while the next capture hangs without starting parallel captures", async () => {
  let finish!: (value: api.ComputerObservation | null) => void;
  vi.mocked(api.computerObserve)
    .mockResolvedValueOnce(frame("last-known"))
    .mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
  const view = renderHook(() => useComputerPreview("session", "run", "target", true));
  await act(async () => {});
  expect(view.result.current.stale).toBe(false);
  await act(async () => { await vi.advanceTimersByTimeAsync(3001); });
  expect(view.result.current).toEqual({ frame: frame("last-known"), stale: true, error: null });
  await act(async () => { await vi.advanceTimersByTimeAsync(20000); });
  expect(api.computerObserve).toHaveBeenCalledTimes(2);
  // A very late frame may be shown as historical, not promoted to live.
  await act(async () => finish(frame("late-capture")));
  expect(view.result.current).toEqual({ frame: frame("late-capture"), stale: true, error: null });
});

it("does not promote an initially slow capture to live and recovers on a timely next capture", async () => {
  let finish!: (value: api.ComputerObservation | null) => void;
  vi.mocked(api.computerObserve)
    .mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }))
    .mockResolvedValue(frame("fresh"));
  const view = renderHook(() => useComputerPreview("session", "run", "target", true));
  await act(async () => {});
  await act(async () => { await vi.advanceTimersByTimeAsync(3001); finish(frame("slow")); });
  expect(view.result.current).toEqual({ frame: frame("slow"), stale: true, error: null });
  await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
  expect(view.result.current).toEqual({ frame: frame("fresh"), stale: false, error: null });
});

it("retires the previous frame's expiry when a newer capture succeeds", async () => {
  vi.mocked(api.computerObserve).mockResolvedValue(frame("current"));
  const view = renderHook(() => useComputerPreview("session", "run", "target", true));
  await act(async () => {});
  await act(async () => { await vi.advanceTimersByTimeAsync(3500); });
  expect(view.result.current.stale).toBe(false);
  expect(api.computerObserve).toHaveBeenCalledTimes(4);
  view.unmount();
  expect(vi.getTimerCount()).toBe(0);
});

it("marks a hidden frame stale before the visibility acknowledgement arrives", async () => {
  vi.mocked(api.computerObserve).mockResolvedValue(frame("before-hidden"));
  const view = renderHook(() => useComputerPreview("session", "run", "target", true));
  await act(async () => {});
  vi.mocked(api.computerSetPreview).mockImplementationOnce(() => new Promise(() => {}));
  await act(async () => {
    Object.defineProperty(document, "hidden", { configurable: true, value: true });
    document.dispatchEvent(new Event("visibilitychange"));
  });
  expect(view.result.current).toEqual({ frame: frame("before-hidden"), stale: true, error: null });
});

it("does not alias distinct identity tuples containing colons", async () => {
  vi.mocked(api.computerObserve).mockResolvedValue(frame("private-first"));
  const view = renderHook(({ session, run, active }) => useComputerPreview(session, run, "target", active), {
    initialProps: { session: "session:a", run: "run", active: true },
  });
  await act(async () => {});
  view.rerender({ session: "session", run: "a:run", active: false });
  expect(view.result.current.frame).toBeNull();
  expect(view.result.current.stale).toBe(true);
});

it("never starts capture or enables preview without an exact target", async () => {
  vi.mocked(api.computerObserve).mockResolvedValue(frame("unspecified"));
  renderHook(() => useComputerPreview("session", "run", "", true));
  await act(async () => {});
  expect(api.computerObserve).not.toHaveBeenCalled();
  expect(api.computerSetPreview).not.toHaveBeenCalled();
});

it("uses monotonic frame age despite a wall-clock change", async () => {
  vi.mocked(api.computerObserve)
    .mockResolvedValueOnce(frame("before-clock-change"))
    .mockImplementationOnce(() => new Promise(() => {}));
  const view = renderHook(() => useComputerPreview("session", "run", "target", true));
  await act(async () => {});
  vi.setSystemTime(Date.now() - 24 * 60 * 60 * 1000);
  await act(async () => { await vi.advanceTimersByTimeAsync(3001); });
  expect(view.result.current.stale).toBe(true);
  expect(api.computerObserve).toHaveBeenCalledTimes(2);
});

it("does not let an old identity's expiry or delayed frame age the replacement", async () => {
  let finish!: (value: api.ComputerObservation | null) => void;
  vi.mocked(api.computerObserve)
    .mockResolvedValueOnce(frame("old"))
    .mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }))
    .mockResolvedValue(frame("replacement"));
  const view = renderHook(({ run }) => useComputerPreview("session", run, "target", true), {
    initialProps: { run: "old" },
  });
  await act(async () => {});
  await act(async () => { await vi.advanceTimersByTimeAsync(2500); });
  view.rerender({ run: "new" });
  await act(async () => {});
  await act(async () => { await vi.advanceTimersByTimeAsync(501); finish(frame("late-old")); });
  expect(view.result.current).toEqual({ frame: frame("replacement"), stale: false, error: null });
});
