/** @vitest-environment jsdom */
import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { createT, loadLocaleCatalog } from "@/i18n";
import { SessionImportResults } from "./SessionImportResults";

afterEach(cleanup);

it.each([
  ["en", "constructor"],
  ["en", "toString"],
  ["en", "__proto__"],
  ["zh", "constructor"],
  ["zh", "toString"],
  ["zh", "__proto__"],
] as const)("renders a safe localized failure for %s %s without losing result actions", async (locale, code) => {
  await loadLocaleCatalog(locale);
  const tr = createT(locale);
  const action = vi.fn();
  const exportItem = vi.fn().mockResolvedValue(undefined);
  const view = render(<SessionImportResults
    tr={tr} titleByCandidate={new Map()} active={false} busy={false} itemsStale={false} hasPrevious={false}
    job={{ jobId: "synthetic", requestId: "synthetic-request", planHash: "synthetic-plan",
      state: "completed_with_issues", stateVersion: 1, createdAt: "2026-10-01T00:00:00Z",
      itemCount: 1, progress: { done: 1, retryable: 0 }, nextItemCursor: null,
      items: [{ candidateId: "synthetic-candidate", title: "Synthetic import", phase: "failed",
        importId: "imp:reserved", error: `${code}: C:\\synthetic-private\\catalog.sqlite: raw host diagnostic` }] }}
    onPrevious={action} onNext={action} onRefresh={action} onCancel={action} onRetry={action}
    onUndo={action} onDone={action} onOpen={action} onContinue={action} onExport={exportItem}
  />);
  const failed = screen.getByRole("region", { name: tr("sessionImport.outcomeGroup.failed") });
  const notice = failed.querySelector('p[data-issue="unsupported"]');
  expect(notice).toBeVisible();
  expect(notice?.textContent).toBe(tr("sessionImport.err.generic"));
  expect(view.container).not.toHaveTextContent(/synthetic-private|raw host diagnostic|native code|sessionImport\.err/);
  expect(screen.getByRole("button", { name: tr("sessionImport.refresh") })).toBeEnabled();
  expect(screen.getByRole("button", { name: tr("sessionImport.done") })).toBeEnabled();
  expect(screen.queryByRole("button", { name: tr("sessionImport.openReader") })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: tr("sessionImport.continueInGrok") })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: tr("sessionImport.exportMarkdown") })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: tr("sessionImport.retry") })).not.toBeInTheDocument();
  expect(action).not.toHaveBeenCalled();
  expect(exportItem).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: tr("sessionImport.refresh") }));
  fireEvent.click(screen.getByRole("button", { name: tr("sessionImport.done") }));
  expect(action).toHaveBeenCalledTimes(2);
  expect(exportItem).not.toHaveBeenCalled();
});
