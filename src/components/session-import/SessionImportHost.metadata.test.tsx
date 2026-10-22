/** @vitest-environment jsdom */
import type { ReactNode } from "react";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, expect, test, vi } from "vitest";
import { createT, loadAllLocaleCatalogs } from "@/i18n";
import type { CatalogEntry } from "@/lib/api/sessionMigration";
import { SessionImportHost } from "./SessionImportHost";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const stylesDir = join(dirname(fileURLToPath(import.meta.url)), "../../styles");
const readerStyles = readFileSync(join(stylesDir, "session-import-reader.css"), "utf8");
const buttonStyles = readFileSync(join(stylesDir, "chat.part4.css"), "utf8");

const mocks = vi.hoisted(() => ({
  entry: vi.fn(), events: vi.fn(), search: vi.fn(), prepare: vi.fn(),
  entries: [] as CatalogEntry[], readerId: "imp:outside-first-page" as string | null,
}));
vi.mock("@/lib/api/sessionMigration", async (original) => ({
  ...await original<typeof import("@/lib/api/sessionMigration")>(),
  sessionImportEntry: mocks.entry, sessionImportEvents: mocks.events, sessionImportSearch: mocks.search,
}));
vi.mock("@/components/GlassModal", () => ({ GlassModal: ({ open, title, children, footer, className }: {
  open: boolean; title?: ReactNode; children: ReactNode; footer?: ReactNode; className?: string;
}) => open ? <div role="dialog" className={className}><h2>{title}</h2>{children}{footer}</div> : null }));
vi.mock("@/hooks/useSessionImport", () => ({ useSessionImportUi: () => ({
  open: false, step: "source", busy: false, readerId: mocks.readerId,
  readerRevision: "viewed-revision", readerLeaf: "viewed-leaf",
  candidates: [], mappings: [], selected: new Set(), selectedCount: 0,
  prepareItems: [], prepareBatch: null, prepareContinue: mocks.prepare,
}) }));
vi.mock("@/hooks/useImportedCatalog", () => ({ useImportedCatalog: () => ({ entries: mocks.entries }) }));
vi.mock("@/hooks/useSessionImportContentStates", () => ({ useSessionImportContentStates: () => ({ ensureSelected: () => {} }) }));
vi.mock("./ImportedWorkspaceAssociation", () => ({ ImportedWorkspaceAssociation: () => null }));
vi.mock("./SessionImportContinuationModal", () => ({ SessionImportContinuationModal: () => null }));
const metadata = (importId = "imp:outside-first-page", title = "Archive beyond row 50"): CatalogEntry => ({
  importId, installationId: "synthetic", provider: "generic-file", nativeSessionId: "outside-first-page.json",
  title, hidden: false, currentRevision: "newer-catalog-revision", createdAt: "2026-09-30T00:00:00Z",
});
const page = { events: [{ id: "first", role: "user", text: "Pinned archive body" }],
  revision: "viewed-revision", leaf: "viewed-leaf", previousCursor: null, nextCursor: null, total: 1, start: 0, end: 1 };
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>((done) => { resolve = done; }); return { promise, resolve }; }
beforeAll(loadAllLocaleCatalogs);
beforeEach(() => {
  vi.resetAllMocks(); mocks.entries = []; mocks.readerId = "imp:outside-first-page";
  mocks.entry.mockResolvedValue(metadata()); mocks.events.mockResolvedValue(page);
  mocks.search.mockResolvedValue({ hits: [], previousCursor: null, nextCursor: null });
});
afterEach(cleanup);

test("an off-page deep-link resolves exact metadata without loading catalog pages", async () => {
  render(<SessionImportHost locale="en" />);
  await screen.findByRole("heading", { name: "Archive beyond row 50" });
  expect(mocks.entry).toHaveBeenCalledExactlyOnceWith("imp:outside-first-page");
  expect(screen.getByText(/generic-file.*outside-first-page.json.*2026-09-30/)).toBeTruthy();
});

test("late exact metadata preserves composing search and pinned revision/leaf actions", async () => {
  const pending = deferred<CatalogEntry>(); mocks.entry.mockReturnValueOnce(pending.promise);
  const pendingSearch = deferred<{ hits: never[]; previousCursor: null; nextCursor: null }>();
  mocks.search.mockReturnValueOnce(pendingSearch.promise);
  render(<SessionImportHost locale="en" />); await screen.findByText("Pinned archive body");
  const input = screen.getByRole("searchbox") as HTMLInputElement;
  input.focus(); fireEvent.compositionStart(input); fireEvent.change(input, { target: { value: "ni 中文 😀" } });
  await act(async () => pending.resolve(metadata()));
  expect(screen.getByRole("heading", { name: "Archive beyond row 50" })).toBeTruthy();
  expect(screen.getByRole("searchbox")).toBe(input); expect(document.activeElement).toBe(input);
  fireEvent.submit(input.closest("form")!); expect(mocks.search).not.toHaveBeenCalled();
  fireEvent.compositionEnd(input); fireEvent.submit(input.closest("form")!);
  await waitFor(() => expect(mocks.search).toHaveBeenCalledWith("imp:outside-first-page", "ni 中文 😀", "viewed-revision", null, "viewed-leaf"));
  const continueButton = screen.getByRole("button", { name: "Continue in Grok" });
  expect(continueButton.hasAttribute("disabled")).toBe(true);
  fireEvent.click(continueButton);
  expect(mocks.prepare).not.toHaveBeenCalled();
  await act(async () => pendingSearch.resolve({ hits: [], previousCursor: null, nextCursor: null }));
  await waitFor(() => expect(continueButton.hasAttribute("disabled")).toBe(false));
  fireEvent.click(continueButton);
  expect(mocks.prepare).toHaveBeenCalledWith("imp:outside-first-page", "viewed-leaf", "viewed-revision");
  expect(mocks.events).toHaveBeenCalledTimes(1);
});

