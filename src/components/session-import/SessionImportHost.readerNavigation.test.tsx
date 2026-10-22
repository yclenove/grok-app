/** @vitest-environment jsdom */
import type { ReactNode } from "react";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { SessionImportHost } from "./SessionImportHost";

const mocks = vi.hoisted(() => ({
  events: vi.fn(), search: vi.fn(), context: vi.fn(), remember: vi.fn(),
  readerId: "imp:hit-navigation", revision: null as string | null,
}));
vi.mock("@/lib/api/sessionMigration", async (original) => ({
  ...await original<typeof import("@/lib/api/sessionMigration")>(),
  sessionImportEvents: mocks.events, sessionImportSearch: mocks.search,
  sessionImportEventContext: mocks.context,
}));
vi.mock("@/lib/sessionImport/open", async (original) => ({
  ...await original<typeof import("@/lib/sessionImport/open")>(),
  rememberImportedHistoryAnchor: mocks.remember,
}));
vi.mock("@/components/GlassModal", () => ({ GlassModal: ({ open, children, footer }: {
  open: boolean; children: ReactNode; footer?: ReactNode;
}) => open ? <div role="dialog">{children}{footer}</div> : null }));
vi.mock("@/hooks/useSessionImport", () => ({ useSessionImportUi: () => ({
  open: false, step: "source", busy: false, readerId: mocks.readerId,
  readerRevision: mocks.revision, readerLeaf: null,
  candidates: [], mappings: [], selected: new Set(), selectedCount: 0,
  prepareItems: [], prepareBatch: null,
}) }));
vi.mock("@/hooks/useImportedCatalog", () => ({ useImportedCatalog: () => ({ entries: [] }) }));
vi.mock("@/hooks/useSessionImportContentStates", () => ({ useSessionImportContentStates: () => ({ ensureSelected: () => {} }) }));
vi.mock("./ImportedWorkspaceAssociation", () => ({ ImportedWorkspaceAssociation: () => null }));
vi.mock("./SessionImportContinuationModal", () => ({ SessionImportContinuationModal: () => null }));

const user = { id: "u1", role: "user", text: "User body 025", truncated: true };
const assistant = { id: "a1", role: "assistant", text: "Assistant body 025" };
const assistantHit = { eventId: "a1", kind: "message", snippet: "Assistant match 025" };
const userHit = { eventId: "u1", kind: "message", snippet: "User match 025" };
const page = (events = [user, assistant], revision = "revision-a", leaf = "leaf-a") => ({
  events, revision, leaf, previousCursor: null, nextCursor: null,
  total: events.length, start: 0, end: events.length,
});
const button = (name: string) => screen.getByRole("button", { name });
const event = (id: string) => document.getElementById(`import-ev-${id}`);
const filters = () => {
  fireEvent.click(button("User")); fireEvent.click(button("Issues only"));
  expect(event("a1")).toBeNull(); expect(event("u1")).not.toBeNull();
};
async function search() {
  const input = screen.getByRole("searchbox") as HTMLInputElement;
  await waitFor(() => expect(input.disabled).toBe(false));
  fireEvent.change(input, { target: { value: "025" } });
  fireEvent.submit(input.closest("form")!);
  await screen.findByRole("button", { name: "Message text Assistant match 025" });
  return input;
}
async function renderReader() {
  const view = render(<SessionImportHost locale="en" />);
  await screen.findByText(user.text);
  return view;
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}
beforeEach(() => {
  vi.resetAllMocks(); mocks.readerId = "imp:hit-navigation"; mocks.revision = null;
  mocks.events.mockResolvedValue(page());
  mocks.search.mockResolvedValue({ hits: [assistantHit], previousCursor: null, nextCursor: null });
  mocks.context.mockResolvedValue(page());
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", { configurable: true, value: vi.fn() });
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); });

