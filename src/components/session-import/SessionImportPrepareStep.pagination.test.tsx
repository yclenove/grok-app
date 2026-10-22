/** @vitest-environment jsdom */
import "@testing-library/jest-dom/vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import { createT } from "@/i18n";
import type { PrepareBatchItem, PrepareBatchStatus } from "@/lib/api/sessionMigration";
import { SessionImportPrepareStep } from "./SessionImportPrepareStep";

afterEach(cleanup);

const completed: PrepareBatchStatus = {
  version: 1, batchId: "pagination-batch", scanId: "pagination-scan", epoch: 1,
  selectionId: "pagination-selection", selectionStateVersion: 1,
  state: "completed", stateVersion: 2, total: 1200, prepared: 1200,
  failed: 0, skipped: 0, active: 0, cancelRequested: false,
  createdAt: "2026-10-01T00:00:00Z", updatedAt: "2026-10-01T00:02:00Z",
};

function item(candidateId: string, state: PrepareBatchItem["state"]): PrepareBatchItem {
  return {
    candidateId, title: candidateId, provider: "generic-file", sourceRevision: "synthetic-revision",
    state, attempt: 1, previewId: state === "prepared" ? `preview-${candidateId}` : null,
    error: state === "failed"
      ? { code: "SOURCE_BUSY", message: "synthetic lock", retry: "retry", scope: "candidate" }
      : null,
  };
}

function show(options: {
  status?: PrepareBatchStatus;
  items?: PrepareBatchItem[];
  nextCursor?: string | null;
  itemsError?: boolean;
  busy?: boolean;
} = {}) {
  const actions = {
    onCancel: vi.fn(), onRetry: vi.fn(), onContinue: vi.fn(), onBack: vi.fn(),
    onLoadMore: vi.fn(), onRetryItems: vi.fn(),
  };
  render(<SessionImportPrepareStep
    tr={createT("en")} status={options.status ?? completed}
    items={options.items ?? [item("ready", "prepared")]}
    nextCursor={options.nextCursor === undefined ? "remaining-prepared-page" : options.nextCursor}
    itemsError={options.itemsError ?? false} busy={options.busy ?? false} active={false}
    {...actions}
  />);
  return actions;
}

test("all-ready preparation hides load more even when unrendered prepared pages remain", () => {
  show();
  expect(screen.getByText("1200 ready · 0 failed · 0 skipped")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Load more" })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Continue with prepared" })).toBeEnabled();
});

test("loading every issue removes pagination for remaining successful rows", () => {
  show({ status: { ...completed, state: "completed-with-issues", prepared: 1198, failed: 1, skipped: 1 },
    items: [item("ready", "prepared"), item("failed", "failed"), item("skipped", "skipped")] });
  expect(screen.getByText("failed")).toBeInTheDocument();
  expect(screen.getByText("skipped")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Load more" })).not.toBeInTheDocument();
});

test("an off-page issue keeps keyboard pagination available without starting another action", async () => {
  const user = userEvent.setup();
  const actions = show({ status: { ...completed, state: "completed-with-issues", prepared: 1199, failed: 1 } });
  const more = screen.getByRole("button", { name: "Load more" });
  more.focus();
  await user.keyboard("{Enter}");
  expect(actions.onLoadMore).toHaveBeenCalledTimes(1);
  expect(actions.onContinue).not.toHaveBeenCalled();
  expect(actions.onRetry).not.toHaveBeenCalled();
  expect(actions.onCancel).not.toHaveBeenCalled();
  expect(actions.onBack).not.toHaveBeenCalled();
});

test("unloaded skipped items still expose pagination after cancellation", () => {
  show({ status: { ...completed, state: "cancelled", prepared: 1198, skipped: 2, cancelRequested: true },
    items: [item("skipped-one", "skipped")] });
  expect(screen.getByRole("button", { name: "Load more" })).toBeEnabled();
});

test("results read failures retain their independent retry even with zero issues", async () => {
  const user = userEvent.setup();
  const actions = show({ itemsError: true });
  expect(screen.queryByRole("button", { name: "Load more" })).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Retry loading results" }));
  expect(actions.onRetryItems).toHaveBeenCalledTimes(1);
  expect(actions.onRetry).not.toHaveBeenCalled();
});

test("pending issue reads keep load more disabled", () => {
  show({ status: { ...completed, state: "completed-with-issues", prepared: 1199, failed: 1 }, busy: true });
  expect(screen.getByRole("button", { name: "Load more" })).toBeDisabled();
});

test("a terminal page never offers another page even if issues are missing", () => {
  show({ status: { ...completed, state: "completed-with-issues", prepared: 1199, failed: 1 }, nextCursor: null });
  expect(screen.queryByRole("button", { name: "Load more" })).not.toBeInTheDocument();
});
