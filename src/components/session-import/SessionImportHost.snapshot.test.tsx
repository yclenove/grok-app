/** @vitest-environment jsdom */
import type { ReactNode } from "react";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { CatalogEntry } from "@/lib/api/sessionMigration";
import { SessionImportHost } from "./SessionImportHost";

const mocks = vi.hoisted(() => ({
  events: vi.fn(), search: vi.fn(), context: vi.fn(), exportMd: vi.fn(), prepare: vi.fn(), autoPrepare: vi.fn(),
  entries: [] as CatalogEntry[], revision: null as string | null, pendingContinue: false,
}));
vi.mock("@/lib/api/sessionMigration", async (importOriginal) => ({
  ...await importOriginal<typeof import("@/lib/api/sessionMigration")>(),
  sessionImportEvents: mocks.events, sessionImportSearch: mocks.search,
  sessionImportEventContext: mocks.context, sessionImportExportMarkdown: mocks.exportMd,
}));
vi.mock("@/components/GlassModal", () => ({ GlassModal: ({ open, children, footer }: { open: boolean; children: ReactNode; footer?: ReactNode }) => open ? <div role="dialog">{children}{footer}</div> : null }));
vi.mock("@/hooks/useSessionImport", () => ({ useSessionImportUi: () => ({
  open: false, step: "source", busy: false, readerId: "imp:snapshot-host", readerRevision: mocks.revision, readerLeaf: null,
  candidates: [], mappings: [], selected: new Set(), selectedCount: 0, prepareItems: [], prepareBatch: null,
  prepareContinue: mocks.prepare, readerContinueRequested: mocks.pendingContinue, prepareReaderContinue: mocks.autoPrepare,
}) }));
vi.mock("@/hooks/useImportedCatalog", () => ({ useImportedCatalog: () => ({ entries: mocks.entries }) }));
vi.mock("@/hooks/useSessionImportContentStates", () => ({ useSessionImportContentStates: () => ({ ensureSelected: () => {} }) }));
vi.mock("./ImportedWorkspaceAssociation", () => ({ ImportedWorkspaceAssociation: () => null }));
vi.mock("./SessionImportContinuationModal", () => ({ SessionImportContinuationModal: () => null }));
const page = (revision = "revision-a", leaf = "leaf-a") => ({ events: [{ id: "first", role: "user", text: "Accepted archive" }], revision, leaf, previousCursor: null, nextCursor: null, total: 1, start: 0, end: 1 });
const entry: CatalogEntry = { importId: "imp:snapshot-host", installationId: "test", provider: "codex", nativeSessionId: "native", title: "Updated title", hidden: false, createdAt: "2026-09-27T00:00:00Z", currentRevision: "revision-b" };
beforeEach(() => {
  vi.resetAllMocks(); mocks.entries = []; mocks.revision = null; mocks.pendingContinue = false;
  mocks.events.mockResolvedValue(page()); mocks.search.mockResolvedValue({ hits: [], previousCursor: null, nextCursor: null });
});
afterEach(cleanup);

test("late metadata preserves the query and composition node while all derived actions keep the accepted archive", async () => {
  const view = render(<SessionImportHost locale="en" />);
  await screen.findByText("Accepted archive");
  const input = screen.getByRole("searchbox") as HTMLInputElement;
  input.focus(); fireEvent.compositionStart(input); fireEvent.change(input, { target: { value: "ni 中文" } });
  mocks.entries = [entry]; view.rerender(<SessionImportHost locale="en" />);
  expect(screen.getByRole("searchbox")).toBe(input); expect(document.activeElement).toBe(input); expect(input.value).toBe("ni 中文");
  fireEvent.submit(input.closest("form")!); expect(mocks.search).not.toHaveBeenCalled();
  fireEvent.compositionEnd(input); fireEvent.submit(input.closest("form")!);
  await waitFor(() => expect(mocks.search).toHaveBeenCalledWith("imp:snapshot-host", "ni 中文", "revision-a", null, "leaf-a"));
  expect(mocks.events).toHaveBeenCalledTimes(1);
  await waitFor(() => expect((screen.getByRole("button", { name: "Continue in Grok" }) as HTMLButtonElement).disabled).toBe(false));
  fireEvent.click(screen.getByRole("button", { name: "Continue in Grok" }));
  expect(mocks.prepare).toHaveBeenCalledWith("imp:snapshot-host", "leaf-a", "revision-a");
});

test("metadata arriving before the initial reply does not restart loading or erase an unfinished query", async () => {
  let resolve!: (value: ReturnType<typeof page>) => void;
  mocks.events.mockReturnValueOnce(new Promise((done) => { resolve = done; }));
  const view = render(<SessionImportHost locale="en" />);
  const input = screen.getByRole("searchbox") as HTMLInputElement;
  fireEvent.change(input, { target: { value: "unfinished" } });
  mocks.entries = [entry]; view.rerender(<SessionImportHost locale="en" />);
  expect(mocks.events).toHaveBeenCalledTimes(1);
  await act(async () => resolve(page()));
  expect(screen.getByRole("searchbox")).toBe(input); expect(input.value).toBe("unfinished");
});

test("automatic continuation is prepared only after scope resolution, and metadata refresh does not repeat it", async () => {
  let resolve!: (value: ReturnType<typeof page>) => void;
  mocks.pendingContinue = true;
  mocks.events.mockReturnValueOnce(new Promise((done) => { resolve = done; }));
  const view = render(<SessionImportHost locale="en" />);
  expect(mocks.autoPrepare).not.toHaveBeenCalled();
  await act(async () => resolve(page()));
  expect(mocks.autoPrepare).toHaveBeenCalledWith("imp:snapshot-host", "leaf-a", "revision-a");
  mocks.entries = [entry]; view.rerender(<SessionImportHost locale="en" />);
  expect(mocks.autoPrepare).toHaveBeenCalledTimes(1);
});

test("explicit revision navigation, unlike metadata refresh, resets the query and resolves a new archive", async () => {
  const view = render(<SessionImportHost locale="en" />);
  await screen.findByText("Accepted archive");
  const input = screen.getByRole("searchbox"); fireEvent.change(input, { target: { value: "old query" } });
  mocks.revision = "revision-b"; mocks.events.mockResolvedValueOnce(page("revision-b", "leaf-b"));
  view.rerender(<SessionImportHost locale="en" />);
  await waitFor(() => expect(mocks.events).toHaveBeenLastCalledWith("imp:snapshot-host", "revision-b", null, 100, null));
  expect(screen.getByRole("searchbox")).not.toBe(input);
  expect((screen.getByRole("searchbox") as HTMLInputElement).value).toBe("");
  await waitFor(() => expect((screen.getByRole("button", { name: "Continue in Grok" }) as HTMLButtonElement).disabled).toBe(false));
  fireEvent.click(screen.getByRole("button", { name: "Continue in Grok" }));
  expect(mocks.prepare).toHaveBeenCalledWith("imp:snapshot-host", "leaf-b", "revision-b");
});
