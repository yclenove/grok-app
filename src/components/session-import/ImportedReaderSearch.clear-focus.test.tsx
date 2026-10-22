/** @vitest-environment jsdom */
import "@testing-library/jest-dom/vitest";
import { useState } from "react";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createT } from "@/i18n";
import { installDialogFocus } from "@/lib/a11yFocus";
import { useImportedReader } from "@/hooks/useImportedReader";
import { ImportedReaderSearch } from "./ImportedReaderSearch";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => { resolve = res; });
  return { promise, resolve };
}

const mocks = vi.hoisted(() => ({
  events: vi.fn(),
  search: vi.fn(),
  context: vi.fn(),
  exportMd: vi.fn(),
}));
vi.mock("@/lib/api/sessionMigration", () => ({
  sessionImportEvents: (...args: [string, string | null, string | null, number, string | null, ("forward" | "backward")?]) => {
    const pending = mocks.events(...args);
    return Promise.resolve(pending).then(async (page) => {
      const { importedReaderPageFixture } = await import("@/test/importedReaderPageFixture");
      return importedReaderPageFixture(page, args[1], args[4]);
    });
  },
  sessionImportSearch: (...args: unknown[]) => mocks.search(...args),
  sessionImportEventContext: (...args: unknown[]) => mocks.context(...args),
  sessionImportExportMarkdown: (...args: unknown[]) => mocks.exportMd(...args),
}));

afterEach(cleanup);
const tr = createT("en");
const searchName = tr("sessionImport.search");
const clearName = tr("sessionImport.clearSearch");
const IMPORT_ID = "imp:611d8adf-8e9f-4388-a609-61f1bfea5de5";
const REVISION = "f72d9d75-096a-4415-a786-65f6297bad19";
const LEAF = "codex:v152-reader-r1-e0205";

beforeEach(() => {
  vi.resetAllMocks();
  mocks.events.mockResolvedValue({
    events: [{ id: "e93" }, { id: "e109" }],
    previousCursor: "c92",
    nextCursor: "c110",
  });
  mocks.search.mockResolvedValue({
    hits: [{ eventId: "e101", ordinal: 101, snippet: "跨页检索🙂", kind: "message" }],
    previousCursor: null,
    nextCursor: "h102",
  });
  mocks.context.mockResolvedValue({ events: [], previousCursor: null, nextCursor: null });
  mocks.exportMd.mockResolvedValue("# leaf\n");
});

function FocusHarness(props: {
  busy?: boolean;
  identity?: string;
  open?: boolean;
  query?: string;
  hasHits?: boolean;
  retainHits?: boolean;
  onSearch: (query: string) => void;
  onClear: () => void;
}) {
  const [query, setQuery] = useState(props.query ?? "跨页检索🙂");
  const [hasHits, setHasHits] = useState(props.hasHits ?? true);
  if (props.open === false) return <textarea aria-label="composer" />;
  return (
    <>
      <button type="button">Close</button>
      <textarea aria-label="composer" />
      <ImportedReaderSearch
        key={props.identity ?? "reader-a"}
        tr={tr}
        busy={props.busy ?? false}
        query={query}
        hasHits={props.retainHits ? true : hasHits}
        onQueryChange={setQuery}
        onSearch={() => props.onSearch(query)}
        onClear={() => {
          props.onClear();
          setQuery("");
          if (!props.retainHits) setHasHits(false);
        }}
      />
    </>
  );
}

function setup(props: Omit<Parameters<typeof FocusHarness>[0], "onSearch" | "onClear"> = {}) {
  const onSearch = vi.fn();
  const onClear = vi.fn();
  const user = userEvent.setup();
  const view = render(<FocusHarness {...props} onSearch={onSearch} onClear={onClear} />);
  const input = screen.getByRole("searchbox") as HTMLInputElement;
  const clear = screen.getByRole("button", { name: clearName });
  return { ...view, user, input, clear, onSearch, onClear };
}

async function clearAndExpectInput(user: ReturnType<typeof userEvent.setup>, input: HTMLInputElement, clear: HTMLElement) {
  clear.focus();
  expect(document.activeElement).toBe(clear);
  await user.click(clear);
  expect(document.activeElement).toBe(input);
  expect(input).toHaveValue("");
  expect(input).toBeEnabled();
  expect(screen.getByRole("button", { name: "Close" })).not.toHaveFocus();
}

