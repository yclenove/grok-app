/** @vitest-environment jsdom */
import { StrictMode } from "react";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { address, deferred, execution, key, pendingView, runningView, setupTaskUi } from "./taskUiTestSupport";
import * as events from "@/lib/api/computerTaskEvents";
import * as taskApi from "@/lib/api/computerTask";
import * as pickerApi from "@/lib/api/computerTaskPicker";
import type { ComputerTaskView } from "@/lib/api/computerTask";
import { ComputerPanel } from "./ComputerPanel";
import { ComputerTaskCardLive } from "./ComputerTaskCardLive";

let ui: ReturnType<typeof setupTaskUi>;
beforeEach(() => { ui = setupTaskUi(); vi.useFakeTimers(); });
afterEach(() => { cleanup(); vi.useRealTimers(); });
async function start(host = pendingView(), props: Partial<Parameters<typeof ComputerPanel>[0]> = {}) {
  const view = render(<ComputerPanel locale="en" sessionId="chat" runId={null} {...props} />);
  await act(async () => {}); await ui.emit(host);
  return view;
}
async function openPicker() {
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Choose windows for this task" })); });
  return within(screen.getByRole("dialog"));
}
const settle = () => act(async () => {});
function paused() {
  const host = runningView();
  host.executions[0] = execution({ phase: "paused", generation: "18446744073709551615", hasFreshObservation: false });
  host.grants[0] = { ...host.grants[0], suspended: true, executable: false };
  return host;
}
function stopped() {
  const host = runningView(); host.executions[0].phase = "stopped"; host.grants[0].executable = false;
  return host;
}

