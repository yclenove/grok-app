/** @vitest-environment jsdom */
import "@testing-library/jest-dom/vitest";
import { useState, type ComponentProps } from "react";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import { createT, loadLocaleCatalog } from "@/i18n";
import { GlassModal } from "@/components/GlassModal";
import { SessionImportResults } from "./SessionImportResults";

type Props = ComponentProps<typeof SessionImportResults>;
const tr = createT("en");
function makeProps(): Props {
  return {
    tr, titleByCandidate: new Map([["known", "Known title"]]), active: false, busy: false, itemsStale: false, hasPrevious: false,
    job: { jobId: "one", requestId: "request", planHash: "hash", state: "completed_with_issues", stateVersion: 5,
      createdAt: "2026-09-25T00:00:00Z", itemCount: 1000, progress: { done: 1000, retryable: 900 }, nextItemCursor: "next",
      items: [{ candidateId: "known", phase: "committed", importId: "imp:one", outcome: "complete" }] },
    onPrevious: vi.fn(), onNext: vi.fn(), onRefresh: vi.fn(), onCancel: vi.fn(), onRetry: vi.fn(), onUndo: vi.fn(),
    onDone: vi.fn(), onOpen: vi.fn(), onContinue: vi.fn(), onExport: vi.fn().mockResolvedValue(undefined),
  };
}
afterEach(cleanup);

it.each(["partial", "unsupported"])("keeps a %s import out of the success group and explains its limitations", (outcome) => {
  const p = makeProps();
  p.job.items[0].outcome = outcome;
  render(<SessionImportResults {...p} />);
  const region = screen.getByRole("region", { name: tr("sessionImport.outcomeGroup.partial") });
  expect(within(region).getByText(tr(outcome === "partial"
    ? "sessionImport.err.partialHistory" : "sessionImport.err.schemaUnsupported"))).toBeVisible();
  expect(screen.queryByRole("region", { name: tr("sessionImport.outcomeGroup.complete") })).toBeNull();
  fireEvent.click(within(region).getByRole("button", { name: tr("sessionImport.openReader") }));
  expect(p.onOpen).toHaveBeenCalledWith("imp:one");
});

it.each([
  ["FORMAT_UNSUPPORTED: empty-jsonl", "sessionImport.diagnostic.empty"],
  ["FORMAT_UNSUPPORTED: binary-record", "sessionImport.diagnostic.binary"],
  ["FORMAT_UNSUPPORTED: invalid-utf8", "sessionImport.diagnostic.invalidUtf8"],
  ["RESOURCE_LIMIT: jsonl-record-too-large", "sessionImport.diagnostic.oversizedLine"],
  ["SOURCE_CHANGED: rewritten during read", "sessionImport.err.sourceChanged"],
] as const)("shows a localized failure for %s without reader actions", (error, key) => {
  const p = makeProps();
  p.job.items = [{ candidateId: "broken", phase: "failed", error }];
  render(<SessionImportResults {...p} />);
  const region = screen.getByRole("region", { name: tr("sessionImport.outcomeGroup.failed") });
  expect(within(region).getByText(tr(key))).toBeVisible();
  expect(screen.queryByRole("button", { name: tr("sessionImport.openReader") })).toBeNull();
});

