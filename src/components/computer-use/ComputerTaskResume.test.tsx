/** @vitest-environment jsdom */
import { StrictMode } from "react";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { createT, loadLocaleCatalog } from "@/i18n";
import { address, deferred, execution, runningView, setupTaskUi } from "./taskUiTestSupport";
import * as taskApi from "@/lib/api/computerTask";
import * as pickerApi from "@/lib/api/computerTaskPicker";
import type { ComputerTaskResumeOutcome } from "@/lib/api/computerTask";
import { ComputerPanel } from "./ComputerPanel";
import { ComputerTaskCardLive } from "./ComputerTaskCardLive";

let ui: ReturnType<typeof setupTaskUi>;
beforeEach(() => { ui = setupTaskUi(); vi.useFakeTimers(); });
afterEach(() => { cleanup(); vi.useRealTimers(); });
function paused(cleanupPending = false) {
  const host = runningView();
  host.executions[0] = execution({ phase: "paused", cleanupPending, resumeAvailable: true, hasFreshObservation: false });
  host.grants[0] = { ...host.grants[0], suspended: true, executable: false };
  return host;
}
async function showBoth(cleanupPending = false) {
  render(<><ComputerTaskCardLive locale="en" sessionId="chat" />
    <ComputerPanel locale="en" sessionId="chat" runId={null} /></>);
  await act(async () => {}); await ui.emit(paused(cleanupPending));
}

it("shows original cleanup in both surfaces and accepts one Continue without disabling Stop", async () => {
  const reply = deferred<ComputerTaskResumeOutcome>();
  vi.mocked(taskApi.computerTaskResume).mockReturnValueOnce(reply.promise);
  await showBoth(true);
  for (const status of screen.getAllByRole("status")) expect(status).toHaveTextContent("Paused · finishing up…");
  expect(document.querySelector(".cu-task-card")).toHaveAttribute("data-state", "paused");
  const buttons = screen.getAllByRole("button", { name: "Resume" });
  for (const button of buttons) expect(button).toBeEnabled();
  fireEvent.click(buttons[0]); fireEvent.click(buttons[1]);
  expect(taskApi.computerTaskResume).toHaveBeenCalledExactlyOnceWith("chat", address);
  const waiting = paused(true); waiting.executions[0].phase = "resuming";
  waiting.executions[0].resumeAvailable = false;
  await ui.emit(waiting);
  for (const status of screen.getAllByRole("status")) expect(status).toHaveTextContent("Resuming…");
  for (const button of screen.getAllByRole("button", { name: "Stop" })) expect(button).toBeEnabled();
  fireEvent.click(screen.getAllByRole("button", { name: "Stop" })[1]);
  await act(async () => { reply.reject(new Error("retired native wait")); });
  expect(taskApi.computerTaskDecide).toHaveBeenCalledExactlyOnceWith("chat", address.key, "stop");
  expect(screen.queryByRole("alert")).toBeNull();
  expect(taskApi.computerTaskPreview).not.toHaveBeenCalled();
  expect(pickerApi.computerTaskPickerApprove).not.toHaveBeenCalled();
});

it("card and panel share one Continue and keep Stop enabled while recovery is pending", async () => {
  const reply = deferred<ComputerTaskResumeOutcome>();
  vi.mocked(taskApi.computerTaskResume).mockReturnValueOnce(reply.promise);
  await showBoth();
  const buttons = screen.getAllByRole("button", { name: "Resume" });
  expect(buttons).toHaveLength(2);
  fireEvent.click(buttons[0]); fireEvent.click(buttons[1]);
  expect(taskApi.computerTaskResume).toHaveBeenCalledExactlyOnceWith("chat", address);
  expect(screen.getAllByRole("button", { name: "Resume" }).every((button) => (button as HTMLButtonElement).disabled)).toBe(true);
  expect(screen.getAllByRole("button", { name: "Stop" }).every((button) => !(button as HTMLButtonElement).disabled)).toBe(true);
  expect(taskApi.computerTaskPreview).not.toHaveBeenCalled();
  fireEvent.click(screen.getAllByRole("button", { name: "Stop" })[1]);
  await act(async () => { reply.reject(new Error("late recovery error")); });
  expect(taskApi.computerTaskDecide).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole("alert")).toBeNull();
});

it("receipts cannot turn a paused task green without fresh Host authority", async () => {
  await showBoth();
  await act(async () => { fireEvent.click(screen.getAllByRole("button", { name: "Resume" })[0]); });
  expect(document.querySelector(".cu-task-card")).toHaveAttribute("data-state", "resuming");
  expect(taskApi.computerTaskPreview).not.toHaveBeenCalled();
  const native = runningView(); native.executions[0].generation = "18446744073709551615";
  await ui.emit(native);
  expect(document.querySelector(".cu-task-card")).toHaveAttribute("data-state", "running");
  expect(screen.getAllByRole("button", { name: "Pause" })).toHaveLength(2);
  expect(taskApi.computerTaskResume).toHaveBeenCalledTimes(1);
});

it("new native recovery settles before an obsolete error without requiring another click", async () => {
  const reply = deferred<ComputerTaskResumeOutcome>();
  vi.mocked(taskApi.computerTaskResume).mockReturnValueOnce(reply.promise);
  await showBoth(); fireEvent.click(screen.getAllByRole("button", { name: "Resume" })[0]);
  const native = runningView(); native.executions[0].generation = "18446744073709551615";
  await ui.emit(native);
  await act(async () => { reply.reject(new Error("old transport error")); });
  expect(screen.queryByRole("alert")).toBeNull();
  expect(document.querySelector(".cu-task-card")).toHaveAttribute("data-state", "running");
});

it("stale Host metadata disables Continue while retaining exact task Stop", async () => {
  await showBoth();
  await act(async () => { await vi.advanceTimersByTimeAsync(4001); });
  for (const button of screen.getAllByRole("button", { name: "Resume" })) expect(button).toBeDisabled();
  for (const button of screen.getAllByRole("button", { name: "Stop" })) expect(button).toBeEnabled();
  expect(taskApi.computerTaskResume).not.toHaveBeenCalled();
});

it("mount and StrictMode never resume an unsupported or unapproved paused scope", async () => {
  render(<StrictMode><ComputerTaskCardLive locale="en" sessionId="chat" /></StrictMode>);
  await act(async () => {});
  const unsupported = paused(); unsupported.executions[0].resumeAvailable = false;
  await ui.emit(unsupported);
  expect(screen.queryByRole("button", { name: "Resume" })).toBeNull();
  expect(screen.getByRole("button", { name: "Stop" })).toBeEnabled();
  expect(taskApi.computerTaskResume).not.toHaveBeenCalled();
});

it.each(["en", "zh"] as const)("shows a concise localized recovery failure in %s", async (locale) => {
  await loadLocaleCatalog(locale);
  vi.mocked(taskApi.computerTaskResume).mockRejectedValueOnce(new Error("native reason not exposed"));
  render(<ComputerTaskCardLive locale={locale} sessionId="chat" />);
  await act(async () => {}); await ui.emit(paused());
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: createT(locale)("cu.panel.resume") })); });
  expect(screen.getByRole("alert")).toHaveTextContent(createT(locale)("cu.consent.resumeFailed"));
  expect(screen.getByRole("button", { name: createT(locale)("cu.panel.resume") })).toBeEnabled();
  expect(screen.queryByText("native reason not exposed")).toBeNull();
});
