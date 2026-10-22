/** @vitest-environment jsdom */
import "@testing-library/jest-dom/vitest";
import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import { SessionImportJobHistory } from "./SessionImportJobHistory";

const mocks = vi.hoisted(() => ({ page: vi.fn() }));
vi.mock("@/lib/api/sessionMigrationJobs", () => ({ sessionImportJobHistory: mocks.page }));
afterEach(() => { cleanup(); vi.resetAllMocks(); });

test("keyboard opens the dialog, pages older jobs without selection and restores focus on Escape", async () => {
  const user = userEvent.setup();
  const one = { jobId: "new", state: "completed", createdAt: "2026-10-16T00:00:00Z", itemCount: 3 };
  mocks.page.mockResolvedValueOnce({ items: [one], nextCursor: "older" })
    .mockResolvedValueOnce({ items: [{ ...one, jobId: "old", itemCount: 2, createdAt: null }], nextCursor: null });
  const select = vi.fn().mockResolvedValue(true);
  render(<SessionImportJobHistory locale="en" enabled onSelect={select} />);
  const trigger = screen.getByRole("button", { name: "Import jobs" });
  trigger.focus();
  await user.keyboard("{Enter}");
  const dialog = await screen.findByRole("dialog", { name: "Import jobs" });
  await waitFor(() => expect(within(dialog).getByRole("button", { name: "Next page" })).toBeEnabled());
  expect(select).not.toHaveBeenCalled();
  await user.click(within(dialog).getByRole("button", { name: "Next page" }));
  const older = await within(dialog).findByRole("button", { name: /Unknown date.*2 sessions/ });
  expect(within(dialog).getByRole("button", { name: "Next page" })).toBeDisabled();
  await user.keyboard("{Escape}");
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(trigger).toHaveFocus();
  expect(older.isConnected).toBe(false);
  expect(select).not.toHaveBeenCalled();
});

test("Chinese errors and unknown metadata stay readable and selecting has no implicit confirmation", async () => {
  mocks.page.mockRejectedValueOnce(new Error("SOURCE_BUSY: secret/path"))
    .mockResolvedValueOnce({ items: [{ jobId: "hidden-id", state: null, createdAt: "invalid", itemCount: 3 }], nextCursor: null });
  const select = vi.fn().mockResolvedValue(true);
  const user = userEvent.setup();
  render(<SessionImportJobHistory locale="zh" enabled onSelect={select} />);
  await user.click(screen.getByRole("button", { name: "导入任务" }));
  const dialog = await screen.findByRole("dialog", { name: "导入任务" });
  await within(dialog).findByRole("alert");
  expect(dialog).not.toHaveTextContent("secret/path");
  await user.click(within(dialog).getByRole("button", { name: "重试" }));
  await user.click(await within(dialog).findByRole("button", { name: /日期未知.*3 个会话/ }));
  expect(select).toHaveBeenCalledWith("hidden-id", expect.any(Function));
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(screen.queryByText("hidden-id")).not.toBeInTheDocument();
});

test("disabled intent prevents reads; closing cancels selection ownership", async () => {
  mocks.page.mockResolvedValue({ items: [{ jobId: "one", state: "completed", createdAt: null, itemCount: 1 }], nextCursor: null });
  let resolve!: (accepted: boolean) => void;
  let owns!: () => boolean;
  const select = vi.fn((_id, current) => { owns = current; return new Promise<boolean>((ok) => { resolve = ok; }); });
  const user = userEvent.setup();
  const { rerender } = render(<SessionImportJobHistory locale="en" enabled={false} onSelect={select} />);
  expect(screen.getByRole("button", { name: "Import jobs" })).toBeDisabled();
  expect(mocks.page).not.toHaveBeenCalled();
  rerender(<SessionImportJobHistory locale="en" enabled onSelect={select} />);
  await user.click(screen.getByRole("button", { name: "Import jobs" }));
  await user.click(await screen.findByRole("button", { name: /Unknown date.*1 sessions/ }));
  expect(owns()).toBe(true);
  await user.keyboard("{Escape}");
  expect(owns()).toBe(false);
  await act(async () => resolve(false));
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});