it("returns focus to the search input when Clear removes its own focused button", async () => {
  const { user, input, clear, onSearch, onClear } = setup();
  await clearAndExpectInput(user, input, clear);
  expect(onClear).toHaveBeenCalledOnce();
  expect(onSearch).not.toHaveBeenCalled();
  expect(screen.queryByRole("button", { name: clearName })).toBeNull();
  await user.keyboard("下一页");
  expect(input).toHaveValue("下一页");
  await user.keyboard("{Enter}");
  expect(onSearch).toHaveBeenCalledExactlyOnceWith("下一页");
});

it.each(["{Enter}", " "])("keyboard Clear (%s) clears once and does not insert the activation key", async (key) => {
  const { user, input, clear, onSearch, onClear } = setup();
  clear.focus();
  await user.keyboard(key);
  expect(onClear).toHaveBeenCalledOnce();
  expect(onSearch).not.toHaveBeenCalled();
  expect(document.activeElement).toBe(input);
  expect(input).toHaveValue("");
  await user.keyboard("kept");
  expect(input).toHaveValue("kept");
  await user.keyboard("{Enter}");
  expect(onSearch).toHaveBeenCalledExactlyOnceWith("kept");
});

it("keeps modified and composing keys from activating Clear", () => {
  const { input, clear, onSearch, onClear } = setup();
  clear.focus();
  for (const init of [
    { key: "Enter", shiftKey: true },
    { key: "Enter", ctrlKey: true },
    { key: "Enter", altKey: true },
    { key: "Enter", metaKey: true },
    { key: "Enter", isComposing: true },
    { key: "Enter", keyCode: 229 },
    { key: " ", shiftKey: true },
  ]) {
    const event = new KeyboardEvent("keydown", { ...init, bubbles: true, cancelable: true });
    fireEvent(clear, event);
    expect(event.defaultPrevented).toBe(false);
  }
  expect(onClear).not.toHaveBeenCalled();
  expect(onSearch).not.toHaveBeenCalled();
  expect(input).toHaveValue("跨页检索🙂");
  expect(document.activeElement).toBe(clear);
});

it("supports an empty query with retained hits and a second clear", async () => {
  const { user, input, onSearch, onClear } = setup({ query: "", retainHits: true });
  const clear = screen.getByRole("button", { name: clearName });
  clear.focus();
  await user.keyboard(" ");
  expect(onClear).toHaveBeenCalledOnce();
  expect(onSearch).not.toHaveBeenCalled();
  expect(document.activeElement).toBe(input);
  expect(input).toHaveValue("");
  expect(screen.getByRole("button", { name: clearName })).toBeEnabled();
  await user.click(screen.getByRole("button", { name: clearName }));
  expect(onClear).toHaveBeenCalledTimes(2);
  expect(onSearch).not.toHaveBeenCalled();
  expect(document.activeElement).toBe(input);
});

it("repeats clear and a deliberate search without auto-submitting", async () => {
  const { user, input, onSearch, onClear } = setup();
  await user.click(screen.getByRole("button", { name: clearName }));
  await user.keyboard("first");
  await user.click(screen.getByRole("button", { name: searchName }));
  expect(onSearch).toHaveBeenCalledExactlyOnceWith("first");
  await user.click(input);
  await user.keyboard(" second");
  await user.click(screen.getByRole("button", { name: clearName }));
  expect(onClear).toHaveBeenCalledTimes(2);
  expect(onSearch).toHaveBeenCalledTimes(1);
  expect(document.activeElement).toBe(input);
  expect(input).toHaveValue("");
  await user.keyboard("third{Enter}");
  expect(onSearch).toHaveBeenLastCalledWith("third");
  expect(onSearch).toHaveBeenCalledTimes(2);
});

