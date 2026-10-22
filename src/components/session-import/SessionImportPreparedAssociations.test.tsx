/** @vitest-environment jsdom */
import "@testing-library/jest-dom/vitest";
import type { ComponentProps } from "react";
import { useState } from "react";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import { createT } from "@/i18n";
import type { PrepareBatchItem } from "@/lib/api/sessionMigration";
import { SessionImportPreparedAssociations } from "./SessionImportPreparedAssociations";
import { SessionImportMappingStep } from "./SessionImportMappingStep";

afterEach(cleanup);
const tr = createT("en");
type Ui = ComponentProps<typeof SessionImportPreparedAssociations>["ui"];
function members(n: number, provider = "cursor"): PrepareBatchItem[] {
  return Array.from({ length: n }, (_, i) => ({ candidateId: `candidate-${i}`, title: `Synthetic ${i}`,
    provider, sourceRevision: "rev", state: "prepared", attempt: 1, previewId: `preview-${i}` }));
}
function props(overrides: Partial<Ui> = {}): Ui {
  return { busy: false, prepareItems: members(120), prepareItemsError: false, prepareNextCursor: null,
    loadMorePrepare: vi.fn(async () => false), retryPrepareItems: vi.fn(async () => false),
    associations: { confirmed: [], open: vi.fn(), clear: vi.fn() }, ...overrides };
}
async function expand(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByText(tr("sessionImport.historyLink.title"), { selector: "summary" }));
}
function rows() { return within(screen.getByRole("list", { name: tr("sessionImport.historyLink.source") })); }

test("cached metadata stays at 50 rows; off-page confirmations remain visible and clearable", async () => {
  const user = userEvent.setup();
  const ui = props();
  ui.associations.confirmed = [{ candidateId: "candidate-0", title: "Synthetic 0", targetTitle: "Old history",
    confirmation: { version: 1, importId: "imp:old", expectedRevision: "rev", previewId: "preview-0", reviewDigest: "digest" } }];
  render(<SessionImportPreparedAssociations tr={tr} ui={ui} />); await expand(user);
  expect(rows().getAllByRole("listitem")).toHaveLength(50);
  expect(screen.getByRole("button", { name: tr("sessionImport.previousPage") })).toBeDisabled();
  await user.click(screen.getByRole("button", { name: tr("sessionImport.nextPage") }));
  expect(rows().getAllByRole("listitem")).toHaveLength(50);
  expect(rows().queryByText("Synthetic 0 → Old history")).toBeNull();
  expect(screen.getByText("Synthetic 0 → Old history")).toBeVisible();
  await user.click(screen.getByRole("button", { name: tr("sessionImport.association.clear") }));
  expect(ui.associations.clear).toHaveBeenCalledWith("candidate-0");
  await user.click(rows().getAllByRole("button")[0]);
  expect(ui.associations.open).toHaveBeenCalledWith(ui.prepareItems[50]);
  await user.click(screen.getByRole("button", { name: tr("sessionImport.nextPage") }));
  expect(rows().getAllByRole("listitem")).toHaveLength(20);
  expect(screen.getByRole("button", { name: tr("sessionImport.nextPage") })).toBeDisabled();
  await user.click(screen.getByRole("button", { name: tr("sessionImport.previousPage") }));
  expect(rows().getByText("Synthetic 50")).toBeVisible();
  expect(ui.loadMorePrepare).not.toHaveBeenCalled();
});

test("one next-page click fetches once and advances; loaded pages can be revisited without fetching", async () => {
  let resolve!: (value: boolean) => void;
  const load = vi.fn(() => new Promise<boolean>((done) => { resolve = done; }));
  function Harness() {
    const [items, setItems] = useState(members(50));
    return <SessionImportPreparedAssociations tr={tr} ui={props({ prepareItems: items, prepareNextCursor: "next",
      loadMorePrepare: async () => { const ok = await load(); if (ok) setItems(members(100)); return ok; } })} />;
  }
  const user = userEvent.setup(); render(<Harness />); await expand(user);
  const next = screen.getByRole("button", { name: tr("sessionImport.nextPage") });
  act(() => { fireEvent.click(next); fireEvent.click(next); });
  expect(load).toHaveBeenCalledOnce(); expect(next).toBeDisabled();
  await act(async () => resolve(true));
  expect(screen.getByText(tr("sessionImport.page", { n: 2 }))).toBeVisible();
  expect(rows().getByText("Synthetic 50")).toBeVisible();
  await user.click(screen.getByRole("button", { name: tr("sessionImport.previousPage") }));
  await user.click(next); expect(load).toHaveBeenCalledOnce();
});