test("metadata failure can retry without restarting the pinned reader", async () => {
  mocks.entry.mockRejectedValueOnce(new Error("synthetic metadata unavailable"));
  render(<SessionImportHost locale="en" />); await screen.findByText("Pinned archive body");
  await screen.findByText("Could not load imported history.");
  fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
  await screen.findByRole("heading", { name: "Archive beyond row 50" });
  expect(mocks.entry).toHaveBeenCalledTimes(2); expect(mocks.events).toHaveBeenCalledTimes(1);
  expect(screen.queryByText("Could not load imported history.")).toBeNull();
});

test("a late response cannot replace a newly opened reader or reopen a closed dialog", async () => {
  const a = deferred<CatalogEntry>(); const b = deferred<CatalogEntry>();
  mocks.entry.mockReturnValueOnce(a.promise).mockReturnValueOnce(b.promise);
  const view = render(<SessionImportHost locale="en" />); await screen.findByText("Pinned archive body");
  mocks.readerId = "imp:second"; view.rerender(<SessionImportHost locale="en" />);
  await act(async () => a.resolve(metadata()));
  expect(screen.queryByRole("heading", { name: "Archive beyond row 50" })).toBeNull();
  mocks.readerId = null; view.rerender(<SessionImportHost locale="en" />);
  await act(async () => b.resolve(metadata("imp:second", "Second archive")));
  expect(screen.queryByRole("dialog")).toBeNull(); expect(mocks.entry).toHaveBeenCalledTimes(2);
});

test("known catalog metadata remains authoritative without a redundant exact lookup", async () => {
  mocks.entries = [{ ...metadata(), alias: "Renamed locally" }];
  render(<SessionImportHost locale="en" />);
  await screen.findByRole("heading", { name: "Renamed locally" });
  expect(mocks.entry).not.toHaveBeenCalled();
});


test("one refresh recovers simultaneous metadata and transcript errors", async () => {
  mocks.entry.mockRejectedValueOnce(new Error("synthetic metadata unavailable"));
  mocks.events.mockRejectedValueOnce(new Error("synthetic transcript unavailable"));
  render(<SessionImportHost locale="en" />);
  const refresh = await screen.findByRole("button", { name: "Refresh" });
  expect(screen.getAllByRole("button", { name: "Refresh" })).toHaveLength(1);
  fireEvent.click(refresh);
  await screen.findByText("Pinned archive body");
  await screen.findByRole("heading", { name: "Archive beyond row 50" });
  expect(mocks.entry).toHaveBeenCalledTimes(2);
  expect(mocks.events).toHaveBeenCalledTimes(2);
});

test.each(["en", "zh"] as const)("%s long search snippets retain their localized kind and complete source text", async (locale) => {
  const label = createT(locale)("sessionImport.scopeMessages");
  const snippet = "Synthetic long search snippet 中文 😀 " + "x".repeat(240);
  mocks.search.mockResolvedValue({ hits: [{ eventId: "first", ordinal: 0, kind: "message", snippet }], nextCursor: null });
  const style = document.createElement("style");
  const baseButtonRule = buttonStyles.match(/\.btn\s*\{[^}]+\}/)?.[0];
  expect(baseButtonRule).toBeTruthy();
  style.textContent = baseButtonRule + "\n" + readerStyles;
  document.head.append(style);
  try {
    render(<SessionImportHost locale={locale} />);
    await screen.findByText("Pinned archive body");
    const input = screen.getByRole("searchbox");
    fireEvent.change(input, { target: { value: "Synthetic" } });
    fireEvent.submit(input.closest("form")!);
    const hit = await screen.findByRole("button", { name: label + " " + snippet });
    const kind = hit.querySelector(".session-import__hit-kind")!;
    const text = hit.querySelector(".session-import__hit-snippet")!;
    expect(kind.textContent).toBe(label);
    expect(text.textContent).toBe(snippet);
    const css = window.getComputedStyle(hit);
    expect(css.display).toBe("grid");
    expect(css.height).toBe("auto");
    expect(css.minHeight).toBe("32px");
    expect(css.lineHeight).toBe("1.4");
    expect(window.getComputedStyle(kind).whiteSpace).toBe("nowrap");
    expect(window.getComputedStyle(text).minWidth).toBe("0px");
  } finally { style.remove(); }
});