it("leaves typing available while busy and does not search until a later plain Enter", async () => {
  const { user, input, clear, onSearch, onClear, rerender } = setup({ busy: true });
  expect(input).toBeEnabled();
  expect(screen.getByRole("button", { name: searchName })).toBeDisabled();
  fireEvent.change(input, { target: { value: "跨页检索🙂 more" } });
  expect(input).toHaveValue("跨页检索🙂 more");
  await user.click(clear);
  expect(onClear).toHaveBeenCalledOnce();
  expect(onSearch).not.toHaveBeenCalled();
  expect(document.activeElement).toBe(input);
  await user.keyboard("draft{Enter}");
  expect(input).toHaveValue("draft");
  expect(onSearch).not.toHaveBeenCalled();
  rerender(<FocusHarness busy={false} query="" hasHits={false} onSearch={onSearch} onClear={onClear} />);
  input.focus();
  await user.keyboard("{Enter}");
  expect(onSearch).toHaveBeenCalledExactlyOnceWith("draft");
});

it("does not steal focus when a connected Clear click does not own focus", () => {
  // Component contract: fireEvent.click does not move focus first. A real
  // pointer click does, and is covered by the mouse test above. The reachable
  // hook path while a composer is focused is covered further down.
  const { input, onClear, onSearch } = setup({ query: "跨页检索🙂", retainHits: true });
  const clear = screen.getByRole("button", { name: clearName });
  const composer = screen.getByRole("textbox", { name: "composer" });
  const close = screen.getByRole("button", { name: "Close" });
  composer.focus();
  fireEvent.click(clear);
  expect(onClear).toHaveBeenCalledOnce();
  expect(onSearch).not.toHaveBeenCalled();
  expect(input).toHaveValue("");
  expect(clear.isConnected).toBe(true);
  expect(document.activeElement).toBe(composer);
  close.focus();
  fireEvent.click(clear);
  expect(onClear).toHaveBeenCalledTimes(2);
  expect(document.activeElement).toBe(close);
  expect(screen.getByRole("searchbox")).not.toHaveFocus();
});

it("drops a covered Clear and does not revive it when the newer dialog closes", async () => {
  const { user, input, clear, onClear, onSearch } = setup();
  const composer = screen.getByRole("textbox", { name: "composer" });
  composer.focus();
  const overlay = document.createElement("div");
  overlay.setAttribute("role", "dialog");
  const overlayInput = document.createElement("input");
  overlayInput.setAttribute("aria-label", "overlay");
  overlay.append(overlayInput);
  document.body.append(overlay);
  const stop = installDialogFocus(() => overlay, { initialFocus: "none" });
  try {
    overlayInput.focus();
    await user.click(clear);
    expect(onClear).not.toHaveBeenCalled();
    expect(onSearch).not.toHaveBeenCalled();
    expect(input).toHaveValue("跨页检索🙂");
    expect(document.activeElement).not.toBe(input);
  } finally {
    stop();
    overlay.remove();
  }
  expect(document.activeElement).not.toBe(input);
  expect(input).toHaveValue("跨页检索🙂");
  expect(onClear).not.toHaveBeenCalled();
  await user.click(screen.getByRole("button", { name: clearName }));
  expect(onClear).toHaveBeenCalledOnce();
  expect(document.activeElement).toBe(input);
});

it("does not focus a search field mounted for a different reader identity", async () => {
  const onSearch = vi.fn();
  const onClear = vi.fn();
  const user = userEvent.setup();
  function RemountHarness() {
    const [identity, setIdentity] = useState("leaf-a");
    const [query, setQuery] = useState("跨页检索🙂");
    const [hasHits, setHasHits] = useState(true);
    return (
      <>
        <textarea aria-label="composer" />
        <ImportedReaderSearch
          key={identity}
          tr={tr}
          busy={false}
          query={query}
          hasHits={hasHits}
          onQueryChange={setQuery}
          onSearch={() => onSearch(query)}
          onClear={() => {
            onClear();
            setQuery("");
            setHasHits(false);
            setIdentity("leaf-b");
          }}
        />
      </>
    );
  }
  render(<RemountHarness />);
  await user.click(screen.getByRole("button", { name: clearName }));
  expect(onClear).toHaveBeenCalledOnce();
  expect(onSearch).not.toHaveBeenCalled();
  expect(document.activeElement).not.toBe(screen.getByRole("searchbox"));
  expect(screen.getByRole("textbox", { name: "composer" })).not.toHaveFocus();
});

