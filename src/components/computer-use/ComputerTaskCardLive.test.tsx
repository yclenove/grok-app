/** @vitest-environment jsdom */
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { address, deferred, key, pendingView, runningView, setupTaskUi } from "./taskUiTestSupport";
import * as events from "@/lib/api/computerTaskEvents";
import * as taskApi from "@/lib/api/computerTask";
import { ComputerTaskCardLive } from "./ComputerTaskCardLive";

let ui: ReturnType<typeof setupTaskUi>;
beforeEach(() => { ui = setupTaskUi(); vi.useFakeTimers(); });
afterEach(() => { cleanup(); vi.useRealTimers(); });
async function start() {
  const view = render(<ComputerTaskCardLive locale="en" sessionId="chat" />);
  await act(async () => {});
  await ui.emit(runningView());
  return view;
}

describe("task card uses authoritative task metadata", () => {
  it("keeps the original task stoppable after its stream goes stale, never across chats", async () => {
    const view = await start();
    await act(async () => { await vi.advanceTimersByTimeAsync(4001); });
    expect(screen.getByText(/Status unknown/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Pause" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(taskApi.computerTaskDecide).toHaveBeenCalledExactlyOnceWith("chat", key, "stop");
    view.rerender(<ComputerTaskCardLive locale="en" sessionId="other-chat" />);
    expect(screen.queryByText("Arrange my owned editor")).toBeNull();
    expect(screen.queryByRole("button", { name: "Stop" })).toBeNull();
    await act(async () => {});
    expect(events.computerTaskSubscribe).toHaveBeenCalledTimes(2);
  });

  it("deduplicates exact-generation Pause and lets task Stop overtake it", async () => {
    const pause = deferred<void>();
    vi.mocked(taskApi.computerTaskPause).mockReturnValue(pause.promise);
    await start();
    const button = screen.getByRole("button", { name: "Pause" });
    fireEvent.click(button); fireEvent.click(button);
    expect(taskApi.computerTaskPause).toHaveBeenCalledExactlyOnceWith("chat", address);
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(taskApi.computerTaskDecide).toHaveBeenCalledExactlyOnceWith("chat", key, "stop");
    await act(async () => { pause.reject(new Error("obsolete pause")); });
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByRole("button", { name: "Stopping…" })).toBeDisabled();
  });

  it("loses its green state without any extra polling while the Host is hung", async () => {
    await start();
    expect(document.querySelector(".cu-task-card")).toHaveAttribute("data-state", "running");
    await act(async () => { await vi.advanceTimersByTimeAsync(4001); });
    expect(document.querySelector(".cu-task-card")).not.toHaveAttribute("data-state", "running");
    expect(screen.getByRole("button", { name: "Stop" })).toBeEnabled();
    expect(events.computerTaskSubscribe).toHaveBeenCalledTimes(1);
  });

  it("shows the typed native window-closed fact without inferring liveness from its retained title", async () => {
    await start();
    const host = runningView();
    host.executions[0] = { ...host.executions[0], phase: "target_closed", hasFreshObservation: false };
    await ui.emit(host);
    expect(screen.getByText("The window closed. Choose a target again.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Pause" })).toBeDisabled();
    expect(document.querySelector(".cu-task-card")).not.toHaveAttribute("data-state", "running");
  });

  it("only enables Pause for a live executable Host grant", async () => {
    await start();
    expect(screen.getByRole("button", { name: "Pause" })).toBeEnabled();
    const host = runningView(); host.grants[0].executable = false;
    await ui.emit(host);
    expect(screen.getByRole("button", { name: "Pause" })).toBeDisabled();
    expect(screen.queryByText("Control authorized")).toBeNull();
  });

  it("shows retained typed business failure without parsing trace strings", async () => {
    await start();
    const host = runningView(); host.executions[0].lastFailure = "action_rejected";
    await ui.emit(host);
    expect(screen.getByRole("alert")).toHaveTextContent("Could not run that action.");
    expect(screen.getByText("Backend: synthetic")).toBeInTheDocument();
    expect(screen.queryByText(/^Running$/)).toBeNull();
  });

  it("shows stopped only after native execution reports stopped", async () => {
    await start();
    const host = runningView();
    host.executions[0].phase = "stopped"; host.grants[0].executable = false;
    await ui.emit(host);
    expect(screen.getByText("Stopped")).toBeInTheDocument();
    expect(screen.getByText("Backend: synthetic")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Stop" })).toBeDisabled();
  });

  it("does not confuse a retired grant or pending native cleanup with stopped", async () => {
    await start();
    const host = runningView();
    host.executions[0].phase = "stop_requested"; host.executions[0].cleanupPending = true;
    host.grants = [];
    await ui.emit(host);
    expect(screen.queryByText("Stopped")).toBeNull();
    expect(screen.getByRole("button", { name: "Stopping…" })).toBeDisabled();
  });

  it("does not expose opaque target ids as ordinary task names", async () => {
    const host = runningView();
    host.request = null; host.executions[0].targetName = null;
    const view = render(<ComputerTaskCardLive locale="en" sessionId="chat" />);
    await act(async () => {});
    await ui.emit(host);
    expect(screen.queryByText("fixture-window")).toBeNull();
    expect(screen.getByRole("button", { name: "Stop" })).toBeEnabled();
    view.unmount();
    expect(taskApi.computerTaskDecide).not.toHaveBeenCalled();
    expect(pendingView().executions).toHaveLength(0);
  });
});