it.each([
  ["en", "SOURCE_BUSY", "An import resource is busy or locked"],
  ["zh", "SOURCE_BUSY", "导入资源正忙或被锁定"],
  ["en", "DISK_SPACE_LOW", "Not enough disk space"],
  ["zh", "DISK_SPACE_LOW", "磁盘空间不足"],
] as const)("shows a safe %s %s error and retries only after explicit confirmation", async (locale, code, message) => {
  await loadLocaleCatalog(locale);
  const p = makeProps();
  const user = userEvent.setup();
  const localTr = createT(locale);
  p.tr = localTr;
  p.job = {
    ...p.job, itemCount: 1, progress: { done: 1, retryable: 1 }, nextItemCursor: null,
    items: [{ candidateId: "failed", title: "Synthetic import", importId: "imp:reserved", phase: "failed",
      error: `${code}: C:\\synthetic-private\\import\\catalog.sqlite: raw host diagnostic` }],
  };
  const view = render(<SessionImportResults {...p} />);
  const failed = screen.getByRole("region", { name: localTr("sessionImport.outcomeGroup.failed") });
  expect(within(failed).getByText(message)).toBeVisible();
  expect(view.container).not.toHaveTextContent(/synthetic-private|raw host diagnostic|SOURCE_BUSY|DISK_SPACE_LOW/);
  expect(screen.queryByRole("button", { name: localTr("sessionImport.openReader") })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: localTr("sessionImport.continueInGrok") })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: localTr("sessionImport.exportMarkdown") })).not.toBeInTheDocument();

  view.rerender(<SessionImportResults {...p} />);
  expect(p.onRetry).not.toHaveBeenCalled();
  await user.click(screen.getByRole("button", { name: localTr("sessionImport.retry") }));
  expect(p.onRetry).not.toHaveBeenCalled();
  await user.keyboard("{Escape}");
  expect(p.onRetry).not.toHaveBeenCalled();
  const retry = screen.getByRole("button", { name: localTr("sessionImport.retry") });
  expect(retry).toHaveFocus();
  await user.click(retry);
  view.rerender(<SessionImportResults {...p} busy />);
  const confirm = screen.getByRole("button", { name: localTr("sessionImport.confirmRetry") });
  expect(confirm).toBeDisabled();
  await user.click(confirm);
  expect(p.onRetry).not.toHaveBeenCalled();
  view.rerender(<SessionImportResults {...p} />);
  await user.click(screen.getByRole("button", { name: localTr("sessionImport.confirmRetry") }));
  expect(p.onRetry).toHaveBeenCalledOnce();
  expect(p.onOpen).not.toHaveBeenCalled();
  expect(p.onContinue).not.toHaveBeenCalled();
  expect(p.onExport).not.toHaveBeenCalled();
  expect(p.onUndo).not.toHaveBeenCalled();
});

it("cancelled items use the remaining-items action and still require confirmation", async () => {
  const user = userEvent.setup();
  const props = makeProps();
  props.job = { ...props.job, state: "cancelled", itemCount: 2, progress: { done: 1, retryable: 1 },
    items: [{ candidateId: "stopped", phase: "cancelled" }], nextItemCursor: null };
  render(<SessionImportResults {...props} />);
  await user.click(screen.getByRole("button", { name: "Retry remaining" }));
  expect(props.onRetry).not.toHaveBeenCalled();
  await user.click(screen.getByRole("button", { name: tr("sessionImport.confirmRetry") }));
  expect(props.onRetry).toHaveBeenCalledOnce();
});

it("groups only the loaded page, preserving within-group ordering and global progress", () => {
  const p = makeProps();
  p.job.items = [
    { candidateId: "a", title: "First complete", phase: "committed", outcome: "complete" },
    { candidateId: "b", title: "Bad", phase: "failed", error: "SOURCE_BUSY: private path" },
    { candidateId: "c", title: "Second complete", phase: "committed", outcome: "deduplicated" },
    { candidateId: "d", title: "Partial", phase: "committed", outcome: "partial" },
    { candidateId: "e", title: "Waiting", phase: "queued" },
    { candidateId: "f", title: "Cancelled row", phase: "cancelled" },
  ];
  render(<SessionImportResults {...p} />);
  expect(screen.getByText(tr("sessionImport.results.pageSummary", { n: 6 }))).toBeVisible();
  expect(screen.getByText("1000 / 1000 processed")).toBeVisible();
  expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "1000");
  const regions = screen.getAllByRole("region");
  expect(regions.map((r) => r.dataset.resultGroup)).toEqual(["failed", "partial", "pending", "stopped", "complete"]);
  const complete = screen.getByRole("region", { name: "Imported" });
  const rows = within(complete).getAllByRole("listitem");
  expect(rows).toHaveLength(2);
  expect(rows[0]).toHaveTextContent("First complete");
  expect(rows[1]).toHaveTextContent("Second complete");
  expect(rows[1]).toHaveTextContent("Already imported");
  expect(screen.queryByText(/private path/)).not.toBeInTheDocument();
});

