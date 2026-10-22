/** @vitest-environment jsdom */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { useCandidatePeek } from "./useCandidatePeek";

const mocks = vi.hoisted(() => ({ preview: vi.fn(), events: vi.fn() }));
vi.mock("@/lib/api/sessionMigration", () => ({
  sessionImportPreview: mocks.preview,
  sessionImportPreviewEvents: mocks.events,
}));

beforeEach(() => {
  vi.resetAllMocks();
  mocks.preview.mockResolvedValue({
    previewId: "p1",
    adapter: {
      outcome: "complete",
      inventory: { imported: { messages: 2, toolCalls: 1, attachments: 0 } },
      graph: { events: [{}, {}] },
    },
  });
  mocks.events.mockResolvedValue({
    events: [{ eventId: "e1", kind: "message", role: "user", text: "hello-from-source" }],
    nextCursor: null,
  });
});
afterEach(() => cleanup());

test("peek loads shipped preview then paged events", async () => {
  const onError = vi.fn();
  const { result } = renderHook(() => useCandidatePeek(onError));
  await act(async () => {
    await result.current.peek("cand-1", "rev-a", "My chat");
  });
  expect(mocks.preview).toHaveBeenCalledWith("cand-1", "rev-a");
  expect(mocks.events).toHaveBeenCalledWith("p1", null, 20);
  expect(result.current.title).toBe("My chat");
  expect(result.current.events[0].text).toBe("hello-from-source");
  expect(result.current.summary?.messages).toBe(2);
  expect(result.current.summary?.tools).toBe(1);
  expect(onError).not.toHaveBeenCalled();
});

test("a late preview page cannot replace a newer selection", async () => {
  function deferred<T>() {
    let resolve!: (value: T) => void;
    const promise = new Promise<T>((done) => {
      resolve = done;
    });
    return { promise, resolve };
  }
  const pending = deferred<{ previewId: string; adapter: unknown }>();
  mocks.preview.mockReturnValueOnce(pending.promise);
  mocks.preview.mockResolvedValueOnce({
    previewId: "p-new",
    adapter: {
      outcome: "complete",
      inventory: { imported: { messages: 1, toolCalls: 0, attachments: 0 } },
      graph: { events: [{}] },
    },
  });
  mocks.events.mockResolvedValue({
    events: [{ eventId: "new", kind: "message", role: "user", text: "newer-leaf" }],
    nextCursor: null,
  });
  const onError = vi.fn();
  const { result } = renderHook(() => useCandidatePeek(onError));
  let first!: Promise<void>;
  act(() => {
    first = result.current.peek("cand-old", "rev-a", "Old");
  });
  await act(async () => {
    await result.current.peek("cand-new", "rev-b", "New");
  });
  await act(async () => {
    pending.resolve({
      previewId: "p-old",
      adapter: {
        outcome: "complete",
        inventory: { imported: { messages: 9, toolCalls: 0, attachments: 0 } },
        graph: { events: [{}, {}] },
      },
    });
    await first;
  });
  expect(result.current.title).toBe("New");
  expect(result.current.events.map((e) => e.eventId)).toEqual(["new"]);
  expect(result.current.events[0].text).toBe("newer-leaf");
  expect(onError).not.toHaveBeenCalled();
});

test("peek load more appends the next preview page", async () => {
  mocks.events
    .mockResolvedValueOnce({
      events: [{ eventId: "e1", kind: "message", text: "a" }],
      nextCursor: "20",
    })
    .mockResolvedValueOnce({
      events: [{ eventId: "e2", kind: "message", text: "b" }],
      nextCursor: null,
    });
  const onError = vi.fn();
  const { result } = renderHook(() => useCandidatePeek(onError));
  await act(async () => {
    await result.current.peek("cand-1", "rev-a", "My chat");
  });
  expect(result.current.nextCursor).toBe("20");
  await act(async () => {
    await result.current.loadMore();
  });
  expect(mocks.events).toHaveBeenLastCalledWith("p1", "20", 20);
  expect(result.current.events.map((e) => e.eventId)).toEqual(["e1", "e2"]);
  expect(result.current.nextCursor).toBeNull();
  expect(onError).not.toHaveBeenCalled();
});
