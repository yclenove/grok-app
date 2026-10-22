/** @vitest-environment jsdom */
import { act, cleanup, fireEvent, render, renderHook, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { createT } from "@/i18n";
import { useImportedCatalog } from "@/hooks/useImportedCatalog";
import { useImportedWorkspaceGroups } from "@/hooks/useImportedWorkspaceGroups";
import type { CatalogEntry, CatalogQuery } from "@/lib/api/sessionMigration";
import { IMPORTED_LIST_WINDOW } from "@/lib/sessionImport/catalogView";
import { ImportedHistoryGroup } from "./ImportedHistoryGroup";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@/lib/api/host", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));

const tr = createT("en");
const groups = Array.from({ length: 10_000 }, (_, i) => ({
  sourceWorkspaceId: `ws:${String(i).padStart(5, "0")}`, displayName: `Workspace ${i}`,
  sourceId: "scale-source", provider: "codex", sourceLocator: `/scale/${i}`, missing: false,
}));
const entries: CatalogEntry[] = groups.map((_, i) => ({
  importId: `imp:00000000-0000-4000-8000-${String(i).padStart(12, "0")}`,
  installationId: "scale-installation", nativeSessionId: `native-${i}`,
  title: `History ${String(i).padStart(5, "0")}`, alias: null, hidden: false,
  provider: i % 2 ? "cursor" : "codex", createdAt: new Date(Date.UTC(2026, 0, 1, 0, 0, -i)).toISOString(),
}));
const membership = new Map(entries.map((entry, i) => [entry.importId, i % 5 ? groups[i].sourceWorkspaceId : null]));
type Args = CatalogQuery & { cursor: string | null; limit: number; selectedId?: string | null };

// Only the host fixture sees all records. Components use the real paged IPC wrappers.
async function hostPage(command: string, args: Args) {
  const start = Number(args.cursor ?? 0);
  if (command === "session_import_workspace_groups") {
    return {
      items: groups.slice(start, start + args.limit),
      nextCursor: start + args.limit < groups.length ? String(start + args.limit) : null,
      selected: groups.find((group) => group.sourceWorkspaceId === args.selectedId) ?? null,
    };
  }
  if (command !== "session_import_entries") throw new Error(`Unexpected IPC: ${command}`);
  const workspace = args.workspace;
  const filtered = entries.filter((entry) => (!args.provider || entry.provider === args.provider)
    && (!args.query || entry.title.toLowerCase().includes(args.query.trim().toLowerCase()))
    && (!workspace || membership.get(entry.importId) === (workspace.kind === "unassigned" ? null : workspace.sourceWorkspaceId)));
  if (args.sort === "oldest") filtered.reverse();
  return {
    items: filtered.slice(start, start + args.limit), total: filtered.length, providers: ["codex", "cursor"],
    nextCursor: start + args.limit < filtered.length ? String(start + args.limit) : null,
  };
}
const calls = (command: string) => mocks.invoke.mock.calls.filter(([name]) => name === command);
const rows = () => document.querySelectorAll(".tree-imported__row");
const tick = (ms: number) => act(async () => { await vi.advanceTimersByTimeAsync(ms); });
async function mount(variant: "sidebar" | "settings" = "sidebar") {
  let view!: ReturnType<typeof render>;
  await act(async () => { view = render(<ImportedHistoryGroup tr={tr} variant={variant} />); });
  return view;
}

beforeEach(() => {
  vi.useFakeTimers();
  sessionStorage.clear();
  mocks.invoke.mockImplementation(hostPage);
  mocks.listen.mockResolvedValue(() => {});
});
afterEach(() => { cleanup(); vi.resetAllMocks(); vi.useRealTimers(); });

test.each(["sidebar", "settings"] as const)("%s bounds DOM and IPC while browsing and searching 10,000 histories", async (variant) => {
  await mount(variant);
  expect(rows()).toHaveLength(50);
  expect(mocks.invoke).toHaveBeenCalledTimes(2);
  // Reuse persistent controls instead of recomputing accessible names for every row action on each page.
  const loadMore = screen.getByRole("button", { name: tr("sessionImport.loadMore") }) as HTMLButtonElement;
  const searchbox = screen.getByRole("searchbox") as HTMLInputElement;
  for (let page = 1; page <= 5; page++) {
    expect(loadMore.isConnected).toBe(true);
    expect(loadMore.disabled).toBe(false);
    await act(async () => { fireEvent.click(loadMore); });
    expect(rows()).toHaveLength(Math.min((page + 1) * 50, IMPORTED_LIST_WINDOW));
    expect(calls("session_import_entries")).toHaveLength(page + 1);
    expect(calls("session_import_entries").at(-1)?.[1]).toMatchObject({ cursor: String(page * 50), limit: 50 });
  }
  expect(document.querySelector(".tree-imported__name")?.textContent).toBe(entries[100].title);
  expect(calls("session_import_workspace_groups")).toHaveLength(1);
  // Scrolling itself cannot start an eager sweep of the remaining catalog.
  fireEvent.scroll(document.querySelector(".tree-imported__list")!, { target: { scrollTop: 100_000 } });
  await tick(1000);
  expect(mocks.invoke).toHaveBeenCalledTimes(7);

  const query = entries[9999].title;
  for (let length = 1; length <= query.length; length++) {
    fireEvent.change(searchbox, { target: { value: query.slice(0, length) } });
    await tick(10);
  }
  expect(searchbox.isConnected).toBe(true);
  expect(searchbox.value).toBe(query);
  expect(calls("session_import_entries")).toHaveLength(6);
  await tick(250);
  expect(rows()).toHaveLength(1);
  expect(document.querySelector(".tree-imported__name")?.textContent).toBe(query);
  expect(calls("session_import_entries")).toHaveLength(7);
  expect(calls("session_import_entries").at(-1)?.[1]).toMatchObject({ cursor: null, limit: 50, query });
  expect(mocks.invoke).toHaveBeenCalledTimes(8);
});

test("10,000 workspaces retain a bounded menu and an exact off-page selection", async () => {
  sessionStorage.setItem("grok:imported-catalog-view:v1:sidebar", JSON.stringify({
    query: "", provider: null, sort: "newest", workspace: { kind: "workspace", sourceWorkspaceId: groups[9999].sourceWorkspaceId },
  }));
  await mount();
  expect(rows()).toHaveLength(1);
  expect(document.querySelector(".tree-imported__name")?.textContent).toBe(entries[9999].title);
  expect(screen.getByRole("button", { name: tr("sessionImport.workspaceFilter.title") }).textContent).toContain(groups[9999].displayName);
  expect(mocks.invoke).toHaveBeenCalledTimes(2);
  for (let page = 1; page <= 5; page++) {
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: tr("sessionImport.workspaceFilter.more") })); });
    expect(calls("session_import_workspace_groups")).toHaveLength(page + 1);
    expect(calls("session_import_entries")).toHaveLength(1);
  }
  fireEvent.click(screen.getByRole("button", { name: tr("sessionImport.workspaceFilter.title") }));
  await tick(50);
  expect(screen.getAllByRole("option")).toHaveLength(IMPORTED_LIST_WINDOW + 3);
  const selected = screen.getAllByRole("option").find((option) => option.getAttribute("aria-selected") === "true");
  expect(selected?.textContent).toContain(groups[9999].displayName);
  fireEvent.keyDown(screen.getByRole("listbox"), { key: "Home" });
  expect(document.activeElement).toBe(within(screen.getByRole("option", { name: tr("sessionImport.workspaceFilter.all") })).getByRole("button"));
  fireEvent.keyDown(screen.getByRole("listbox"), { key: "End" });
  expect(document.activeElement).toBe(within(selected!).getByRole("button"));
  await act(async () => {
    fireEvent.click(within(screen.getByRole("option", { name: tr("sessionImport.association.none") })).getByRole("button"));
  });
  expect(calls("session_import_entries").at(-1)?.[1]).toMatchObject({ cursor: null, limit: 50, workspace: { kind: "unassigned" } });
  expect(rows()).toHaveLength(50);
  expect(calls("session_import_entries")).toHaveLength(2);
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Codex" })); });
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: tr("sessionImport.sortNewest") })); });
  expect(calls("session_import_entries")).toHaveLength(4);
  expect(calls("session_import_entries").at(-1)?.[1]).toMatchObject({ provider: "codex", sort: "oldest", limit: 50, workspace: { kind: "unassigned" } });
  expect(rows()).toHaveLength(50);
});