it("does not resurrect undo/cancelled items through retained successful outcomes", () => {
  const p = makeProps();
  p.job.items = [
    { candidateId: "u", title: "Undo row", phase: "undone", outcome: "complete", importId: "imp:u" },
    { candidateId: "c", title: "Cancelled row", phase: "cancelled", outcome: "partial", importId: "imp:c" },
    { candidateId: "r", title: "Recover row", phase: "recovery_needed", outcome: "complete" },
  ];
  render(<SessionImportResults {...p} />);
  expect(within(screen.getByRole("region", { name: "Cancelled or undone" })).getAllByRole("listitem")).toHaveLength(2);
  expect(screen.getByText("Undone")).toBeVisible();
  expect(screen.getByText("Recovery needed")).toBeVisible();
  expect(screen.queryByRole("region", { name: "Imported" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: tr("sessionImport.openReader") })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: tr("sessionImport.continueInGrok") })).not.toBeInTheDocument();
});

it("routes published item actions by import id without invoking each other", async () => {
  const p = makeProps(); const user = userEvent.setup();
  render(<SessionImportResults {...p} />);
  expect(screen.getByText("Known title")).toBeVisible();
  await user.click(screen.getByRole("button", { name: tr("sessionImport.openReader") }));
  expect(p.onOpen).toHaveBeenCalledWith("imp:one");
  expect(p.onContinue).not.toHaveBeenCalled(); expect(p.onExport).not.toHaveBeenCalled();
  await user.click(screen.getByRole("button", { name: tr("sessionImport.continueInGrok") }));
  expect(p.onContinue).toHaveBeenCalledWith("imp:one"); expect(p.onOpen).toHaveBeenCalledTimes(1);
});

it("keeps unknown totals indeterminate and distinguishes queued from running", () => {
  const p = makeProps(); p.active = true;
  p.job = { ...p.job, state: "queued", items: [], itemCount: undefined, progress: undefined };
  const view = render(<SessionImportResults {...p} />);
  expect(screen.getByText("Queued")).toBeVisible();
  expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: tr("sessionImport.undo") })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: tr("sessionImport.cancel") }));
  expect(p.onCancel).toHaveBeenCalledOnce();
  view.rerender(<SessionImportResults {...p} job={{ ...p.job, state: "cancelling" }} />);
  expect(screen.getByRole("button", { name: tr("sessionImport.cancel") })).toBeDisabled();
});

it("shows recovery status and retains explicit retry without restarting the job", async () => {
  const p = makeProps(); const user = userEvent.setup();
  p.job = { ...p.job, state: "recovery_needed", items: [{ candidateId: "r", phase: "recovery_needed" }] };
  render(<SessionImportResults {...p} />);
  expect(screen.getByRole("status")).toHaveTextContent("Import needs recovery");
  expect(screen.queryByText(tr("sessionImport.job.running"))).not.toBeInTheDocument();
  expect(screen.getByRole("progressbar")).toHaveAttribute("data-state", "recovery_needed");
  expect(screen.queryByRole("button", { name: tr("sessionImport.cancel") })).not.toBeInTheDocument();
  expect(p.onRetry).not.toHaveBeenCalled();
  await user.click(screen.getByRole("button", { name: tr("sessionImport.retry") }));
  expect(p.onRetry).not.toHaveBeenCalled();
  await user.keyboard("{Enter}"); expect(p.onRetry).toHaveBeenCalledOnce();
});