it("unmounts the reader from Clear without focusing the composer", async () => {
  const onSearch = vi.fn();
  const onClear = vi.fn();
  const user = userEvent.setup();
  function CloseHarness() {
    const [open, setOpen] = useState(true);
    const [query, setQuery] = useState("跨页检索🙂");
    const [hasHits, setHasHits] = useState(true);
    if (!open) return <textarea aria-label="composer" />;
    return (
      <>
        <textarea aria-label="composer" />
        <ImportedReaderSearch
          tr={tr}
          busy={false}
          query={query}
          hasHits={hasHits}
          onQueryChange={setQuery}
          onSearch={() => onSearch(query)}
          onClear={() => {
            onClear();
            setQuery("");
            setHasHits(false);
            setOpen(false);
          }}
        />
      </>
    );
  }
  render(<CloseHarness />);
  const composer = screen.getByRole("textbox", { name: "composer" });
  await user.click(screen.getByRole("button", { name: clearName }));
  expect(onClear).toHaveBeenCalledOnce();
  expect(screen.queryByRole("searchbox")).toBeNull();
  expect(document.activeElement).not.toBe(composer);
});

function LiveReader(props: { importId?: string; revision?: string; leaf?: string; remountOnClear?: boolean }) {
  const [leaf, setLeaf] = useState(props.leaf ?? LEAF);
  const importId = props.importId ?? IMPORT_ID;
  const revision = props.revision ?? REVISION;
  const reader = useImportedReader(importId, () => {}, { revision, leaf });
  live.reader = reader;
  return (
    <>
      <textarea aria-label="composer" />
      <ImportedReaderSearch
        key={`search:${importId}:${revision}:${leaf}`}
        tr={tr}
        busy={reader.busy || !reader.scope}
        query={reader.query}
        hasHits={reader.hits.length > 0}
        onQueryChange={reader.setQuery}
        onSearch={() => { void reader.search(); }}
        onClear={() => {
          reader.clearSearch();
          if (props.remountOnClear) setLeaf("leaf-next");
        }}
      />
      <pre data-testid="reader">{JSON.stringify({
        events: reader.events,
        query: reader.query,
        hits: reader.hits.map((hit) => hit.eventId),
        scope: reader.scope,
        previousCursor: reader.previousCursor,
        nextCursor: reader.nextCursor,
        hitsNext: reader.hitsNext,
        busy: reader.busy,
      })}</pre>
    </>
  );
}

const live: { reader: ReturnType<typeof useImportedReader> | null } = { reader: null };

function readerState() {
  return JSON.parse(screen.getByTestId("reader").textContent || "{}") as {
    events: { id: string }[];
    query: string;
    hits: string[];
    scope: { revision: string; leaf: string } | null;
    previousCursor: string | null;
    nextCursor: string | null;
    hitsNext: string | null;
    busy: boolean;
  };
}

async function loadLive(props: Parameters<typeof LiveReader>[0] = {}, eventIds = ["e93", "e109"]) {
  const user = userEvent.setup();
  const view = render(<LiveReader {...props} />);
  await waitFor(() => expect(readerState().busy).toBe(false));
  expect(readerState().events.map((event) => event.id)).toEqual(eventIds);
  expect(readerState().scope).toEqual({ revision: props.revision ?? REVISION, leaf: props.leaf ?? LEAF });
  return { user, ...view };
}

it("keeps the loaded reader when Clear runs during a pending search", async () => {
  const pending = deferred<{ hits: { eventId: string; snippet: string; kind: string }[]; nextCursor: string | null }>();
  mocks.search.mockReturnValueOnce(pending.promise);
  const { user } = await loadLive();
  const input = screen.getByRole("searchbox") as HTMLInputElement;
  fireEvent.change(input, { target: { value: "跨页检索🙂" } });
  await user.click(screen.getByRole("button", { name: searchName }));
  await waitFor(() => expect(readerState().busy).toBe(true));
  expect(input).toBeEnabled();
  fireEvent.change(input, { target: { value: "跨页检索🙂 draft" } });
  expect(input).toHaveValue("跨页检索🙂 draft");
  const before = readerState();
  await user.click(screen.getByRole("button", { name: clearName }));
  expect(document.activeElement).toBe(input);
  expect(input).toHaveValue("");
  expect(readerState().hits).toEqual([]);
  expect(readerState().events).toEqual(before.events);
  expect(readerState().scope).toEqual(before.scope);
  expect(readerState().previousCursor).toBe("c92");
  expect(readerState().nextCursor).toBe("c110");
  await user.keyboard("{Enter}");
  expect(mocks.search).toHaveBeenCalledTimes(1);
  await act(async () => {
    pending.resolve({
      hits: [{ eventId: "late", snippet: "old", kind: "message" }],
      nextCursor: "late-cursor",
    });
    await pending.promise;
  });
  await waitFor(() => expect(readerState().busy).toBe(false));
  expect(readerState().hits).toEqual([]);
  expect(readerState().hitsNext).toBeNull();
  expect(readerState().query).toBe("");
  expect(readerState().events).toEqual(before.events);
  expect(readerState().scope).toEqual(before.scope);
  await user.keyboard("fresh{Enter}");
  await waitFor(() => expect(readerState().hits).toEqual(["e101"]));
  expect(mocks.search).toHaveBeenCalledTimes(2);
  expect(mocks.search).toHaveBeenLastCalledWith(IMPORT_ID, "fresh", REVISION, null, LEAF);
});