test("every page of 10,000 entries and groups stays bounded without automatic IPC", async () => {
  const { result } = renderHook(() => ({ catalog: useImportedCatalog(), groups: useImportedWorkspaceGroups(groups[9999].sourceWorkspaceId) }));
  await act(async () => {});
  for (let page = 0; page < 200; page++) {
    if (page) await act(async () => { await Promise.all([result.current.catalog.loadMore(), result.current.groups.loadMore()]); });
    expect(result.current.catalog.entries).toHaveLength(Math.min((page + 1) * 50, IMPORTED_LIST_WINDOW));
    expect(result.current.groups.items).toHaveLength(Math.min((page + 1) * 50, IMPORTED_LIST_WINDOW));
    expect(result.current.catalog.entries.at(-1)?.importId).toBe(entries[(page + 1) * 50 - 1].importId);
    expect(result.current.groups.items.at(-1)?.sourceWorkspaceId).toBe(groups[(page + 1) * 50 - 1].sourceWorkspaceId);
    expect(result.current.groups.selected?.sourceWorkspaceId).toBe(groups[9999].sourceWorkspaceId);
    expect(mocks.invoke).toHaveBeenCalledTimes(2 * (page + 1));
  }
  expect(result.current.catalog.nextCursor).toBeNull();
  expect(result.current.groups.cursor).toBeNull();
  await act(async () => { await Promise.all([result.current.catalog.loadMore(), result.current.groups.loadMore()]); });
  await tick(1000);
  expect(mocks.invoke).toHaveBeenCalledTimes(400);
  expect(mocks.invoke.mock.calls.every(([, args]) => args.limit === 50)).toBe(true);
});