const actions = ["click", "next", "previous", "n", "p"] as const;
for (const loaded of [true, false]) {
  test.each(actions)(`%s reveals a filtered assistant hit (${loaded ? "loaded" : "bounded context"})`, async (action) => {
    if (!loaded) mocks.events.mockResolvedValueOnce(page([user]));
    await renderReader(); const input = await search(); filters();
    if (action === "click") fireEvent.click(button("Message text Assistant match 025"));
    else if (action === "next") fireEvent.click(button("Next match"));
    else if (action === "previous") fireEvent.click(button("Previous match"));
    else fireEvent.keyDown(event("u1")!, { key: action });
    await waitFor(() => expect(event("a1")).not.toBeNull());
    expect(button("All roles").getAttribute("aria-pressed")).toBe("true");
    expect(button("Issues only").getAttribute("aria-pressed")).toBe("false");
    expect(event("a1")!.getAttribute("data-highlight")).toBe("1");
    await waitFor(() => expect(document.activeElement).toBe(event("a1")));
    expect(screen.getByRole("searchbox")).toBe(input); expect(input.value).toBe("025");
    expect(mocks.search).toHaveBeenCalledTimes(1);
    expect(mocks.search).toHaveBeenCalledWith("imp:hit-navigation", "025", "revision-a", null, "leaf-a");
    expect(mocks.events).toHaveBeenCalledTimes(1);
    if (loaded) expect(mocks.context).not.toHaveBeenCalled();
    else expect(mocks.context).toHaveBeenCalledExactlyOnceWith("imp:hit-navigation", "a1", "revision-a", "leaf-a", { before: 8, after: 8 });
    expect(mocks.remember).toHaveBeenLastCalledWith({ importId: "imp:hit-navigation", revision: "revision-a", leaf: "leaf-a", anchorEventId: "a1" });
  });
}

test("the second native-reproduced hit is visible and previous/next wrap without losing the query", async () => {
  mocks.search.mockResolvedValue({ hits: [userHit, assistantHit], previousCursor: null, nextCursor: null });
  await renderReader(); await search(); fireEvent.click(button("User"));
  fireEvent.click(button("Message text Assistant match 025"));
  await waitFor(() => expect(document.activeElement).toBe(event("a1")));
  expect(event("a1")).not.toBeNull();
  fireEvent.click(button("Previous match"));
  await waitFor(() => expect(document.activeElement).toBe(event("u1")));
  fireEvent.click(button("Previous match"));
  await waitFor(() => expect(document.activeElement).toBe(event("a1")));
  fireEvent.click(button("Next match"));
  await waitFor(() => expect(document.activeElement).toBe(event("u1")));
  expect((screen.getByRole("searchbox") as HTMLInputElement).value).toBe("025");
});

test("search alone, typing and composing shortcuts do not reset display filters or jump", async () => {
  await renderReader(); filters(); const input = await search();
  fireEvent.keyDown(input, { key: "n" });
  fireEvent.keyDown(event("u1")!, { key: "n", isComposing: true });
  fireEvent.keyDown(event("u1")!, { key: "p", keyCode: 229 });
  fireEvent.keyDown(event("u1")!, { key: "n", ctrlKey: true });
  expect(button("User").getAttribute("aria-pressed")).toBe("true");
  expect(button("Issues only").getAttribute("aria-pressed")).toBe("true");
  expect(event("a1")).toBeNull(); expect(mocks.context).not.toHaveBeenCalled();
  expect(mocks.remember).not.toHaveBeenCalled();
});

test("a late context cannot replace a newer selected hit", async () => {
  const pending = deferred<ReturnType<typeof page>>();
  mocks.events.mockResolvedValueOnce(page([user])); mocks.context.mockReturnValueOnce(pending.promise);
  mocks.search.mockResolvedValue({ hits: [userHit, assistantHit], previousCursor: null, nextCursor: null });
  await renderReader(); await search(); filters();
  fireEvent.click(button("Message text Assistant match 025"));
  fireEvent.click(button("Message text User match 025"));
  await act(async () => pending.resolve(page()));
  await waitFor(() => expect(document.activeElement).toBe(event("u1")));
  expect(event("a1")).toBeNull();
  expect(mocks.remember).toHaveBeenCalledTimes(1);
  expect(mocks.remember.mock.calls[0][0].anchorEventId).toBe("u1");
});