it("keeps unknown job states visible without printing raw protocol values", () => {
  const p = makeProps(); p.job = { ...p.job, state: "internal_future_state" };
  render(<SessionImportResults {...p} />);
  expect(screen.getByRole("status")).toHaveTextContent("Import status unavailable");
  expect(screen.queryByText(tr("sessionImport.job.running"))).not.toBeInTheDocument();
  expect(screen.queryByText(/internal_future_state/)).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: tr("sessionImport.refresh") })).toBeEnabled();
});

it("shows an empty-page recovery instead of headings without items", () => {
  const p = makeProps(); p.job.items = []; p.hasPrevious = true;
  render(<SessionImportResults {...p} />);
  expect(screen.getByText(tr("sessionImport.results.empty"))).toBeVisible();
  expect(screen.queryByRole("region")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: tr("sessionImport.previousPage") }));
  fireEvent.click(screen.getByRole("button", { name: tr("sessionImport.refresh") }));
  expect(p.onPrevious).toHaveBeenCalledOnce(); expect(p.onRefresh).toHaveBeenCalledOnce();
});

it("disables stale item and mutation actions without blocking page recovery", () => {
  const p = makeProps(); p.itemsStale = true;
  render(<SessionImportResults {...p} />);
  expect(screen.getByText(tr("sessionImport.results.stale"))).toBeVisible();
  for (const key of ["openReader", "continueInGrok", "exportMarkdown", "retry", "undo"] as const) {
    expect(screen.getByRole("button", { name: tr(`sessionImport.${key}`) })).toBeDisabled();
  }
  expect(screen.getByRole("button", { name: tr("sessionImport.refresh") })).toBeEnabled();
  expect(screen.getByRole("button", { name: tr("sessionImport.nextPage") })).toBeEnabled();
});

it("moves keyboard focus into confirmation, cancels locally with Escape and restores focus", async () => {
  const p = makeProps(); const user = userEvent.setup();
  const escaped = vi.fn();
  render(<div onKeyDown={escaped}><SessionImportResults {...p} /></div>);
  const retry = screen.getByRole("button", { name: tr("sessionImport.retry") });
  retry.focus(); await user.keyboard("{Enter}");
  expect(screen.getByRole("button", { name: tr("sessionImport.confirmRetry") })).toHaveFocus();
  expect(p.onRetry).not.toHaveBeenCalled(); escaped.mockClear();
  await user.keyboard("{Escape}");
  expect(escaped).not.toHaveBeenCalled();
  expect(screen.getByRole("button", { name: tr("sessionImport.retry") })).toHaveFocus();
  await user.keyboard("{Enter}{Enter}");
  expect(p.onRetry).toHaveBeenCalledOnce(); expect(p.onUndo).not.toHaveBeenCalled();
});

it("undo requires explicit confirmation and is mutually exclusive with retry", async () => {
  const p = makeProps(); const user = userEvent.setup(); render(<SessionImportResults {...p} />);
  await user.click(screen.getByRole("button", { name: tr("sessionImport.undo") }));
  expect(p.onUndo).not.toHaveBeenCalled();
  expect(screen.queryByRole("button", { name: tr("sessionImport.retry") })).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: tr("common.cancel") }));
  expect(screen.getByRole("button", { name: tr("sessionImport.undo") })).toHaveFocus();
  await user.click(screen.getByRole("button", { name: tr("sessionImport.undo") }));
  await user.keyboard("{Enter}"); expect(p.onUndo).toHaveBeenCalledOnce();
});

it("invalidates armed confirmations on authoritative job version changes", async () => {
  const p = makeProps(); const user = userEvent.setup();
  const view = render(<SessionImportResults {...p} />);
  await user.click(screen.getByRole("button", { name: tr("sessionImport.undo") }));
  view.rerender(<SessionImportResults {...p} job={{ ...p.job, stateVersion: 6 }} />);
  expect(screen.queryByRole("button", { name: tr("sessionImport.confirmUndo") })).not.toBeInTheDocument();
  expect(p.onUndo).not.toHaveBeenCalled();
});

