/** @vitest-environment jsdom */
import "@testing-library/jest-dom/vitest";
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { createT } from "@/i18n";
import { LOCALES } from "@/i18n/messages";
import type { PrepareBatchItemPage, PrepareBatchStatus } from "@/lib/api/sessionMigration";
import { useSessionImportPrepare } from "@/hooks/useSessionImportPrepare";
import { SessionImportPrepareStep } from "./SessionImportPrepareStep";

const mocks = vi.hoisted(() => ({
  start: vi.fn(), get: vi.fn(), items: vi.fn(), cancel: vi.fn(), retry: vi.fn(),
  onError: vi.fn(), onContinue: vi.fn(), onBack: vi.fn(),
}));
vi.mock("@/lib/api/sessionMigration", () => ({
  sessionImportPrepareStart: mocks.start, sessionImportPrepareGet: mocks.get,
  sessionImportPrepareItems: mocks.items, sessionImportPrepareCancel: mocks.cancel,
  sessionImportPrepareRetry: mocks.retry,
}));

const completed: PrepareBatchStatus = {
  version: 1, batchId: "batch-a", scanId: "scan-a", epoch: 1,
  selectionId: "selection-a", selectionStateVersion: 1,
  state: "completed-with-issues", stateVersion: 2,
  total: 2, prepared: 1, failed: 1, skipped: 0, active: 0, cancelRequested: false,
  createdAt: "2026-09-20T00:00:00Z", updatedAt: "2026-09-20T00:00:00Z",
};
const results: PrepareBatchItemPage = { items: [{
  candidateId: "issue-a", title: "Failed source", provider: "codex",
  sourceRevision: "revision-a", state: "failed", attempt: 1, previewId: null,
  error: { code: "SOURCE_BUSY", message: "locked", retry: "retry", scope: "candidate" },
}], nextCursor: null };

function Harness({ initial = completed }: { initial?: PrepareBatchStatus }) {
  const prepare = useSessionImportPrepare(mocks.onError);
  return prepare.batch ? (
    <SessionImportPrepareStep
      tr={createT("en")} status={prepare.batch} items={prepare.items}
      itemsError={prepare.itemsError} nextCursor={prepare.nextCursor}
      busy={prepare.busy} active={prepare.active}
      onCancel={() => void prepare.cancel()} onRetry={() => void prepare.retry()}
      onContinue={mocks.onContinue} onBack={mocks.onBack}
      onLoadMore={() => void prepare.loadMore()} onRetryItems={() => void prepare.retryItems()}
    />
  ) : <button onClick={() => prepare.adopt(initial)}>Open preparation</button>;
}

beforeEach(() => vi.resetAllMocks());
afterEach(cleanup);

test("preparation separates processed work from ready, failed and skipped counts", () => {
  render(<SessionImportPrepareStep
    tr={createT("en")} status={{ ...completed, total: 4, prepared: 2, failed: 1, skipped: 1 }}
    items={[]} itemsError={false} nextCursor="more-results" busy={false} active={false}
    onCancel={vi.fn()} onRetry={vi.fn()} onContinue={vi.fn()} onBack={vi.fn()}
    onLoadMore={vi.fn()} onRetryItems={vi.fn()}
  />);
  expect(screen.getByText("4 / 4 processed")).toBeInTheDocument();
  expect(screen.getByText("2 ready · 1 failed · 1 skipped")).toBeInTheDocument();
  expect(screen.queryByText("4 / 4 prepared")).not.toBeInTheDocument();
  expect(screen.getByRole("progressbar", { name: "Prepare" })).toHaveAttribute("aria-valuenow", "4");
  expect(screen.getByRole("button", { name: "Continue with prepared" })).toBeEnabled();
});

test("active preparation uses authoritative totals even before item pages arrive", () => {
  render(<SessionImportPrepareStep
    tr={createT("en")} status={{ ...completed, state: "preparing", total: 100, prepared: 7, failed: 2, skipped: 1 }}
    items={[]} itemsError={false} nextCursor={null} busy={false} active
    onCancel={vi.fn()} onRetry={vi.fn()} onContinue={vi.fn()} onBack={vi.fn()}
    onLoadMore={vi.fn()} onRetryItems={vi.fn()}
  />);
  expect(screen.getByText("10 / 100 processed")).toBeInTheDocument();
  expect(screen.getByText("7 ready · 2 failed · 1 skipped")).toBeInTheDocument();
  expect(screen.getByRole("progressbar", { name: "Prepare" })).toHaveAttribute("aria-valuemax", "100");
  expect(screen.queryByRole("button", { name: "Continue with prepared" })).not.toBeInTheDocument();
});