test("publication hints for 10,000 entries refresh each list once without per-entry reads", async () => {
  const view = await mount();
  const hint = () => {
    for (const entry of entries) {
      for (const [, callback] of mocks.listen.mock.calls) callback({ payload: { importId: entry.importId } });
    }
  };
  act(hint);
  expect(mocks.invoke).toHaveBeenCalledTimes(2);
  await tick(1000);
  expect(calls("session_import_entries")).toHaveLength(2);
  expect(calls("session_import_workspace_groups")).toHaveLength(2);
  expect(mocks.invoke).toHaveBeenCalledTimes(4);
  expect(rows()).toHaveLength(50);
  act(hint);
  view.unmount();
  await tick(5000);
  expect(mocks.invoke).toHaveBeenCalledTimes(4);
  expect(vi.getTimerCount()).toBe(0);
});

test("query edits consume queued hints; explicit filters and clearing search stay immediate", async () => {
  const view = await mount();
  fireEvent.change(screen.getByRole("searchbox"), { target: { value: entries[9999].title } });
  expect(calls("session_import_entries")).toHaveLength(1);
  // Hints cannot bypass the pending query, even on the same timer boundary.
  act(() => { for (const [, callback] of mocks.listen.mock.calls) callback(); });
  await tick(250);
  expect(calls("session_import_entries")).toHaveLength(2);
  await tick(1000);
  expect(calls("session_import_entries")).toHaveLength(2);
  fireEvent.change(screen.getByRole("searchbox"), { target: { value: "History 00001" } });
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Cursor" })); });
  expect(calls("session_import_entries")).toHaveLength(3);
  await tick(1000);
  expect(calls("session_import_entries")).toHaveLength(3);
  fireEvent.change(screen.getByRole("searchbox"), { target: { value: "History 00003" } });
  await act(async () => { fireEvent.change(screen.getByRole("searchbox"), { target: { value: "" } }); });
  expect(calls("session_import_entries")).toHaveLength(4);
  expect(rows()).toHaveLength(50);
  fireEvent.change(screen.getByRole("searchbox"), { target: { value: "History 00003" } });
  view.unmount();
  await tick(5000);
  expect(calls("session_import_entries")).toHaveLength(4);
  expect(vi.getTimerCount()).toBe(0);
});

