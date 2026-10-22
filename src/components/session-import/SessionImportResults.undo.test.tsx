/** @vitest-environment jsdom */
import "@testing-library/jest-dom/vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { createT, loadLocaleCatalog, type MessageKey } from "@/i18n";
import { SessionImportResults } from "./SessionImportResults";

afterEach(cleanup);

// Exact Host text from the durable undo record (jobs_publish.rs).
const reasons: [string, MessageKey][] = [
  ["MAPPING_CONFLICT: continuation sessions still reference this import", "sessionImport.continuationConflict"],
  ["later revision kept", "sessionImport.undoReason.laterRevision"],
  ["ownership receipt missing", "sessionImport.undoReason.ownership"],
  ["previous revision is unknown", "sessionImport.undoReason.ownership"],
];

it.each(["en", "zh"] as const)("names the exact reason an item was kept by Undo (%s)", async (locale) => {
  await loadLocaleCatalog(locale);
  const tr = createT(locale);
  const action = vi.fn();
  const view = render(<SessionImportResults
    tr={tr} titleByCandidate={new Map()} active={false} busy={false} itemsStale={false} hasPrevious={false}
    job={{ jobId: "synthetic", requestId: "synthetic-request", planHash: "synthetic-plan",
      state: "completed", stateVersion: 3, createdAt: "2026-10-01T00:00:00Z",
      itemCount: reasons.length, progress: { done: reasons.length, retryable: 0 }, nextItemCursor: null,
      items: reasons.map(([error], index) => ({ candidateId: `c${index}`, title: `Kept ${index}`, phase: "conflict",
        importId: `imp:${index}`, error })) }}
    onPrevious={action} onNext={action} onRefresh={action} onCancel={action} onRetry={action}
    onUndo={action} onDone={action} onOpen={action} onContinue={action} onExport={vi.fn()}
  />);
  for (const [index, [, key]] of reasons.entries()) {
    const row = screen.getByText(`Kept ${index}`).closest("li")!;
    expect(row).toHaveTextContent(tr("sessionImport.item.notUndone"));
    expect(row.querySelector('p[data-issue="unsupported"]')?.textContent).toBe(tr(key));
  }
  // The generic project-mapping copy no longer stands in for an Undo reason.
  expect(view.container).not.toHaveTextContent(tr("sessionImport.err.mappingConflict"));
  expect(screen.queryByRole("button", { name: tr("sessionImport.openReader") })).not.toBeInTheDocument();
});