it("drops a pending hit page after Clear and does not refill the old results", async () => {
  const { user } = await loadLive();
  const input = screen.getByRole("searchbox") as HTMLInputElement;
  fireEvent.change(input, { target: { value: "跨页检索🙂" } });
  await user.click(screen.getByRole("button", { name: searchName }));
  await waitFor(() => expect(readerState().hits).toEqual(["e101"]));
  const pending = deferred<{ hits: { eventId: string; snippet: string; kind: string }[]; nextCursor: string | null }>();
  mocks.search.mockReturnValueOnce(pending.promise);
  await act(async () => { void live.reader?.loadMoreHits(); });
  expect(readerState().busy).toBe(true);
  const before = readerState();
  await user.click(screen.getByRole("button", { name: clearName }));
  expect(document.activeElement).toBe(input);
  expect(readerState().hits).toEqual([]);
  expect(readerState().query).toBe("");
  await act(async () => {
    pending.resolve({
      hits: [{ eventId: "page-late", snippet: "nope", kind: "message" }],
      nextCursor: "page-cursor",
    });
    await pending.promise;
  });
  await waitFor(() => expect(readerState().busy).toBe(false));
  expect(readerState().hits).toEqual([]);
  expect(readerState().hitsNext).toBeNull();
  expect(readerState().events).toEqual(before.events);
  expect(readerState().scope).toEqual({ revision: REVISION, leaf: LEAF });
  expect(readerState().previousCursor).toBe(before.previousCursor);
  expect(readerState().nextCursor).toBe(before.nextCursor);
});

it("does not move composer focus when the hook clears search directly", async () => {
  const { user } = await loadLive();
  const input = screen.getByRole("searchbox") as HTMLInputElement;
  fireEvent.change(input, { target: { value: "跨页检索🙂" } });
  await user.click(screen.getByRole("button", { name: searchName }));
  await waitFor(() => expect(readerState().hits).toEqual(["e101"]));
  const composer = screen.getByRole("textbox", { name: "composer" });
  const before = readerState();
  composer.focus();
  act(() => live.reader?.clearSearch());
  expect(document.activeElement).toBe(composer);
  expect(screen.getByRole("searchbox")).not.toHaveFocus();
  expect(readerState().query).toBe("");
  expect(readerState().hits).toEqual([]);
  expect(readerState().events).toEqual(before.events);
  expect(readerState().scope).toEqual(before.scope);
  expect(readerState().nextCursor).toBe(before.nextCursor);
});

it("does not focus the next reader identity when Clear remounts it", async () => {
  mocks.events.mockImplementation(async (_importId: string, _revision: string | null, _cursor: string | null, _limit: number, leaf: string | null) => ({
    events: [{ id: leaf === "leaf-next" ? "e-next" : "e93" }],
    previousCursor: "prev",
    nextCursor: "next",
  }));
  const { user } = await loadLive({ remountOnClear: true }, ["e93"]);
  const input = screen.getByRole("searchbox") as HTMLInputElement;
  fireEvent.change(input, { target: { value: "跨页检索🙂" } });
  await user.click(screen.getByRole("button", { name: clearName }));
  await waitFor(() => expect(readerState().scope?.leaf).toBe("leaf-next"));
  expect(document.activeElement).not.toBe(screen.getByRole("searchbox"));
  expect(readerState().query).toBe("");
  expect(readerState().hits).toEqual([]);
  expect(readerState().events).toEqual([{ id: "e-next" }]);
});