test.each([
  ["queued", "Waiting to prepare…"],
  ["preparing", "Preparing selected sessions…"],
  ["cancelling", "Cancelling…"],
] as const)("%s preparation renders its own status", (state, label) => {
  render(<SessionImportPrepareStep
    tr={createT("en")} status={{ ...completed, state }}
    items={[]} itemsError={false} nextCursor={null} busy={false} active
    onCancel={vi.fn()} onRetry={vi.fn()} onContinue={vi.fn()} onBack={vi.fn()}
    onLoadMore={vi.fn()} onRetryItems={vi.fn()}
  />);
  expect(screen.getByText(label)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: createT("en")("sessionImport.cancel") }).hasAttribute("disabled"))
    .toBe(state === "cancelling");
});

test("queued preparation does not imply it is waiting for import publication", () => {
  render(<SessionImportPrepareStep
    tr={createT("zh")} status={{ ...completed, state: "queued" }}
    items={[]} itemsError={false} nextCursor={null} busy={false} active
    onCancel={vi.fn()} onRetry={vi.fn()} onContinue={vi.fn()} onBack={vi.fn()}
    onLoadMore={vi.fn()} onRetryItems={vi.fn()}
  />);
  expect(screen.getByText("等待准备…")).toBeInTheDocument();
  expect(screen.queryByText("等待导入")).not.toBeInTheDocument();
});

test("cancel stays disabled during the request and after cancellation is acknowledged", async () => {
  let resolve!: (status: PrepareBatchStatus) => void;
  const pending = new Promise<PrepareBatchStatus>((done) => { resolve = done; });
  const preparing: PrepareBatchStatus = { ...completed, state: "preparing" };
  const cancelling: PrepareBatchStatus = { ...completed, state: "cancelling", stateVersion: 3 };
  mocks.cancel.mockReturnValueOnce(pending);
  mocks.get.mockResolvedValue(preparing);
  const user = userEvent.setup();
  render(<Harness initial={preparing} />);
  await user.click(screen.getByRole("button", { name: "Open preparation" }));
  const cancel = screen.getByRole("button", { name: createT("en")("sessionImport.cancel") });
  await user.click(cancel);
  expect(cancel).toBeDisabled();
  await user.dblClick(cancel);
  expect(mocks.cancel).toHaveBeenCalledTimes(1);
  await act(async () => resolve(cancelling));
  expect(cancel).toBeDisabled();
  expect(screen.getByText("Cancelling…")).toBeInTheDocument();
  await user.dblClick(cancel);
  expect(mocks.cancel).toHaveBeenCalledTimes(1);
});

test("keyboard retry reloads results without retrying the preparation itself", async () => {
  const user = userEvent.setup();
  mocks.items.mockRejectedValueOnce(new Error("temporary read failure")).mockResolvedValueOnce(results);
  render(<Harness />);
  await user.click(screen.getByRole("button", { name: "Open preparation" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Could not load preparation results.");
  const retry = screen.getByRole("button", { name: "Retry loading results" });
  retry.focus();
  await user.keyboard("{Enter}");
  expect(await screen.findByText("Failed source")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Retry loading results" })).toBeNull();
  expect(mocks.items).toHaveBeenCalledTimes(2);
  expect(mocks.retry).not.toHaveBeenCalled();
  expect(mocks.start).not.toHaveBeenCalled();
});

test("a pending results retry keeps prepare actions disabled until it finishes", async () => {
  let resolve!: (page: PrepareBatchItemPage) => void;
  const pending = new Promise<PrepareBatchItemPage>((done) => { resolve = done; });
  mocks.items.mockRejectedValueOnce(new Error("temporary read failure")).mockReturnValueOnce(pending);
  const user = userEvent.setup();
  render(<Harness />);
  await user.click(screen.getByRole("button", { name: "Open preparation" }));
  await user.click(await screen.findByRole("button", { name: "Retry loading results" }));
  expect(screen.getByRole("button", { name: createT("en")("sessionImport.retry") })).toBeDisabled();
  expect(screen.getByRole("button", { name: "Continue with prepared" })).toBeDisabled();
  await act(async () => resolve(results));
  await waitFor(() => expect(screen.getByRole("button", { name: "Continue with prepared" })).toBeEnabled());
});

test.each(LOCALES)("results retry renders translated chrome in %s", (locale) => {
  const tr = createT(locale);
  render(<SessionImportPrepareStep
    tr={tr} status={completed} items={[]} itemsError nextCursor={null} busy={false} active={false}
    onCancel={vi.fn()} onRetry={vi.fn()} onContinue={vi.fn()} onBack={vi.fn()}
    onLoadMore={vi.fn()} onRetryItems={vi.fn()}
  />);
  expect(screen.getByRole("alert")).toHaveTextContent(tr("sessionImport.prepare.resultsError"));
  expect(screen.getByRole("button", { name: tr("sessionImport.prepare.retryResults") })).toBeEnabled();
});
