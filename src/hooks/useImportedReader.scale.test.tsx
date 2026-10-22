/** @vitest-environment jsdom */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { MAX_READER_EVENTS } from "@/lib/sessionImport/readerBudget";
import { useImportedReader } from "./useImportedReader";

const mocks = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@/lib/api/host", () => ({ invoke: mocks.invoke }));
const events = Array.from({ length: 10_000 }, (_, i) => ({
  id: `event-${i}`, kind: "message", role: "user", text: `needle ${i}`,
}));
const hits = events.map((event, ordinal) => ({ eventId: event.id, ordinal, snippet: event.text, kind: event.kind }));
const importId = "imp:00000000-0000-4000-8000-000000000001";
const scope = { revision: "scale-revision", leaf: "scale-leaf" };
type Args = {
  cursor: string | null; limit: number; eventId: string;
  before: number; after: number; revision: string | null; leaf: string | null;
};
function eventPage(start: number, end: number) {
  return {
    events: events.slice(start, end), total: events.length, start, end, ...scope,
    previousCursor: start ? String(Math.max(0, start - 100)) : null,
    nextCursor: end < events.length ? String(end) : null,
  };
}
beforeEach(() => {
  mocks.invoke.mockImplementation(async (command: string, args: Args) => {
    const start = Number(args.cursor ?? 0);
    if (command === "session_import_events") return eventPage(start, Math.min(events.length, start + args.limit));
    if (command === "session_import_search") return {
      hits: hits.slice(start, start + args.limit), previousCursor: start ? String(start - args.limit) : null,
      nextCursor: start + args.limit < hits.length ? String(start + args.limit) : null,
    };
    if (command === "session_import_event_context") {
      const ordinal = events.findIndex((event) => event.id === args.eventId);
      const from = Math.max(0, ordinal - args.before);
      return {
        ...eventPage(from, Math.min(events.length, ordinal + args.after + 1)),
        ordinal, position: ordinal, targetIndex: ordinal - from, anchorCursor: String(from),
      };
    }
    throw new Error(`Unexpected IPC: ${command}`);
  });
});
afterEach(() => { cleanup(); vi.resetAllMocks(); });

test("reading all 10,000 events retains at most 800 records and requests one page at a time", async () => {
  const onError = vi.fn();
  const { result } = renderHook(() => useImportedReader(importId, onError));
  await act(async () => {});
  for (let page = 0; page < 100; page++) {
    if (page) await act(() => result.current.loadMore());
    expect(result.current.events).toHaveLength(Math.min((page + 1) * 100, MAX_READER_EVENTS));
    expect(result.current.events.at(-1)).toEqual(events[(page + 1) * 100 - 1]);
    expect(mocks.invoke).toHaveBeenCalledTimes(page + 1);
    expect(mocks.invoke.mock.calls.at(-1)?.[1]).toMatchObject({ limit: 100, cursor: page ? String(page * 100) : null });
  }
  expect(result.current.nextCursor).toBeNull();
  await act(() => result.current.loadMore());
  expect(mocks.invoke).toHaveBeenCalledTimes(100);
  // An old event uses one exact context lookup, never a walk through the archive.
  await act(async () => { expect(await result.current.ensureEvent("event-5000")).toBe(true); });
  expect(mocks.invoke).toHaveBeenCalledTimes(101);
  expect(mocks.invoke).toHaveBeenLastCalledWith("session_import_event_context", expect.objectContaining({
    importId, ...scope, eventId: "event-5000", before: 8, after: 8,
  }));
  expect(result.current.events).toHaveLength(17);
  expect(result.current.events[8]).toEqual(events[5000]);
  expect(onError).not.toHaveBeenCalled();
});

test("10,000 search matches stay on 20-hit IPC pages and a distant hit uses one context read", async () => {
  const { result } = renderHook(() => useImportedReader(importId, vi.fn()));
  await act(async () => {});
  act(() => result.current.setQuery("needle"));
  expect(mocks.invoke).toHaveBeenCalledTimes(1);
  await act(() => result.current.search());
  for (let page = 0; page < 5; page++) {
    if (page) await act(() => result.current.loadMoreHits());
    expect(result.current.hits).toHaveLength(20);
    expect(result.current.hits[0].eventId).toBe(`event-${page * 20}`);
    expect(mocks.invoke).toHaveBeenCalledTimes(page + 2);
    expect(mocks.invoke.mock.calls.at(-1)?.[1]).toMatchObject({
      importId, ...scope, query: "needle", limit: 20, cursor: page ? String(page * 20) : null,
    });
  }
  // The unloaded tail is addressable without fetching the other 495 hit pages.
  await act(async () => { expect(await result.current.ensureEvent("event-9999")).toBe(true); });
  expect(mocks.invoke).toHaveBeenCalledTimes(7);
  expect(result.current.events).toHaveLength(9);
  expect(result.current.events.at(-1)).toEqual(events[9999]);
  expect(result.current.scope).toEqual(scope);
});
