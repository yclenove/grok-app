/** @vitest-environment jsdom */
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { address, deferred, runningView, setupTaskUi } from "./taskUiTestSupport";
import * as api from "@/lib/api/computerTask";
import type { ComputerTaskUploadCompletion } from "@/lib/api/computerTask";
import { ComputerTaskUpload } from "./ComputerTaskUpload";
import { useComputerTask } from "./useComputerTask";

function Harness({ sessionId = "upload-chat" }: { sessionId?: string }) {
  const host = useComputerTask(sessionId);
  return <section><ComputerTaskUpload locale="en" sessionId={sessionId} host={host} /></section>;
}
const uploadView = () => {
  const view = runningView("upload-chat");
  view.executions[0].target = { targetId: "upload-page", surface: "app-webview" };
  view.executions[0].upload = { requestId: "upload-request", actionId: "upload-action", phase: "awaiting_file", targetName: "Owned fixture page", fieldName: "Attachment" };
  return view;
};
const completion = (kind: "verified" | "rejected" = "verified"): ComputerTaskUploadCompletion => ({
  outcome: { runId: address.runId, actionId: "upload-action", kind, executed: kind === "verified", reason: kind === "rejected" ? "user cancelled file selection" : null },
  bytes: kind === "verified" ? 0 : null,
});
let ui: ReturnType<typeof setupTaskUi>;
beforeEach(() => {
  ui = setupTaskUi();
  vi.mocked(api.computerTaskCancelUpload).mockResolvedValue(completion("rejected"));
});
afterEach(cleanup);

it("waits for a task intent and a real user click, and shares the chooser between views", async () => {
  const pending = deferred<ComputerTaskUploadCompletion>();
  vi.mocked(api.computerTaskChooseUpload).mockReturnValue(pending.promise);
  render(<><Harness /><Harness /></>);
  await waitFor(() => expect(ui.streams).toHaveLength(1));
  expect(api.computerTaskChooseUpload).not.toHaveBeenCalled();
  await ui.emit(uploadView());
  const buttons = screen.getAllByRole("button", { name: "Choose and upload" });
  fireEvent.click(buttons[0]);
  expect(buttons[1]).toBeDisabled();
  fireEvent.click(buttons[1]);
  expect(api.computerTaskChooseUpload).toHaveBeenCalledExactlyOnceWith("upload-chat", address, "upload-request", "Choose a file for Owned fixture page");
  const finished = uploadView();
  finished.executions[0].upload = null;
  await ui.emit(finished);
  await act(async () => pending.resolve(completion()));
  expect(screen.getAllByText("File assigned · website submission not confirmed.")).toHaveLength(2);
});

it("keeps the original native chooser lock across unmount and reopen", async () => {
  const pending = deferred<ComputerTaskUploadCompletion>();
  vi.mocked(api.computerTaskChooseUpload).mockReturnValue(pending.promise);
  const first = render(<Harness />);
  await ui.emit(uploadView());
  fireEvent.click(screen.getByRole("button", { name: "Choose and upload" }));
  first.unmount();
  await waitFor(() => expect(api.computerTaskCancelUpload).toHaveBeenCalledOnce());
  render(<Harness />);
  await ui.emit(uploadView());
  expect(screen.getByRole("button", { name: "Choose and upload" })).toBeDisabled();
  expect(screen.getByText("Close the file dialog so cleanup can finish.")).toBeInTheDocument();
  await act(async () => pending.resolve(completion("rejected")));
  expect(api.computerTaskChooseUpload).toHaveBeenCalledTimes(1);
  expect(screen.queryByText("File assigned · website submission not confirmed.")).not.toBeInTheDocument();
});

it("cancel is exact and does not claim that the original system dialog closed", async () => {
  const pending = deferred<ComputerTaskUploadCompletion>();
  vi.mocked(api.computerTaskChooseUpload).mockReturnValue(pending.promise);
  render(<Harness />);
  await ui.emit(uploadView());
  fireEvent.click(screen.getByRole("button", { name: "Choose and upload" }));
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  await waitFor(() => expect(api.computerTaskCancelUpload).toHaveBeenCalledExactlyOnceWith("upload-chat", address, "upload-request"));
  const cancelled = uploadView();
  cancelled.executions[0].upload = null;
  await ui.emit(cancelled);
  expect(screen.getByText("Close the file dialog so cleanup can finish.")).toBeInTheDocument();
  await act(async () => pending.resolve(completion("rejected")));
  expect(screen.getByText("File selection cancelled")).toBeInTheDocument();
});

it("stale task metadata cannot choose a file and picker failure has a readable error", async () => {
  const pending = deferred<ComputerTaskUploadCompletion>();
  vi.mocked(api.computerTaskChooseUpload).mockReturnValue(pending.promise);
  render(<Harness />);
  await ui.emit(uploadView());
  fireEvent.click(screen.getByRole("button", { name: "Choose and upload" }));
  await act(async () => pending.reject(new Error("private OS detail must not render")));
  expect(screen.getByRole("alert")).toHaveTextContent("File choice or assignment failed. Check the task status.");
  expect(screen.queryByText(/private OS detail/)).not.toBeInTheDocument();
});