describe("Computer workspace task-consent lifecycle", () => {
  it("shows retained names for inactive scope members without discovering or capturing them", async () => {
    const host = runningView();
    host.grants[0].members.push({ targetId: "fixture-viewer", surface: "desktop", displayName: "Selected reference viewer" });
    await start(host);
    expect(screen.getByText("Selected reference viewer")).toBeInTheDocument();
    expect(screen.queryByText("fixture-viewer")).toBeNull();
    expect(pickerApi.computerTaskPickerBegin).not.toHaveBeenCalled();
    expect(pickerApi.computerTaskPickerLoad).not.toHaveBeenCalled();
    expect(taskApi.computerTaskPreview).toHaveBeenCalledExactlyOnceWith("chat", address);
  });

  it("opens idle without requesting, selecting, approving or capturing anything", async () => {
    await start({ ...pendingView(), taskId: null, request: null, pickerSurfaces: [] });
    expect(screen.getByText("Access is requested when a task needs it.")).toBeInTheDocument();
    expect(pickerApi.computerTaskPickerBegin).not.toHaveBeenCalled();
    expect(pickerApi.computerTaskPickerLoad).not.toHaveBeenCalled();
    expect(pickerApi.computerTaskPickerApprove).not.toHaveBeenCalled();
    expect(taskApi.computerTaskPreview).not.toHaveBeenCalled();
  });

  it("does not create any local observer or actionable Stop without a chat", async () => {
    render(<ComputerPanel locale="en" sessionId={null} runId={null} />);
    expect(screen.getByText("Open a local chat to use Computer Use.")).toBeInTheDocument();
    expect(events.computerTaskSubscribe).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "Stop" })).toBeNull();
  });

  it("shows a pending request with feature off instead of forcing a settings visit", async () => {
    await start();
    expect(screen.getByText("Arrange my owned editor")).toBeInTheDocument();
    expect(screen.getByText("Only selected fixture windows")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Choose windows for this task" })).toBeEnabled();
    expect(screen.queryByRole("link", { name: "Settings" })).toBeNull();
    expect(pickerApi.computerTaskPickerBegin).not.toHaveBeenCalled();
  });

  it("honestly disables unsupported task surfaces instead of using the legacy picker", async () => {
    const host = pendingView(); host.pickerSurfaces = []; host.request!.surfaces = ["existing-tabs"];
    await start(host, { surface: "existing-tabs" });
    expect(screen.getByText("Target selection for this surface is not available yet.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Select target" })).toBeDisabled();
    expect(pickerApi.computerTaskPickerBegin).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Deny" })).toBeEnabled();
  });

  it("performs exactly one explicit picker transaction after repeated clicks", async () => {
    await start();
    const allow = screen.getByRole("button", { name: "Choose windows for this task" });
    await act(async () => { fireEvent.click(allow); fireEvent.click(allow); });
    expect(pickerApi.computerTaskPickerBegin).toHaveBeenCalledExactlyOnceWith("chat", key, "desktop");
    expect(pickerApi.computerTaskPickerLoad).toHaveBeenCalledTimes(1);
    expect(taskApi.computerTaskPreview).not.toHaveBeenCalled();
    expect(pickerApi.computerTaskPickerApprove).not.toHaveBeenCalled();
  });

  it("bounds multiple window selection and approval uses only original picker ids", async () => {
    await start(); const modal = await openPicker();
    fireEvent.click(modal.getByRole("button", { name: /Fixture editor/ }));
    fireEvent.click(modal.getByRole("button", { name: /Fixture viewer/ }));
    expect(modal.getByText("Selected 2 of up to 2")).toBeInTheDocument();
    const allow = modal.getByRole("button", { name: "Enable and allow this task" });
    await act(async () => { fireEvent.click(allow); fireEvent.click(allow); });
    expect(pickerApi.computerTaskPickerApprove).toHaveBeenCalledTimes(1);
    expect(vi.mocked(pickerApi.computerTaskPickerApprove).mock.calls[0][1]).toEqual(["fixture-A", "fixture-B"]);
    expect(screen.queryByText("Control authorized")).toBeNull();
    expect(taskApi.computerTaskPreview).not.toHaveBeenCalled();
  });

  it("shares one observer and one modal/approval transaction between card and panel", async () => {
    render(<><ComputerTaskCardLive locale="en" sessionId="chat" /><ComputerPanel locale="en" sessionId="chat" runId={null} /></>);
    await settle(); await ui.emit(pendingView());
    await act(async () => { fireEvent.click(screen.getAllByRole("button", { name: "Choose windows for this task" })[0]); });
    expect(events.computerTaskSubscribe).toHaveBeenCalledTimes(1);
    expect(screen.getAllByRole("dialog")).toHaveLength(1);
    const modal = within(screen.getByRole("dialog"));
    fireEvent.click(modal.getByRole("button", { name: /Fixture editor/ }));
    await act(async () => { fireEvent.click(modal.getByRole("button", { name: "Enable and allow this task" })); });
    expect(pickerApi.computerTaskPickerApprove).toHaveBeenCalledTimes(1);
    expect(screen.getAllByRole("button", { name: "Stop" })).toHaveLength(3);
  });

  it("does not turn an approval receipt into a grant before Host commit facts", async () => {
    await start(); const modal = await openPicker();
    fireEvent.click(modal.getByRole("button", { name: /Fixture editor/ }));
    await act(async () => { fireEvent.click(modal.getByRole("button", { name: "Enable and allow this task" })); });
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(taskApi.computerTaskPreview).not.toHaveBeenCalled();
    await ui.emit(runningView());
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.getByText("Control authorized")).toBeInTheDocument();
    expect(taskApi.computerTaskPreview).toHaveBeenCalledExactlyOnceWith("chat", address);
  });

  it("retires failed target choices and requires a new explicit choice", async () => {
    vi.mocked(pickerApi.computerTaskPickerLoad).mockRejectedValueOnce(new Error("synthetic failure"));
    await start(); const modal = await openPicker();
    expect(modal.queryByText("Fixture editor")).toBeNull();
    expect(modal.getByRole("alert")).toHaveTextContent("Could not load targets");
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    await openPicker();
    expect(pickerApi.computerTaskPickerBegin).toHaveBeenCalledTimes(2);
  });

  it("cancels a preparing request even while approval is awaiting its reply", async () => {
    const approve = deferred<Awaited<ReturnType<typeof pickerApi.computerTaskPickerApprove>>>();
    vi.mocked(pickerApi.computerTaskPickerApprove).mockReturnValue(approve.promise);
    await start(); const modal = await openPicker();
    fireEvent.click(modal.getByRole("button", { name: /Fixture editor/ }));
    fireEvent.click(modal.getByRole("button", { name: "Enable and allow this task" }));
    const host = pendingView(); host.request!.phase = "preparing"; await ui.emit(host);
    const buttons = screen.getAllByRole("button", { name: "Cancel" });
    fireEvent.click(buttons[buttons.length - 1]);
    expect(taskApi.computerTaskDecide).toHaveBeenCalledExactlyOnceWith("chat", key, "cancel");
    await act(async () => { approve.reject(new Error("obsolete approval")); });
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("lets Stop overtake a pending picker without waiting for cancellation", async () => {
    const begin = deferred<Awaited<ReturnType<typeof pickerApi.computerTaskPickerBegin>>>();
    vi.mocked(pickerApi.computerTaskPickerBegin).mockReturnValue(begin.promise);
    await start();
    fireEvent.click(screen.getByRole("button", { name: "Choose windows for this task" }));
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Stop" }));
    expect(taskApi.computerTaskDecide).toHaveBeenCalledExactlyOnceWith("chat", key, "stop");
    await act(async () => { begin.resolve({ sessionId: "chat", key, pickerId: "late-picker", maxSelection: 2, surface: "desktop" }); });
    expect(pickerApi.computerTaskPickerLoad).not.toHaveBeenCalled();
    expect(pickerApi.computerTaskPickerCancel).toHaveBeenCalledTimes(1);
  });

  it("shows approval failures without reapproving or reopening automatically", async () => {
    vi.mocked(pickerApi.computerTaskPickerApprove).mockRejectedValueOnce(new Error("synthetic permission refusal"));
    await start(); const modal = await openPicker();
    fireEvent.click(modal.getByRole("button", { name: /Fixture editor/ }));
    await act(async () => { fireEvent.click(modal.getByRole("button", { name: "Enable and allow this task" })); });
    expect(modal.getByRole("alert")).toHaveTextContent("Could not run that action.");
    await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
    expect(pickerApi.computerTaskPickerApprove).toHaveBeenCalledTimes(1);
    expect(pickerApi.computerTaskPickerBegin).toHaveBeenCalledTimes(1);
  });

  it("denies a no-run request and waits for the Host terminal state", async () => {
    await start();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Deny" })); });
    expect(taskApi.computerTaskDecide).toHaveBeenCalledExactlyOnceWith("chat", key, "deny");
    expect(screen.getByRole("button", { name: "Deny" })).toBeDisabled();
    const host = pendingView(); host.request!.phase = "denied"; await ui.emit(host);
    expect(screen.getByText("Request denied")).toBeInTheDocument();
    expect(pickerApi.computerTaskPickerBegin).not.toHaveBeenCalled();
  });

  it("shows expired requests without silently requesting or retrying them", async () => {
    const host = pendingView(); host.request!.phase = "expired";
    await start(host);
    expect(screen.getByText("Request expired")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Choose windows for this task" })).toBeNull();
    expect(taskApi.computerTaskDecide).not.toHaveBeenCalled();
  });

  it("preserves original executable scope after an incremental denial", async () => {
    const host = runningView();
    host.request = { ...pendingView().request!, key: { ...key, requestId: "incremental-request", revision: 2 }, phase: "denied" };
    await start(host);
    expect(screen.getByRole("button", { name: "Pause" })).toBeEnabled();
    expect(taskApi.computerTaskPreview).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(taskApi.computerTaskDecide).toHaveBeenCalledWith("chat", host.request.key, "stop");
  });

  it("pauses exact original execution and Stop remains reachable", async () => {
    const pause = deferred<void>(); vi.mocked(taskApi.computerTaskPause).mockReturnValue(pause.promise);
    await start(runningView());
    const button = screen.getByRole("button", { name: "Pause" });
    fireEvent.click(button); fireEvent.click(button);
    expect(taskApi.computerTaskPause).toHaveBeenCalledExactlyOnceWith("chat", address);
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(taskApi.computerTaskDecide).toHaveBeenCalledTimes(1);
    await act(async () => { pause.reject(new Error("obsolete")); });
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("keeps Stop reachable after feature disablement with a running Host row", async () => {
    const host = runningView(); host.featureEnabled = false; host.grants[0].executable = false;
    await start(host);
    expect(screen.getByRole("button", { name: "Pause" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Stop" })).toBeEnabled();
    expect(taskApi.computerTaskPreview).not.toHaveBeenCalled();
  });

  it("never resumes or reports stopped before native cleanup finishes", async () => {
    const host = runningView(); host.executions[0].phase = "stop_requested"; host.executions[0].cleanupPending = true;
    await start(host);
    expect(screen.queryByText("Stopped")).toBeNull();
    expect(screen.queryByRole("button", { name: "Resume" })).toBeNull();
    expect(screen.getByRole("button", { name: "Stopping…" })).toBeDisabled();
    await ui.emit(stopped());
    expect(screen.getByText("Stopped")).toBeInTheDocument();
  });

  it("settles Stop from native completion before a late control transport error", async () => {
    const stop = deferred<Awaited<ReturnType<typeof taskApi.computerTaskDecide>>>();
    vi.mocked(taskApi.computerTaskDecide).mockReturnValue(stop.promise);
    await start(runningView()); fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    await ui.emit(stopped());
    await act(async () => { stop.reject(new Error("obsolete reply")); });
    expect(screen.getByRole("button", { name: "Stop" })).toBeDisabled();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("keeps business failure after Stop while fixed request tools remain available", async () => {
    const host = stopped(); host.executions[0].lastFailure = "outcome_unknown";
    await start(host);
    expect(screen.getByRole("alert")).toHaveTextContent("The action outcome is unknown.");
    expect(screen.queryByRole("button", { name: "Retry cleanup" })).toBeNull();
    expect(screen.getByText("Stopped")).toBeInTheDocument();
  });

  it("captures UI pixels only while the preview is visible", async () => {
    await start(runningView());
    expect(screen.getByRole("img")).toHaveAttribute("src", "data:image/png;base64,AAAA");
    fireEvent.click(screen.getByRole("button", { name: "Hide preview" }));
    await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
    expect(taskApi.computerTaskPreview).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("img")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Show preview" })); await settle();
    expect(taskApi.computerTaskPreview).toHaveBeenCalledTimes(2);
  });

  it("does not capture when the document is hidden from initial mount", async () => {
    Object.defineProperty(document, "hidden", { configurable: true, value: true });
    await start(runningView());
    expect(taskApi.computerTaskPreview).not.toHaveBeenCalled();
    Object.defineProperty(document, "hidden", { configurable: true, value: false });
    fireEvent(document, new Event("visibilitychange")); await settle();
    expect(taskApi.computerTaskPreview).toHaveBeenCalledTimes(1);
  });

  it("stops capture on document hide and ignores an in-flight late frame", async () => {
    const frame = deferred<Awaited<ReturnType<typeof taskApi.computerTaskPreview>>>();
    vi.mocked(taskApi.computerTaskPreview).mockReturnValue(frame.promise);
    await start(runningView());
    Object.defineProperty(document, "hidden", { configurable: true, value: true });
    fireEvent(document, new Event("visibilitychange"));
    await act(async () => { frame.resolve({ address, previewDataUrl: "data:image/png;base64,OLD", truncated: false }); });
    expect(screen.queryByRole("img")).toBeNull();
    await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
    expect(taskApi.computerTaskPreview).toHaveBeenCalledTimes(1);
  });

  it("does not capture a paused or unapproved target", async () => {
    await start(paused());
    expect(taskApi.computerTaskPreview).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "Resume" })).toBeNull();
    const host = runningView(); host.grants = []; await ui.emit(host);
    expect(taskApi.computerTaskPreview).not.toHaveBeenCalled();
  });

  it("does not preview or Pause a natively closed target, and keeps the original task stoppable", async () => {
    const host = runningView(); host.executions[0].phase = "target_closed";
    await start(host);
    expect(screen.getByText("The window closed. Choose a target again.")).toBeInTheDocument();
    expect(taskApi.computerTaskPreview).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Pause" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Stop" })).toBeEnabled();
    expect(screen.getByText("Backend: synthetic")).toBeInTheDocument();
  });

  it("does not queue duplicate captures while one request is hung", async () => {
    vi.mocked(taskApi.computerTaskPreview).mockReturnValue(new Promise(() => {}));
    await start(runningView());
    await act(async () => { await vi.advanceTimersByTimeAsync(2500); });
    expect(taskApi.computerTaskPreview).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "Stop" })).toBeEnabled();
  });

  it("bounds frame freshness by request start, including slow native capture", async () => {
    const frame = deferred<Awaited<ReturnType<typeof taskApi.computerTaskPreview>>>();
    vi.mocked(taskApi.computerTaskPreview).mockReturnValue(frame.promise);
    await start(runningView());
    await act(async () => { await vi.advanceTimersByTimeAsync(3001); });
    await act(async () => { frame.resolve({ address, previewDataUrl: "data:image/png;base64,SLOW", truncated: false }); });
    expect(screen.getByText("Last captured frame — not live.")).toBeInTheDocument();
  });

  it("never uses an old-generation frame for a new target or generation", async () => {
    const frame = deferred<Awaited<ReturnType<typeof taskApi.computerTaskPreview>>>();
    vi.mocked(taskApi.computerTaskPreview).mockReturnValueOnce(frame.promise).mockReturnValue(new Promise(() => {}));
    await start(runningView());
    const host = runningView(); host.executions[0].generation = "18446744073709551615"; await ui.emit(host);
    expect(taskApi.computerTaskPreview).toHaveBeenCalledTimes(1);
    await act(async () => { frame.resolve({ address, previewDataUrl: "data:image/png;base64,OLD", truncated: false }); });
    expect(screen.queryByRole("img")).toBeNull();
    await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
    expect(taskApi.computerTaskPreview).toHaveBeenCalledTimes(2);
  });

  it("discards late frames after switching chats, even when run ids collide", async () => {
    const frame = deferred<Awaited<ReturnType<typeof taskApi.computerTaskPreview>>>();
    vi.mocked(taskApi.computerTaskPreview).mockReturnValueOnce(frame.promise);
    const view = await start(runningView());
    view.rerender(<ComputerPanel locale="en" sessionId="other-chat" runId={address.runId} />);
    await settle();
    await act(async () => { frame.resolve({ address, previewDataUrl: "data:image/png;base64,OLD", truncated: false }); });
    expect(screen.queryByRole("img")).toBeNull();
    expect(screen.queryByText("Owned editor")).toBeNull();
    expect(taskApi.computerTaskPreview).toHaveBeenCalledTimes(1);
  });

  it("rejects mismatched preview replies and shows no model observation ids", async () => {
    vi.mocked(taskApi.computerTaskPreview).mockResolvedValue({ address: { ...address, generation: "1" }, previewDataUrl: "data:image/png;base64,BAD", truncated: false });
    await start(runningView());
    expect(screen.queryByRole("img")).toBeNull();
    expect(screen.getByRole("alert")).toHaveTextContent("Could not run that action.");
    expect(screen.queryByText(address.runId)).toBeNull();
  });

  it("shows no-image and truncated observation facts from the exact Host reply", async () => {
    vi.mocked(taskApi.computerTaskPreview).mockResolvedValue({ address, previewDataUrl: null, truncated: true });
    await start(runningView());
    expect(screen.getByText("This observation has no image. Coordinate clicks are blocked.")).toBeInTheDocument();
    expect(screen.getByText("This observation was truncated.")).toBeInTheDocument();
    expect(screen.queryByRole("img")).toBeNull();
  });

  it("keeps the exact task stoppable when metadata goes stale, with no extra polling", async () => {
    await start(runningView());
    await act(async () => { await vi.advanceTimersByTimeAsync(4001); });
    expect(screen.getByText(/Status unknown/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(taskApi.computerTaskDecide).toHaveBeenCalledExactlyOnceWith("chat", key, "stop");
    expect(events.computerTaskSubscribe).toHaveBeenCalledTimes(1);
  });

  it("refreshes only metadata on explicit recovery, never access or capture", async () => {
    await start();
    await act(async () => { await vi.advanceTimersByTimeAsync(4001); });
    fireEvent.click(screen.getByRole("button", { name: "Refresh status" })); await settle();
    expect(events.computerTaskSubscribe).toHaveBeenCalledTimes(2);
    expect(pickerApi.computerTaskPickerBegin).not.toHaveBeenCalled();
    expect(pickerApi.computerTaskPickerApprove).not.toHaveBeenCalled();
    expect(taskApi.computerTaskPreview).not.toHaveBeenCalled();
  });

  it("Escape cancels only the original picker and restores the allow-button focus", async () => {
    await start();
    const allow = screen.getByRole("button", { name: "Choose windows for this task" }); allow.focus();
    await openPicker();
    await act(async () => { await vi.advanceTimersByTimeAsync(1); });
    expect(screen.getByRole("dialog")).toContainElement(document.activeElement as HTMLElement);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(allow).toHaveFocus();
    expect(pickerApi.computerTaskPickerCancel).toHaveBeenCalledTimes(1);
    expect(taskApi.computerTaskDecide).not.toHaveBeenCalled();
  });

  it("last consumer unmount releases observer/picker but never Stops a grant", async () => {
    const view = await start(); await openPicker();
    view.unmount();
    expect(pickerApi.computerTaskPickerCancel).toHaveBeenCalledTimes(1);
    expect(events.computerTaskUnsubscribe).toHaveBeenCalledTimes(1);
    expect(taskApi.computerTaskDecide).not.toHaveBeenCalled();
  });

  it("StrictMode remount never approves or Stops and late original callbacks cannot replace new facts", async () => {
    render(<StrictMode><ComputerPanel locale="en" sessionId="chat" runId={null} /></StrictMode>);
    await settle(); await ui.emit(pendingView());
    const original = ui.streams[0];
    if (ui.streams.length > 1) await act(async () => { ui.send(runningView(), original); });
    expect(screen.getByRole("button", { name: "Choose windows for this task" })).toBeEnabled();
    expect(pickerApi.computerTaskPickerApprove).not.toHaveBeenCalled();
    expect(taskApi.computerTaskDecide).not.toHaveBeenCalled();
  });

  it("keeps HTML in model purpose as inert plain text", async () => {
    const host: ComputerTaskView = pendingView(); host.request!.purpose = "<img src=x onerror=alert(1)>";
    await start(host);
    expect(screen.getByText(host.request!.purpose)).toBeInTheDocument();
    expect(screen.queryByRole("img")).toBeNull();
  });
});