test.each(["sidebar", "settings"] as const)("%s refresh button immediately consumes a scheduled search", async (variant) => {
  const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
  await mount(variant);
  const refresh = screen.getByRole("button", { name: tr("sessionImport.refresh") }) as HTMLButtonElement;
  const clickRefresh = () => act(async () => {
    const click = user.click(refresh);
    // Drain userEvent/RTL's zero-delay tasks without reaching the query timer.
    await vi.advanceTimersByTimeAsync(0);
    await click;
  });
  fireEvent.change(screen.getByRole("searchbox"), { target: { value: "History 00002" } });
  expect(refresh.disabled).toBe(false);
  expect(calls("session_import_entries")).toHaveLength(1);
  let finish!: (page: unknown) => void;
  mocks.invoke.mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
  await clickRefresh();
  expect(calls("session_import_entries")).toHaveLength(2);
  expect(calls("session_import_entries").at(-1)?.[1]).toMatchObject({ query: "History 00002", cursor: null, limit: 50 });
  expect(refresh.disabled).toBe(true);
  await clickRefresh();
  expect(calls("session_import_entries")).toHaveLength(2);
  await act(async () => { finish(await hostPage("session_import_entries", { query: "History 00002", cursor: null, limit: 50 })); });
  expect(rows()).toHaveLength(1);
  expect(document.querySelector(".tree-imported__name")?.textContent).toBe("History 00002");
  expect(refresh.disabled).toBe(false);
  await tick(1000);
  expect(calls("session_import_entries")).toHaveLength(2);
});

test("whitespace-only edits collapse into one immediate clear without repeated catalog reads", async () => {
  await mount();
  const searchbox = screen.getByRole("searchbox") as HTMLInputElement;
  for (const value of [" ", "  ", "\t "]) {
    fireEvent.change(searchbox, { target: { value } });
    expect(searchbox.value).toBe("");
  }
  expect(rows()).toHaveLength(50);
  expect(calls("session_import_entries")).toHaveLength(1);
  fireEvent.change(searchbox, { target: { value: "History 00002" } });
  await tick(250);
  expect(rows()).toHaveLength(1);
  expect(calls("session_import_entries")).toHaveLength(2);
  await act(async () => { fireEvent.change(searchbox, { target: { value: " " } }); });
  expect(searchbox.value).toBe("");
  expect(rows()).toHaveLength(50);
  expect(calls("session_import_entries")).toHaveLength(3);
  expect(calls("session_import_entries").at(-1)?.[1]).toMatchObject({ query: "", cursor: null, limit: 50 });
  expect(JSON.parse(sessionStorage.getItem("grok:imported-catalog-view:v1:sidebar")!).query).toBe("");
  for (const value of ["  ", "\t "]) fireEvent.change(searchbox, { target: { value } });
  await tick(1000);
  expect(searchbox.value).toBe("");
  expect(calls("session_import_entries")).toHaveLength(3);
});

test("a superseded page cannot repopulate the list during the query pause", async () => {
  await mount();
  let finish!: (page: unknown) => void;
  mocks.invoke.mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
  fireEvent.click(screen.getByRole("button", { name: tr("sessionImport.loadMore") }));
  fireEvent.change(screen.getByRole("searchbox"), { target: { value: entries[9999].title } });
  await act(async () => { finish(await hostPage("session_import_entries", { cursor: "50", limit: 50 })); });
  expect(rows()).toHaveLength(0);
  expect((screen.getByRole("button", { name: tr("sessionImport.refresh") }) as HTMLButtonElement).disabled).toBe(false);
  expect(calls("session_import_entries")).toHaveLength(2);
  await tick(250);
  expect(rows()).toHaveLength(1);
  expect(document.querySelector(".tree-imported__name")?.textContent).toBe(entries[9999].title);
  expect(calls("session_import_entries")).toHaveLength(3);
});

test("failed searches keep the query and expose immediate retry", async () => {
  await mount();
  mocks.invoke.mockRejectedValueOnce(new Error("SOURCE_BUSY"));
  fireEvent.change(screen.getByRole("searchbox"), { target: { value: entries[9999].title } });
  await tick(250);
  expect(screen.getByRole("alert").textContent).toBe(tr("sessionImport.catalogError"));
  expect(screen.queryByText(tr("sessionImport.filterEmpty"))).toBeNull();
  expect((screen.getByRole("searchbox") as HTMLInputElement).value).toBe(entries[9999].title);
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: tr("sessionImport.refresh") })); });
  expect(rows()).toHaveLength(1);
  expect(screen.queryByRole("alert")).toBeNull();
  expect(calls("session_import_entries")).toHaveLength(3);
});
