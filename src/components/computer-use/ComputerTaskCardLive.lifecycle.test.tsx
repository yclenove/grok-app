/** @vitest-environment jsdom */
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { deferred, key, runningView, setupTaskUi } from "./taskUiTestSupport";
import * as taskApi from "@/lib/api/computerTask";
import { ComputerTaskCardLive } from "./ComputerTaskCardLive";

let ui: ReturnType<typeof setupTaskUi>;
beforeEach(() => { ui = setupTaskUi(); vi.useFakeTimers(); });
afterEach(() => { cleanup(); vi.useRealTimers(); });
async function start() {
  const stop = deferred<Awaited<ReturnType<typeof taskApi.computerTaskDecide>>>();
  vi.mocked(taskApi.computerTaskDecide).mockReturnValue(stop.promise);
  const view = render(<ComputerTaskCardLive locale="en" sessionId="chat" />);
  await act(async () => {}); await ui.emit(runningView());
  fireEvent.click(screen.getByRole("button", { name: "Stop" }));
  expect(taskApi.computerTaskDecide).toHaveBeenCalledExactlyOnceWith("chat", key, "stop");
  return { ...view, stop };
}
function stopped() {
  const host = runningView();
  host.executions[0].phase = "stopped"; host.grants[0].executable = false;
  return host;
}

describe("task card control reply ordering", () => {
  it("isolates a replacement task from an old Stop and its late error", async () => {
    const view = await start();
    const next = runningView();
    const nextKey = { ...key, taskId: "replacement-task", requestId: "replacement-request" };
    next.taskId = nextKey.taskId;
    next.request = { ...next.request!, key: nextKey, purpose: "Replacement editor" };
    next.executions[0] = { ...next.executions[0], key: nextKey, runId: "replacement-run" };
    next.grants[0] = { ...next.grants[0], key: nextKey, runId: "replacement-run" };
    await ui.emit(next);
    expect(screen.getByRole("button", { name: "Stop" })).toBeEnabled();
    const stop = deferred<Awaited<ReturnType<typeof taskApi.computerTaskDecide>>>();
    vi.mocked(taskApi.computerTaskDecide).mockReturnValue(stop.promise);
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(taskApi.computerTaskDecide).toHaveBeenLastCalledWith("chat", nextKey, "stop");
    await act(async () => { view.stop.reject(new Error("obsolete reply")); });
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByRole("button", { name: "Stopping…" })).toBeDisabled();
    await act(async () => { stop.resolve({ sessionId: "chat", key: nextKey, decision: "stop" }); });
  });

  it("settles from exact Host native completion even if its reply never arrives", async () => {
    await start(); await ui.emit(stopped());
    expect(screen.getByText("Stopped")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Stop" })).toBeDisabled();
    expect(taskApi.computerTaskDecide).toHaveBeenCalledTimes(1);
  });

  it("ignores a late transport failure after exact native completion", async () => {
    const view = await start(); await ui.emit(stopped());
    await act(async () => { view.stop.reject(new Error("late failure")); });
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByText("Stopped")).toBeInTheDocument();
  });

  it("keeps native cleanup separate from fixed request-tool catalog availability", async () => {
    await start();
    const host = runningView();
    host.executions[0].phase = "stop_requested"; host.executions[0].cleanupPending = true;
    host.grants = [];
    await ui.emit(host);
    expect(screen.queryByText("Stopped")).toBeNull();
    expect(screen.getByRole("button", { name: "Stopping…" })).toBeDisabled();
    await ui.emit(stopped());
    expect(screen.getByText("Stopped")).toBeInTheDocument();
    // The request catalog intentionally stays present after task Stop.
    expect(screen.queryByText("Removing Computer Use tools…")).toBeNull();
  });

  it("treats neither acknowledgement nor stale state as native completion", async () => {
    const view = await start();
    const host = runningView(); host.executions[0].phase = "stop_requested";
    await ui.emit(host);
    await act(async () => { view.stop.resolve({ sessionId: "chat", key, decision: "stop" }); });
    expect(screen.getByRole("button", { name: "Stopping…" })).toBeDisabled();
    expect(screen.queryByText("Stopped")).toBeNull();
    await act(async () => { await vi.advanceTimersByTimeAsync(4001); });
    expect(screen.getByText(/Status unknown/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Stop" })).toBeEnabled();
  });

  it("retains an action failure after Stop and ignores the late control error", async () => {
    const view = await start();
    const host = stopped(); host.executions[0].lastFailure = "action_rejected";
    await ui.emit(host);
    expect(screen.getByRole("alert")).toHaveTextContent("Could not run that action.");
    await act(async () => { view.stop.reject(new Error("obsolete control error")); });
    expect(screen.getByRole("alert")).toHaveTextContent("Could not run that action.");
  });
});
