/** @vitest-environment jsdom */
import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { pendingView, runningView, setupTaskUi } from "./taskUiTestSupport";
import { ComputerTaskCardLive } from "./ComputerTaskCardLive";

let ui: ReturnType<typeof setupTaskUi>;
beforeEach(() => { ui = setupTaskUi(); vi.useFakeTimers(); });
afterEach(() => { cleanup(); vi.useRealTimers(); });
async function start() {
  render(<ComputerTaskCardLive locale="en" sessionId="chat" />);
  await act(async () => {});
}

it("shows completed only after exact Host cleanup and retires task controls", async () => {
  await start();
  const view = runningView();
  view.taskId = null;
  view.request!.phase = "completed";
  view.grants = [];
  view.executions[0].phase = "stop_requested";
  view.executions[0].cleanupPending = true;
  await ui.emit(view);
  expect(screen.queryByText("Task completed")).toBeNull();
  expect(screen.getByRole("status")).toHaveTextContent("Stopping…");
  view.executions[0].phase = "stopped";
  await ui.emit({ ...view });
  expect(screen.queryByText("Task completed")).toBeNull();
  expect(screen.getByRole("status")).toHaveTextContent("Stopping…");
  view.executions[0].cleanupPending = false;
  await ui.emit({ ...view });
  expect(screen.getByText("Task completed")).toBeInTheDocument();
  expect(screen.queryByText("Stopped")).toBeNull();
  expect(screen.queryByRole("button", { name: "Pause" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Stop" })).toBeNull();
  expect(screen.getByRole("region")).toHaveAttribute("data-state", "completed");
});

it("keeps awaiting observation authorized without claiming continuation is pending", async () => {
  await start();
  const view = runningView();
  view.executions[0].phase = "awaiting_observation";
  view.executions[0].hasFreshObservation = false;
  await ui.emit(view);
  expect(screen.getByText("Control authorized")).toBeInTheDocument();
  expect(screen.queryByText(/waiting for the task to continue/i)).toBeNull();
  expect(screen.getByRole("button", { name: "Pause" })).toBeEnabled();
});

it("preserves pending approval and pause without an explicit completion", async () => {
  await start();
  await ui.emit(pendingView());
  expect(screen.getByRole("button", { name: "Choose windows for this task" })).toBeEnabled();
  expect(screen.queryByText("Task completed")).toBeNull();
  const view = runningView();
  view.executions[0].phase = "paused";
  view.executions[0].resumeAvailable = true;
  view.grants[0].suspended = true;
  view.grants[0].executable = false;
  await ui.emit(view);
  expect(screen.getByRole("button", { name: "Resume" })).toBeEnabled();
  expect(screen.queryByText("Task completed")).toBeNull();
});
