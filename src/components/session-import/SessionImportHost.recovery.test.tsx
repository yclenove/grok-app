/** @vitest-environment jsdom */
import type { ReactNode } from "react";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { SessionImportHost } from "./SessionImportHost";

const mocks = vi.hoisted(() => ({ events: vi.fn(), search: vi.fn(), context: vi.fn(), exportMd: vi.fn(), close: vi.fn(), ensureSelected: vi.fn() }));
vi.mock("@/lib/api/sessionMigration", async (importOriginal) => ({
  ...await importOriginal<typeof import("@/lib/api/sessionMigration")>(),
  sessionImportEvents: mocks.events, sessionImportSearch: mocks.search,
  sessionImportEventContext: mocks.context, sessionImportExportMarkdown: mocks.exportMd,
}));
vi.mock("@/components/GlassModal", () => ({ GlassModal: ({ open, children, footer }: { open: boolean; children: ReactNode; footer?: ReactNode }) => open ? <div role="dialog" aria-modal="true">{children}{footer}</div> : null }));
vi.mock("@/hooks/useSessionImport", () => ({ useSessionImportUi: () => ({
  open: false, step: "source", busy: false, readerId: "imp:recovery", readerRevision: "fixed-revision", readerLeaf: "fixed-leaf",
  candidates: [], mappings: [], selected: new Set(), selectedCount: 0, prepareItems: [], prepareBatch: null, setReaderId: mocks.close,
}) }));
vi.mock("@/hooks/useImportedCatalog", () => ({ useImportedCatalog: () => ({ entries: [] }) }));
vi.mock("@/hooks/useSessionImportContentStates", () => ({ useSessionImportContentStates: () => ({ ensureSelected: mocks.ensureSelected }) }));
vi.mock("./ImportedWorkspaceAssociation", () => ({ ImportedWorkspaceAssociation: () => null }));
vi.mock("./SessionImportContinuationModal", () => ({ SessionImportContinuationModal: () => null }));

const page = { revision: "fixed-revision", leaf: "fixed-leaf", events: [{ id: "recovered", role: "user", text: "Recovery transcript restored" }], previousCursor: null, nextCursor: null };
beforeEach(() => {
  vi.resetAllMocks();
  mocks.events.mockRejectedValueOnce(new Error("TEMPORARY_ARCHIVE_READ_FAILURE")).mockResolvedValue(page);
});
afterEach(cleanup);

test("an initial read error exposes a refresh that restores history without closing or changing archive identity", async () => {
  render(<SessionImportHost locale="en" />);
  await waitFor(() => expect(document.querySelector("[data-import-reader-error]")).not.toBeNull());
  const refresh = screen.getByRole("button", { name: "Refresh" });
  refresh.focus(); fireEvent.click(refresh);
  await screen.findByText("Recovery transcript restored");
  expect(document.querySelector("[data-import-reader-error]")).toBeNull();
  expect(mocks.events).toHaveBeenLastCalledWith("imp:recovery", "fixed-revision", null, 100, "fixed-leaf");
  expect(mocks.close).not.toHaveBeenCalled();
  // Transcript commit can precede the passive recovery-focus effect.
  await waitFor(() => expect(screen.getByRole("searchbox")).toBe(document.activeElement));
});

test("refresh remains single-flight and visible until success while preserving an edited query", async () => {
  let resolve!: (value: typeof page) => void;
  const pending = new Promise<typeof page>((done) => { resolve = done; });
  render(<SessionImportHost locale="en" />);
  await waitFor(() => expect(document.querySelector("[data-import-reader-error]")).not.toBeNull());
  const query = screen.getByRole("searchbox");
  fireEvent.change(query, { target: { value: "draft 中文 🧪" } });
  mocks.events.mockReturnValueOnce(pending);
  const refresh = screen.getByRole("button", { name: "Refresh" });
  refresh.focus(); fireEvent.click(refresh); fireEvent.click(refresh);
  expect((refresh as HTMLButtonElement).disabled).toBe(true);
  expect(mocks.events).toHaveBeenCalledTimes(2);
  expect(document.querySelector("[data-import-reader-error]")).not.toBeNull();
  await act(async () => resolve(page));
  await screen.findByText("Recovery transcript restored");
  expect((query as HTMLInputElement).value).toBe("draft 中文 🧪");
  expect(mocks.search).not.toHaveBeenCalled();
});
