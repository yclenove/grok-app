// @vitest-environment jsdom
import { createElement, StrictMode } from "react";
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { isDesktopHost } from "@/lib/api/host";
import { invoke } from "@tauri-apps/api/core";
import { StreamCoalescer } from "@/lib/streamCoalesce";
import { useNativeSoakTelemetry } from "./useNativeSoakTelemetry";

vi.mock("@/lib/api/host", () => ({ isDesktopHost: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

async function settled() {
  await act(async () => { for (let i = 0; i < 12; i += 1) await Promise.resolve(); });
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.mocked(isDesktopHost).mockReturnValue(true);
  vi.mocked(invoke).mockResolvedValue(false);
  vi.stubGlobal("requestAnimationFrame", vi.fn(() => 7));
  vi.stubGlobal("cancelAnimationFrame", vi.fn());
  vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
});
afterEach(async () => {
  cleanup();
  await settled();
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  vi.clearAllMocks();
});

describe("native soak opt-in lifecycle", () => {
  it.each(["browser", "mirror"])("never invokes native telemetry in a %s surface", async () => {
    vi.mocked(isDesktopHost).mockReturnValue(false);
    renderHook(() => useNativeSoakTelemetry());
    await settled();
    expect(invoke).not.toHaveBeenCalled();
    expect(requestAnimationFrame).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);
  });

  it.each([false, null, "true", 1])("requires exactly boolean true, got %j", async (value) => {
    vi.mocked(invoke).mockResolvedValue(value);
    const { result, rerender } = renderHook(() => useNativeSoakTelemetry());
    const queue = new StreamCoalescer({ onFlush: vi.fn() });
    const observe = vi.spyOn(queue, "observeDiagnostics");
    result.current.attachStream(queue);
    await settled();
    rerender();
    expect(invoke).toHaveBeenCalledExactlyOnceWith("native_soak_telemetry_enabled");
    expect(observe).not.toHaveBeenCalled();
    expect(requestAnimationFrame).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);
    queue.dispose();
  });

  it("failed probe is inert and not retried on rerender", async () => {
    vi.mocked(invoke).mockRejectedValue(new Error("private error"));
    const { rerender } = renderHook(() => useNativeSoakTelemetry());
    await settled();
    rerender();
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(requestAnimationFrame).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("shares one probe and sampler through StrictMode and cleans all work", async () => {
    vi.mocked(invoke).mockImplementation(async (command) => command === "native_soak_telemetry_enabled" ? true : undefined);
    const { unmount } = renderHook(() => useNativeSoakTelemetry(), {
      wrapper: ({ children }) => createElement(StrictMode, null, children),
    });
    await settled();
    expect(invoke).toHaveBeenCalledExactlyOnceWith("native_soak_telemetry_enabled");
    expect(requestAnimationFrame).toHaveBeenCalledTimes(1);
    expect(vi.getTimerCount()).toBe(1);
    await act(async () => { vi.advanceTimersByTime(30_000); });
    await settled();
    expect(invoke).toHaveBeenLastCalledWith("native_soak_telemetry_sample", { sample: expect.objectContaining({ version: 1, sequence: 1 }) });
    unmount();
    expect(cancelAnimationFrame).toHaveBeenCalledWith(7);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("ignores an enable response arriving after unmount", async () => {
    let resolve!: (value: boolean) => void;
    vi.mocked(invoke).mockReturnValue(new Promise<boolean>((done) => { resolve = done; }));
    const { unmount } = renderHook(() => useNativeSoakTelemetry());
    await settled();
    unmount();
    resolve(true);
    await settled();
    expect(requestAnimationFrame).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("ignores an enable reply if the surface is no longer desktop", async () => {
    let resolve!: (value: boolean) => void;
    vi.mocked(invoke).mockReturnValue(new Promise<boolean>((done) => { resolve = done; }));
    renderHook(() => useNativeSoakTelemetry());
    await settled();
    vi.mocked(isDesktopHost).mockReturnValue(false);
    resolve(true);
    await settled();
    expect(requestAnimationFrame).not.toHaveBeenCalled();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("late enable observes existing queue gauges and never serializes content", async () => {
    let resolve!: (value: boolean) => void;
    vi.mocked(invoke).mockReturnValueOnce(new Promise<boolean>((done) => { resolve = done; })).mockResolvedValue(undefined);
    const { result } = renderHook(() => useNativeSoakTelemetry());
    const queue = new StreamCoalescer({ onFlush: vi.fn(), flushMs: 60_000 });
    result.current.attachStream(queue);
    queue.push({ sessionId: "private-id", text: "private-payload" });
    await settled();
    resolve(true);
    await settled();
    await act(async () => { vi.advanceTimersByTime(30_000); });
    await settled();
    const sample = vi.mocked(invoke).mock.calls.find(([command]) => command === "native_soak_telemetry_sample")?.[1];
    expect(sample).toMatchObject({ sample: { stream: { receivedChunks: 0, pendingKeys: 1, pendingTextCodeUnits: 15 } } });
    expect(JSON.stringify(sample)).not.toContain("private");
    queue.dispose();
  });
});
