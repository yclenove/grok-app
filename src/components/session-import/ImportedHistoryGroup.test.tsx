/** @vitest-environment jsdom */
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { createT } from "@/i18n";
import { OPEN_IMPORTED_EVENT } from "@/lib/sessionImport/open";
import { ImportedHistoryGroup } from "./ImportedHistoryGroup";
const mocks = vi.hoisted(() => ({ entries: vi.fn(), groups: vi.fn(), listen: vi.fn(), export: vi.fn() }));
vi.mock("@/lib/api/sessionMigration", () => ({ sessionImportEntries: mocks.entries, sessionImportExportMarkdown: mocks.export }));
vi.mock("@/lib/api/sessionWorkspaceAssociation", () => ({ importedWorkspaceGroups: mocks.groups }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
const one = { sourceWorkspaceId: "ws:one", displayName: "Same name", sourceId: "source-a", provider: "codex", sourceLocator: "folder:/machine/a", missing: false };
const two = { ...one, sourceWorkspaceId: "ws:two", sourceId: "source-b", sourceLocator: "folder:/machine/b" };
const row = { importId: "imp:00000000-0000-4000-8000-000000000001", title: "Imported example", alias: null, hidden: false, provider: "codex", nativeSessionId: "n1", createdAt: "2026-09-20T00:00:00Z" };
beforeEach(() => {
  sessionStorage.clear();
  mocks.listen.mockResolvedValue(() => {});
  mocks.groups.mockImplementation(async (_cursor, selectedId) => ({ items: [one, two], nextCursor: null, selected: selectedId === two.sourceWorkspaceId ? two : null }));
  mocks.entries.mockResolvedValue({ items: [row], nextCursor: null, total: 1, providers: ["codex"] });
});
afterEach(() => { cleanup(); vi.resetAllMocks(); });
const renderGroup = () => render(<ImportedHistoryGroup tr={createT("en")} />);

test("keyboard workspace choice uses server filtering and survives reopening the list", async () => {
  const user = userEvent.setup();
  const view = renderGroup();
  await screen.findByRole("button", { name: /Imported example/ });
  const select = screen.getByRole("button", { name: "Filter by workspace" });
  select.focus();
  await user.keyboard("{Enter}");
  const option = await screen.findByRole("option", { name: /machine\/b/ });
  expect(screen.getByRole("option", { name: /machine\/a/ })).toBeTruthy();
  within(option).getByRole("button").focus();
  await user.keyboard("{Enter}");
  await waitFor(() => expect(mocks.entries.mock.calls.at(-1)?.[2].workspace).toEqual({ kind: "workspace", sourceWorkspaceId: "ws:two" }));
  view.unmount();
  renderGroup();
  await waitFor(() => expect(screen.getByRole("button", { name: "Filter by workspace" }).textContent).toContain("machine/b"));
  expect(document.querySelector("select")).toBeNull();
});

test("opening filtered history uses the read-only reader route; continue remains an explicit separate action", async () => {
  const opened = vi.fn();
  window.addEventListener(OPEN_IMPORTED_EVENT, opened);
  try {
    renderGroup();
    fireEvent.click(await screen.findByRole("button", { name: /Imported example/ }));
    expect(opened.mock.calls[0][0].detail).toEqual({ importId: row.importId });
    fireEvent.click(screen.getByRole("button", { name: "Continue in Grok" }));
    expect(opened.mock.calls[1][0].detail).toEqual({ importId: row.importId, continue: true });
    expect(window.location.hash).toContain(encodeURIComponent(row.importId));
    expect(mocks.export).not.toHaveBeenCalled();
  } finally { window.removeEventListener(OPEN_IMPORTED_EVENT, opened); }
});

test("empty workspaces show zero histories without creating or opening a chat", async () => {
  mocks.entries.mockResolvedValue({ items: [], nextCursor: null, total: 0, providers: [] });
  const opened = vi.fn();
  window.addEventListener(OPEN_IMPORTED_EVENT, opened);
  try {
    renderGroup();
    await waitFor(() => expect(screen.queryByText("Loading histories…")).toBeNull());
    expect(screen.getByText(createT("en")("sessionImport.noImported"))).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Continue in Grok" })).toBeNull();
    expect(opened).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Filter by workspace" }));
    expect(await screen.findByRole("option", { name: /machine\/a/ })).toBeTruthy();
  } finally { window.removeEventListener(OPEN_IMPORTED_EVENT, opened); }
});

test("load errors expose retry without claiming the catalog is empty", async () => {
  mocks.entries.mockRejectedValueOnce("SOURCE_BUSY");
  renderGroup();
  await screen.findByRole("alert");
  expect(screen.queryByText(createT("en")("sessionImport.noImported"))).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
  await screen.findByRole("button", { name: /Imported example/ });
  expect(screen.queryByRole("alert")).toBeNull();
});

test("membership notifications keep the selected filter and remove a cleared membership from results", async () => {
  mocks.entries.mockImplementation(async (_cursor, _limit, filter) => ({ items: filter.workspace ? [] : [row], nextCursor: null, total: filter.workspace ? 0 : 1 }));
  renderGroup();
  await screen.findByRole("button", { name: /Imported example/ });
  fireEvent.click(screen.getByRole("button", { name: "Filter by workspace" }));
  fireEvent.click(within(await screen.findByRole("option", { name: /machine\/a/ })).getByRole("button"));
  await waitFor(() => expect(screen.queryByRole("button", { name: /Imported example/ })).toBeNull());
  const listeners = mocks.listen.mock.calls.filter(([name]) => name === "session-import://workspace-changed");
  await act(async () => { for (const [, callback] of listeners) callback(); });
  await waitFor(() => expect(screen.queryByText("Loading histories…")).toBeNull());
  expect(mocks.entries.mock.calls.at(-1)?.[2].workspace).toEqual({ kind: "workspace", sourceWorkspaceId: "ws:one" });
});

test("a remembered provider can be cleared after its last imported history disappears", async () => {
  sessionStorage.setItem("grok:imported-catalog-view:v1:sidebar", JSON.stringify({ query: "", provider: "claude-code", sort: "newest", workspace: null }));
  mocks.entries.mockResolvedValue({ items: [], nextCursor: null, total: 0, providers: [] });
  renderGroup();
  await screen.findByText(createT("en")("sessionImport.filterEmpty"));
  expect(screen.getByRole("button", { name: createT("en")("sessionImport.source.claude") })).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: createT("en")("sessionImport.providerAll") }));
  await waitFor(() => expect(mocks.entries.mock.calls.at(-1)?.[2].provider).toBeNull());
});