it("serializes clipboard exports, reports success only after completion and permits retry on failure", async () => {
  const p = makeProps(); const user = userEvent.setup(); let finish!: () => void;
  p.onExport = vi.fn(() => new Promise<void>((resolve) => { finish = resolve; }));
  const view = render(<SessionImportResults {...p} />);
  const copy = screen.getByRole("button", { name: tr("sessionImport.exportMarkdown") });
  await user.click(copy); fireEvent.click(copy);
  expect(p.onExport).toHaveBeenCalledOnce(); expect(copy).toBeDisabled();
  expect(screen.queryByText(tr("sessionImport.copiedExport"))).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: tr("sessionImport.undo") })).toBeDisabled();
  await act(async () => finish());
  expect(screen.getByText(tr("sessionImport.copiedExport"))).toBeVisible(); expect(copy).toBeEnabled();
  view.rerender(<SessionImportResults {...p} onExport={vi.fn().mockRejectedValue(new Error("secret path"))} />);
  await user.click(copy);
  expect(screen.getByRole("alert")).toHaveTextContent(tr("sessionImport.results.copyFailed"));
  expect(screen.queryByText(/secret path/)).not.toBeInTheDocument(); expect(copy).toBeEnabled();
  expect(screen.queryByText(tr("sessionImport.copiedExport"))).not.toBeInTheDocument();
});

it("does not expose unrecognized internal phase or outcome names", () => {
  const p = makeProps(); p.job.items = [
    { candidateId: "a", title: "A", phase: "internal_phase" },
    { candidateId: "b", title: "B", phase: "committed", outcome: "internal_outcome" },
  ];
  render(<SessionImportResults {...p} />);
  expect(screen.getAllByText("Status unavailable")).toHaveLength(2);
  expect(screen.queryByText(/internal_/)).not.toBeInTheDocument();
});

function ResultsWithReader({ resultProps }: { resultProps: Props }) {
  const [readerOpen, setReaderOpen] = useState(false);
  return <>
    <GlassModal open title="Import results" initialFocus="none" onClose={() => {}}>
      <SessionImportResults {...resultProps} onOpen={() => setReaderOpen(true)} />
    </GlassModal>
    <GlassModal open={readerOpen} title="Read-only history" initialFocus="none" onClose={() => setReaderOpen(false)}>
      <input aria-label="Reader search" />
    </GlassModal>
  </>;
}

it.each([false, true])("does not return confirmation focus below an open reader (focus lost: %s)", async (focusLost) => {
  const p = makeProps(); const user = userEvent.setup();
  const view = render(<ResultsWithReader resultProps={p} />);
  await user.click(screen.getByRole("button", { name: tr("sessionImport.undo") }));
  expect(screen.getByRole("button", { name: tr("sessionImport.confirmUndo") })).toHaveFocus();
  await user.click(screen.getByRole("button", { name: tr("sessionImport.openReader") }));
  const search = screen.getByRole("textbox", { name: "Reader search" });
  search.focus();
  if (focusLost) search.blur();
  expect(document.activeElement).toBe(focusLost ? document.body : search);
  view.rerender(<ResultsWithReader resultProps={{ ...p, job: { ...p.job, stateVersion: 6 } }} />);
  expect(screen.queryByRole("button", { name: tr("sessionImport.confirmUndo") })).not.toBeInTheDocument();
  expect(document.activeElement).toBe(focusLost ? document.body : search);
  expect(p.onUndo).not.toHaveBeenCalled(); expect(p.onRetry).not.toHaveBeenCalled();
  search.focus(); await user.keyboard("{Escape}");
  expect(screen.queryByRole("dialog", { name: "Read-only history" })).not.toBeInTheDocument();
  expect(screen.getByRole("dialog", { name: "Import results" })).toBeVisible();
  expect(screen.getByRole("button", { name: tr("sessionImport.openReader") })).toHaveFocus();
});