test("a failed fetch keeps the visible page; retry goes to its original destination", async () => {
  const load = vi.fn(); const retry = vi.fn();
  function Harness() {
    const [items, setItems] = useState(members(50)); const [error, setError] = useState(false);
    return <SessionImportPreparedAssociations tr={tr} ui={props({ prepareItems: items,
      prepareNextCursor: "next", prepareItemsError: error,
      loadMorePrepare: async () => { load(); setError(true); return false; },
      retryPrepareItems: async () => { retry(); setItems(members(100)); setError(false); return true; } })} />;
  }
  const user = userEvent.setup(); render(<Harness />); await expand(user);
  await user.click(screen.getByRole("button", { name: tr("sessionImport.nextPage") }));
  expect(screen.getByRole("alert")).toHaveTextContent(tr("sessionImport.prepare.resultsError"));
  expect(rows().getByText("Synthetic 0")).toBeVisible();
  expect(screen.getByRole("button", { name: tr("sessionImport.nextPage") })).toBeDisabled();
  await user.click(screen.getByRole("button", { name: tr("sessionImport.prepare.retryResults") }));
  expect(retry).toHaveBeenCalledOnce(); expect(load).toHaveBeenCalledOnce();
  expect(rows().getByText("Synthetic 50")).toBeVisible(); expect(screen.queryByRole("alert")).toBeNull();
});

test("non-Cursor pages have an empty state and can advance to eligible rows", async () => {
  const ui = props({ prepareItems: [...members(50, "codex"), { ...members(1)[0], candidateId: "cursor-only", title: "Eligible Cursor" }] });
  const user = userEvent.setup(); render(<SessionImportPreparedAssociations tr={tr} ui={ui} />); await expand(user);
  expect(rows().queryAllByRole("listitem")).toHaveLength(0);
  expect(screen.getByText(tr("sessionImport.historyLink.empty"))).toBeVisible();
  await user.click(screen.getByRole("button", { name: tr("sessionImport.nextPage") }));
  expect(rows().getAllByRole("listitem")).toHaveLength(1); expect(rows().getByText("Eligible Cursor")).toBeVisible();
});

test("busy state blocks paging and linking; an empty remote page does not start a fetch loop", async () => {
  const load = vi.fn(async () => true); const ui = props({ prepareItems: [], prepareNextCursor: "next", loadMorePrepare: load, busy: true });
  const user = userEvent.setup(); const view = render(<SessionImportPreparedAssociations tr={tr} ui={ui} />); await expand(user);
  await user.click(screen.getByRole("button", { name: tr("sessionImport.nextPage") })); expect(load).not.toHaveBeenCalled();
  view.rerender(<SessionImportPreparedAssociations tr={tr} ui={{ ...ui, busy: false }} />);
  await user.click(screen.getByRole("button", { name: tr("sessionImport.nextPage") }));
  expect(load).toHaveBeenCalledOnce(); expect(screen.getByText(tr("sessionImport.page", { n: 1 }))).toBeVisible();
  expect(screen.getByText(tr("sessionImport.historyLink.empty"))).toBeVisible();
});

test("the real mapping resets the page for a different batch version; an old load cannot advance it", async () => {
  let resolve!: (value: boolean) => void;
  const ui: ComponentProps<typeof SessionImportMappingStep>["ui"] = {
    ...props({ prepareItems: members(50), prepareNextCursor: "next",
      loadMorePrepare: () => new Promise<boolean>((done) => { resolve = done; }) }),
    associations: { ...props().associations, dialog: null, targets: null, busy: false, error: null,
      close: vi.fn(), choose: vi.fn(async () => {}), markReady: vi.fn(), confirm: () => false,
      snapshot: () => [], isReviewing: () => false, nextTargets: async () => {}, firstTargets: async () => {} },
    prepareBatch: { version: 1, batchId: "batch", scanId: "scan", epoch: 1, selectionId: "selection",
      selectionStateVersion: 1, state: "completed", stateVersion: 4, total: 100, prepared: 100, failed: 0,
      skipped: 0, active: 0, cancelRequested: false, createdAt: "2026-10-15", updatedAt: "2026-10-15" },
    mappings: [], registrations: [], pendingPlan: null, projectId: null, projects: [], setMappingProject: vi.fn(), commitPhase: null,
    explicitReimport: false, setExplicitReimport: vi.fn(),
  };
  const step = (value = ui) => <SessionImportMappingStep tr={tr} ui={value} mappingSessions={[]}
    mappingSessionCount={0} commitImport={vi.fn()} onBack={vi.fn()} />;
  const user = userEvent.setup(); const view = render(step()); await expand(user);
  await user.click(screen.getByRole("button", { name: tr("sessionImport.nextPage") }));
  view.rerender(step({ ...ui, prepareBatch: { ...ui.prepareBatch!, stateVersion: 5 }, prepareItems: members(100) }));
  await expand(user); await act(async () => resolve(true));
  expect(screen.getByText(tr("sessionImport.page", { n: 1 }))).toBeVisible();
  expect(rows().getByText("Synthetic 0")).toBeVisible();
  await user.click(screen.getByRole("button", { name: tr("sessionImport.nextPage") }));
  expect(screen.getByText(tr("sessionImport.page", { n: 2 }))).toBeVisible();
  view.rerender(step({ ...ui, prepareBatch: { ...ui.prepareBatch!, batchId: "new-batch" }, prepareItems: members(100) }));
  await expand(user); expect(screen.getByText(tr("sessionImport.page", { n: 1 }))).toBeVisible();
});