test("a missing selected workspace stays named off-page and remains clearable after a failed refresh", async () => {
  sessionStorage.setItem("grok:imported-catalog-view:v1:sidebar", JSON.stringify({ query: "", provider: null, sort: "newest", workspace: { kind: "workspace", sourceWorkspaceId: "ws:two" } }));
  mocks.groups.mockResolvedValueOnce({ items: [one], nextCursor: "next", selected: { ...two, missing: true } })
    .mockRejectedValueOnce("SOURCE_BUSY");
  renderGroup();
  await waitFor(() => expect(screen.getByRole("button", { name: "Filter by workspace" }).textContent).toContain("machine/b"));
  expect(screen.getByRole("button", { name: "Filter by workspace" }).textContent).toContain(createT("en")("sessionImport.workspaceFilter.unavailable"));
  fireEvent.click(screen.getByRole("button", { name: createT("en")("sessionImport.workspaceFilter.refresh") }));
  await screen.findByRole("alert");
  expect(screen.getByRole("button", { name: "Filter by workspace" }).textContent).toContain("machine/b");
  fireEvent.click(screen.getByRole("button", { name: "Filter by workspace" }));
  fireEvent.click(within(await screen.findByRole("option", { name: createT("en")("sessionImport.workspaceFilter.all") })).getByRole("button"));
  await waitFor(() => expect(mocks.entries.mock.calls.at(-1)?.[2].workspace).toBeNull());
});