it("does not steal deliberate focus elsewhere on the result page when a job update disarms confirmation", async () => {
  const p = makeProps(); const user = userEvent.setup();
  const view = render(<SessionImportResults {...p} />);
  await user.click(screen.getByRole("button", { name: tr("sessionImport.retry") }));
  const refresh = screen.getByRole("button", { name: tr("sessionImport.refresh") });
  await user.click(refresh); expect(refresh).toHaveFocus();
  view.rerender(<SessionImportResults {...p} job={{ ...p.job, stateVersion: 6 }} />);
  expect(screen.queryByRole("button", { name: tr("sessionImport.confirmRetry") })).not.toBeInTheDocument();
  expect(refresh).toHaveFocus(); expect(p.onRefresh).toHaveBeenCalledOnce();
  expect(p.onRetry).not.toHaveBeenCalled();
});

it.each(["retry", "undo"] as const)("restores %s focus when a job update removes the actually focused confirmation", async (action) => {
  const p = makeProps(); const user = userEvent.setup();
  const view = render(<SessionImportResults {...p} />);
  await user.click(screen.getByRole("button", { name: tr(`sessionImport.${action}`) }));
  const confirm = screen.getByRole("button", { name: tr(action === "retry" ? "sessionImport.confirmRetry" : "sessionImport.confirmUndo") });
  expect(confirm).toHaveFocus();
  view.rerender(<SessionImportResults {...p} job={{ ...p.job, stateVersion: 6 }} />);
  expect(confirm).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: tr(`sessionImport.${action}`) })).toHaveFocus();
  expect(p.onRetry).not.toHaveBeenCalled(); expect(p.onUndo).not.toHaveBeenCalled();
});

it.each([
  { isComposing: true, keyCode: 27 },
  { isComposing: false, keyCode: 229 },
])("leaves IME Escape to the input method without disarming confirmation: %j", async (init) => {
  const p = makeProps(); const user = userEvent.setup(); const close = vi.fn();
  render(<GlassModal open title="Import results" initialFocus="none" onClose={close}>
    <SessionImportResults {...p} />
  </GlassModal>);
  await user.click(screen.getByRole("button", { name: tr("sessionImport.undo") }));
  const confirm = screen.getByRole("button", { name: tr("sessionImport.confirmUndo") });
  const event = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true, ...init });
  fireEvent(confirm, event);
  expect(event.defaultPrevented).toBe(false);
  expect(confirm).toBeInTheDocument(); expect(confirm).toHaveFocus();
  expect(close).not.toHaveBeenCalled(); expect(p.onUndo).not.toHaveBeenCalled();
  await user.keyboard("{Escape}");
  expect(confirm).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: tr("sessionImport.undo") })).toHaveFocus();
  expect(close).not.toHaveBeenCalled();
});

it("does not consume Escape already handled by the focused control", async () => {
  const p = makeProps(); const user = userEvent.setup(); const close = vi.fn();
  render(<GlassModal open title="Import results" initialFocus="none" onClose={close}>
    <SessionImportResults {...p} />
  </GlassModal>);
  await user.click(screen.getByRole("button", { name: tr("sessionImport.retry") }));
  const confirm = screen.getByRole("button", { name: tr("sessionImport.confirmRetry") });
  const consume = (event: Event) => event.preventDefault();
  confirm.addEventListener("keydown", consume);
  try {
    fireEvent.keyDown(confirm, { key: "Escape" });
    expect(confirm).toBeInTheDocument(); expect(confirm).toHaveFocus();
    expect(close).not.toHaveBeenCalled(); expect(p.onRetry).not.toHaveBeenCalled();
  } finally {
    confirm.removeEventListener("keydown", consume);
  }
  await user.keyboard("{Escape}");
  expect(confirm).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: tr("sessionImport.retry") })).toHaveFocus();
  expect(close).not.toHaveBeenCalled();
});