test("a late context cannot reveal or remember a hit after the reader changes identity", async () => {
  const pending = deferred<ReturnType<typeof page>>();
  mocks.events.mockResolvedValueOnce(page([user])); mocks.context.mockReturnValueOnce(pending.promise);
  const view = await renderReader(); await search(); filters();
  fireEvent.click(button("Message text Assistant match 025"));
  mocks.readerId = "imp:other";
  mocks.events.mockResolvedValueOnce(page([{ id: "other", role: "user", text: "Other archive" }], "revision-b", "leaf-b"));
  view.rerender(<SessionImportHost locale="en" />); await screen.findByText("Other archive");
  await act(async () => pending.resolve(page()));
  expect(event("a1")).toBeNull(); expect(mocks.remember).not.toHaveBeenCalled();
  expect(event("other")).not.toBeNull();
});

test("an explicit filter change while context loads remains the latest display intent", async () => {
  const pending = deferred<ReturnType<typeof page>>();
  mocks.events.mockResolvedValueOnce(page([user])); mocks.context.mockReturnValueOnce(pending.promise);
  await renderReader(); await search(); filters();
  fireEvent.click(button("Message text Assistant match 025"));
  expect(button("All roles").getAttribute("aria-pressed")).toBe("true");
  fireEvent.click(button("User"));
  await act(async () => pending.resolve(page()));
  expect(button("User").getAttribute("aria-pressed")).toBe("true");
  expect(event("a1")).toBeNull();
});

test("denied context does not fabricate the selected body or write a successful anchor", async () => {
  mocks.events.mockResolvedValueOnce(page([user]));
  mocks.context.mockRejectedValueOnce(new Error("HISTORY_ACCESS_UNAVAILABLE: hidden"));
  await renderReader(); await search(); filters();
  fireEvent.click(button("Message text Assistant match 025"));
  await waitFor(() => expect(document.querySelector("[data-import-reader-error]")).not.toBeNull());
  expect(event("a1")).toBeNull(); expect(mocks.remember).not.toHaveBeenCalled();
  expect(mocks.context).toHaveBeenCalledTimes(1);
});

test("search failure leaves the transcript readable and clear dismisses the search error", async () => {
  await renderReader();
  mocks.search.mockRejectedValueOnce(new Error("RESOURCE_LIMIT: search query exceeds 4 KiB"));
  const input = screen.getByRole("searchbox");
  fireEvent.change(input, { target: { value: "query" } });
  fireEvent.submit((input as HTMLInputElement).form!);
  await waitFor(() => expect(document.querySelector("[data-import-search-error]")).not.toBeNull());
  expect(event("u1")).not.toBeNull();
  expect(event("a1")).not.toBeNull();
  expect(document.querySelector("[data-import-reader-error]")).toBeNull();
  expect((button("Copy visible") as HTMLButtonElement).disabled).toBe(false);
  fireEvent.click(button("Clear"));
  expect(document.querySelector("[data-import-search-error]")).toBeNull();
  expect(event("a1")).not.toBeNull();
  expect(mocks.events).toHaveBeenCalledTimes(1);
});

test("oversize composing query stays editable and is validated only on explicit non-IME submit", async () => {
  await renderReader();
  const input = screen.getByRole("searchbox") as HTMLInputElement;
  const query = "你".repeat(1366);
  fireEvent.compositionStart(input);
  fireEvent.change(input, { target: { value: query } });
  fireEvent.keyDown(input, { key: "Enter", isComposing: true });
  fireEvent.submit(input.form!);
  expect(document.querySelector("[data-import-search-error]")).toBeNull();
  expect(mocks.search).not.toHaveBeenCalled();
  expect(input.value).toBe(query);
  fireEvent.compositionEnd(input, { data: query });
  fireEvent.keyUp(input, { key: "Enter" });
  fireEvent.submit(input.form!);
  await screen.findByText("Search text is too long. Shorten it to 4 KiB or less (UTF-8).");
  expect(mocks.search).not.toHaveBeenCalled();
  expect(event("a1")).not.toBeNull();
  fireEvent.change(input, { target: { value: "025" } });
  fireEvent.submit(input.form!);
  await screen.findByText("Matches: 1");
  expect(document.querySelector("[data-import-search-error]")).toBeNull();
});

test("a single search hit uses a count label instead of a plural sentence", async () => {
  await renderReader(); await search();
  expect(screen.getByText("Matches: 1")).toBeTruthy();
  expect(screen.queryByText("1 matches")).toBeNull();
});