test("catalog rows separate their title and metadata and use explicit button variants", async () => {
  renderGroup();
  const title = await screen.findByRole("button", { name: /Imported example/ });
  expect(title.querySelector(".tree-imported__name")?.textContent).toBe(row.title);
  expect(title.querySelector(".tree-imported__meta")?.textContent).toContain("Codex");
  expect(title.querySelector("time")?.getAttribute("datetime")).toBe(row.createdAt);
  expect(title.classList.contains("tree-session")).toBe(false);
  for (const button of document.querySelectorAll(".tree-imported .btn")) {
    expect([...button.classList].some((name) => /^btn--(ghost|solid|primary|danger)$/.test(name))).toBe(true);
  }
  expect(screen.getByRole("button", { name: "Export Markdown" }).getAttribute("aria-label")).toBe("Export Markdown");
  expect(screen.getByRole("button", { name: "Continue in Grok" }).getAttribute("aria-label")).toBe("Continue in Grok");
});

test("provider selection exposes its state and sorting stays available for an empty filter", async () => {
  sessionStorage.setItem("grok:imported-catalog-view:v1:sidebar", JSON.stringify({ query: "", provider: "claude-code", sort: "newest", workspace: null }));
  mocks.entries.mockResolvedValue({ items: [], nextCursor: null, total: 0, providers: [] });
  renderGroup();
  await screen.findByText(createT("en")("sessionImport.filterEmpty"));
  expect(screen.getByRole("button", { name: createT("en")("sessionImport.source.claude") }).getAttribute("aria-pressed")).toBe("true");
  fireEvent.click(screen.getByRole("button", { name: "Newest first" }));
  await waitFor(() => expect(mocks.entries.mock.calls.at(-1)?.[2].sort).toBe("oldest"));
  expect(screen.getByRole("button", { name: "Oldest first" })).toBeTruthy();
});

test("filter misses do not claim the catalog is empty and clearing search restores rows", async () => {
  const tr = createT("en");
  mocks.entries.mockImplementation(async (_cursor, _limit, filter) => ({ items: filter.query ? [] : [row], total: filter.query ? 0 : 1, providers: ["codex"], nextCursor: null }));
  renderGroup();
  await screen.findByRole("button", { name: /Imported example/ });
  fireEvent.change(screen.getByRole("searchbox"), { target: { value: "中文 😀 unmatched" } });
  await screen.findByText(tr("sessionImport.filterEmpty"));
  expect(screen.queryByText(tr("sessionImport.noImported"))).toBeNull();
  fireEvent.change(screen.getByRole("searchbox"), { target: { value: "" } });
  await screen.findByRole("button", { name: /Imported example/ });
});

test("the settings variant keeps row actions keyboard reachable while export is pending", async () => {
  const user = userEvent.setup();
  let finishExport!: (value: string) => void;
  mocks.export.mockReturnValueOnce(new Promise((resolve) => { finishExport = resolve; }));
  const writeText = vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue();
  render(<ImportedHistoryGroup tr={createT("en")} variant="settings" />);
  const title = await screen.findByRole("button", { name: /Imported example/ });
  title.focus();
  await user.tab();
  const exportButton = screen.getByRole("button", { name: "Export Markdown" });
  expect(document.activeElement).toBe(exportButton);
  await user.keyboard("{Enter}");
  title.focus();
  await user.tab();
  expect(document.activeElement).toBe(screen.getByRole("button", { name: "Continue in Grok" }));
  await act(async () => finishExport("# Synthetic imported example"));
  expect(writeText).toHaveBeenCalledWith("# Synthetic imported example");
  expect(await screen.findByRole("status")).toHaveProperty("textContent", createT("en")("sessionImport.copiedExport"));
});

test("a pending export cannot enqueue duplicate clipboard operations and failure is recoverable", async () => {
  let rejectExport!: (reason: unknown) => void;
  mocks.export.mockReturnValueOnce(new Promise((_resolve, reject) => { rejectExport = reject; }));
  renderGroup();
  await screen.findByRole("button", { name: /Imported example/ });
  const button = screen.getByRole("button", { name: "Export Markdown" }) as HTMLButtonElement;
  fireEvent.click(button);
  fireEvent.click(button);
  expect(mocks.export).toHaveBeenCalledTimes(1);
  expect(button.disabled).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: /Imported example/ }));
  await act(async () => rejectExport(new Error("export failed")));
  await screen.findByRole("alert");
  expect(button.disabled).toBe(false);
});
