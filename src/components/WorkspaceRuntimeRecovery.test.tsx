/** @vitest-environment jsdom */
import { StrictMode } from "react";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { WorkspaceRuntimeRecovery } from "./WorkspaceRuntimeRecovery";
import { requestWorkspaceRuntimeReview } from "@/lib/workspaceRuntime";

const mocks = vi.hoisted(() => ({ preview: vi.fn(), confirm: vi.fn() }));
vi.mock("@/lib/api/workspaceRuntime", () => ({ previewWorkspaceRuntime: mocks.preview, confirmWorkspaceRuntime: mocks.confirm }));
const review = { sessionId: "s1", cwd: "/primary", roots: [
  { path: "/primary", role: "primary", access: "write" }, { path: "/extra", role: "extra", access: "read" },
], requestHash: "review-hash" };
beforeEach(() => { vi.clearAllMocks(); mocks.preview.mockResolvedValue(review); mocks.confirm.mockResolvedValue(undefined); });
afterEach(cleanup);

function start() { act(() => requestWorkspaceRuntimeReview("s1")); }

test("shows current folders and requires explicit confirmation without sending", async () => {
  render(<StrictMode><WorkspaceRuntimeRecovery locale="en" sessionId="s1" /></StrictMode>);
  start();
  expect(await screen.findByText("/extra")).toBeTruthy();
  expect(mocks.confirm).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Confirm for next connection" }));
  await screen.findByText("Confirmed. Send your message again to connect using these folders.");
  expect(mocks.confirm).toHaveBeenCalledExactlyOnceWith(review);
});

test("Escape cancels a review without changing the binding", async () => {
  render(<WorkspaceRuntimeRecovery locale="en" sessionId="s1" />);
  start(); await screen.findByText("/extra");
  fireEvent.keyDown(document, { key: "Escape" });
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(mocks.confirm).not.toHaveBeenCalled();
});

test("late preview after navigation does not open the new chat", async () => {
  let resolve!: (value: typeof review) => void;
  mocks.preview.mockImplementation(() => new Promise((done) => { resolve = done; }));
  const ui = render(<WorkspaceRuntimeRecovery locale="en" sessionId="s1" />);
  start();
  ui.rerender(<WorkspaceRuntimeRecovery locale="en" sessionId="s2" />);
  await act(async () => resolve(review));
  expect(screen.queryByRole("dialog")).toBeNull();
  act(() => requestWorkspaceRuntimeReview("s1"));
  expect(mocks.preview).toHaveBeenCalledTimes(1);
});

test("double confirmation has one request; stale review failure requires a new preview", async () => {
  let reject!: (reason: unknown) => void;
  mocks.confirm.mockImplementation(() => new Promise((_done, fail) => { reject = fail; }));
  render(<WorkspaceRuntimeRecovery locale="en" sessionId="s1" />);
  start(); await screen.findByText("/extra");
  const button = screen.getByRole("button", { name: "Confirm for next connection" });
  fireEvent.click(button); fireEvent.click(button);
  expect(mocks.confirm).toHaveBeenCalledTimes(1);
  await act(async () => reject("WORKSPACE_CHANGED: review expired"));
  expect(screen.getByRole("alert").textContent).toContain("Check the project");
  expect(screen.queryByRole("button", { name: "Confirm for next connection" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  start(); await screen.findByText("/extra");
  expect(mocks.preview).toHaveBeenCalledTimes(2);
});

test("missing folders are explained without a confirmation button", async () => {
  mocks.preview.mockRejectedValue("WORKSPACE_CHANGED: missing root");
  render(<WorkspaceRuntimeRecovery locale="zh" sessionId="s1" />);
  start(); await screen.findByRole("alert");
  expect(screen.getByRole("alert").textContent).toContain("检查项目和工作区目录");
  expect(mocks.confirm).not.toHaveBeenCalled();
});

test("repair opens the existing workspace manager after closing review", async () => {
  const manage = vi.fn();
  render(<WorkspaceRuntimeRecovery locale="en" sessionId="s1" onManage={manage} />);
  start(); await screen.findByText("/extra");
  fireEvent.click(screen.getByRole("button", { name: "Workspace roots…" }));
  expect(manage).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(mocks.confirm).not.toHaveBeenCalled();
});
