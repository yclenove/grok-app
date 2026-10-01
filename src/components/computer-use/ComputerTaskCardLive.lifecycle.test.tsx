/** @vitest-environment jsdom */
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import * as api from "@/lib/api/computerUse";
import { ComputerTaskCardLive } from "./ComputerTaskCardLive";

vi.mock("@/lib/api/computerUse", () => ({
  computerStatus: vi.fn(), computerPause: vi.fn(), computerStop: vi.fn(),
}));

const initial = (): api.ComputerStatus => ({
  runId: "run", targetId: "window", targetName: "Editor", featureEnabled: true,
  enabled: true, paused: false, stopState: "running", backend: "windows",
  notes: [], traces: [], recovery: null, timings: null, targetAlive: true, mcpCatalog: null,
});
const deferred = () => {
  let resolve!: () => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<void>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
};
const poll = () => act(async () => { await vi.advanceTimersByTimeAsync(800); });

beforeEach(() => { vi.resetAllMocks(); vi.useFakeTimers(); });
afterEach(() => { cleanup(); vi.useRealTimers(); });

async function start() {
  let status = initial();
  const stop = deferred();
  vi.mocked(api.computerStatus).mockImplementation(async () => status);
  vi.mocked(api.computerStop).mockReturnValue(stop.promise);
  const view = render(<ComputerTaskCardLive locale="en" sessionId="chat" />);
  await act(async () => {});
  fireEvent.click(screen.getByRole("button", { name: "Stop" }));
  expect(api.computerStop).toHaveBeenCalledExactlyOnceWith({ sessionId: "chat", runId: "run" });
  return { ...view, stop, setStatus: (next: api.ComputerStatus) => { status = next; } };
}

describe("task card control reply ordering", () => {
  it("isolates a replacement run from a pending old Stop and its late error", async () => {
    const view = await start();
    view.setStatus({ ...initial(), runId: "replacement", targetName: "New editor" });
    await poll();
    expect(screen.getByRole("button", { name: "Stop" })).toBeEnabled();
    const next = deferred();
    vi.mocked(api.computerStop).mockReturnValue(next.promise);
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(api.computerStop).toHaveBeenLastCalledWith({ sessionId: "chat", runId: "replacement" });
    await act(async () => { view.stop.reject(new Error("obsolete reply")); });
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByRole("button", { name: "Stopping…" })).toBeDisabled();
    expect(api.computerStop).toHaveBeenCalledTimes(2);
    await act(async () => { next.resolve(); });
  });

  it("settles the button from an exact-run Host stop even if the reply never arrives", async () => {
    const view = await start();
    view.setStatus({ ...initial(), enabled: false, stopState: "stopped" });
    await poll();
    expect(screen.getByText("Stopped")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Stop" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Stopping…" })).toBeNull();
    expect(api.computerStop).toHaveBeenCalledTimes(1);
  });

  it("ignores a late Stop transport error after the same Host run has stopped", async () => {
    const view = await start();
    view.setStatus({ ...initial(), enabled: false, stopState: "stopped" });
    await poll();
    await act(async () => { view.stop.reject(new Error("late transport failure")); });
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByText("Stopped")).toBeInTheDocument();
  });

  it("keeps tool cleanup distinct from physical Stop while a reply is pending", async () => {
    const view = await start();
    view.setStatus({ ...initial(), enabled: false, stopState: "stopped", mcpCatalog: {
      desiredGeneration: 3, appliedGeneration: 2, desiredPresent: false,
      pending: true, cleanupPending: true, lastError: "cleanup timed out",
    } });
    await poll();
    expect(screen.getByText("Removing Computer Use tools…")).toBeInTheDocument();
    expect(screen.queryByText("Stopped")).toBeNull();
    expect(screen.getByRole("button", { name: "Stop" })).toBeDisabled();
    await act(async () => { view.stop.reject(new Error("late transport failure")); });
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("does not treat a Stop acknowledgement or stale readback as native completion", async () => {
    const view = await start();
    view.setStatus({ ...initial(), enabled: false, stopState: "stop_requested" });
    await poll();
    await act(async () => { view.stop.resolve(); });
    expect(screen.getByRole("button", { name: "Stopping…" })).toBeDisabled();
    expect(screen.queryByText("Stopped")).toBeNull();
    vi.mocked(api.computerStatus).mockRejectedValue(new Error("readback lost"));
    await poll();
    expect(screen.getByText(/Status unknown/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Stop" })).toBeEnabled();
    expect(api.computerStop).toHaveBeenCalledTimes(1);
  });

  it("preserves a Host action failure after stopping instead of hiding all errors", async () => {
    const view = await start();
    view.setStatus({ ...initial(), enabled: false, stopState: "stopped", traces: [{
      kind: "act", runId: "run", detail: "click Rejected executed=false", ms: 12, audience: "ui",
    }] });
    await poll();
    expect(screen.getByRole("alert")).toHaveTextContent("Could not run that action.");
    await act(async () => { view.stop.resolve(); });
    expect(screen.getByRole("alert")).toHaveTextContent("Could not run that action.");
  });
});
