/** @vitest-environment jsdom */
import { act, cleanup, renderHook } from "@testing-library/react";
import { StrictMode } from "react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { useSessionImportContentStates } from "./useSessionImportContentStates";

const mocks = vi.hoisted(() => ({ preview: vi.fn(), events: vi.fn() }));
vi.mock("@/lib/api/sessionMigration", () => ({
  sessionImportPreview: mocks.preview,
  sessionImportPreviewEvents: mocks.events,
}));

type Preview = { previewId: string; adapter: unknown };

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

const seed = { candidateId: "synthetic-same-candidate", sourceRevision: "same-revision", title: "Synthetic" };
const preview = (id: string): Preview => ({
  previewId: id,
  adapter: {
    outcome: "complete", activeLeaf: `leaf-${id}`,
    graph: { events: [{}] },
    inventory: { imported: { messages: 1, toolCalls: 0, attachments: 0 } },
  },
});

beforeEach(() => {
  vi.resetAllMocks();
  mocks.events.mockImplementation(async (id: string) => ({
    events: [{ eventId: `event-${id}`, kind: "message", role: "user", text: id, selectable: true, defaultSelected: true }],
    nextCursor: null,
  }));
});
afterEach(cleanup);

test("close and reopen the same candidate rejects old preview success and its finally", async () => {
  const old = deferred<Preview>();
  const fresh = deferred<Preview>();
  mocks.preview.mockReturnValueOnce(old.promise).mockReturnValueOnce(fresh.promise);
  const { result } = renderHook(() => useSessionImportContentStates());
  let first!: Promise<void>;
  let second!: Promise<void>;
  act(() => { first = result.current.open(seed); });
  act(() => { result.current.closeWizard(); });
  act(() => { second = result.current.open(seed); });
  await act(async () => { old.resolve(preview("obsolete-preview")); await first; });
  const intermediate = result.current.byCandidate[seed.candidateId];
  const intermediateEvents = intermediate.selection.events.map((event) => event.eventId);
  const intermediateActive = result.current.activePreviewId;
  await act(async () => { fresh.resolve(preview("current-preview")); await second; });
  expect(intermediate.previewId).toBeNull();
  expect(intermediateEvents).toEqual([]);
  expect(intermediate.busy).toBe(true);
  expect(intermediateActive).toBeNull();
  expect(result.current.activePreviewId).toBe("current-preview");
  expect(result.current.active?.selection.events.map((event) => event.eventId)).toEqual(["event-current-preview"]);
  expect(mocks.events).toHaveBeenCalledTimes(1);
});

test("close and reopen the same candidate rejects old preview error and its finally", async () => {
  const old = deferred<Preview>();
  const fresh = deferred<Preview>();
  mocks.preview.mockReturnValueOnce(old.promise).mockReturnValueOnce(fresh.promise);
  const onError = vi.fn();
  const { result } = renderHook(() => useSessionImportContentStates(onError));
  let first!: Promise<void>;
  let second!: Promise<void>;
  act(() => { first = result.current.open(seed); });
  act(() => { result.current.closeWizard(); });
  act(() => { second = result.current.open(seed); });
  await act(async () => { old.reject(new Error("obsolete-preview-error")); await first; });
  const intermediate = result.current.byCandidate[seed.candidateId];
  await act(async () => { fresh.resolve(preview("current-preview")); await second; });
  expect(intermediate.error).toBeNull();
  expect(intermediate.busy).toBe(true);
  expect(onError).not.toHaveBeenCalled();
  expect(result.current.activePreviewId).toBe("current-preview");
});

test("new scan generation with the same candidate and revision rejects old preview ownership", async () => {
  const old = deferred<Preview>();
  const fresh = deferred<Preview>();
  mocks.preview.mockReturnValueOnce(old.promise).mockReturnValueOnce(fresh.promise);
  const { result } = renderHook(() => useSessionImportContentStates());
  let first!: Promise<void>;
  let second!: Promise<void>;
  act(() => { first = result.current.open(seed); });
  act(() => { result.current.ensureSelected([seed], 99); });
  act(() => { second = result.current.open(seed); });
  await act(async () => { old.resolve(preview("old-scan-preview")); await first; });
  const intermediate = result.current.byCandidate[seed.candidateId];
  await act(async () => { fresh.resolve(preview("new-scan-preview")); await second; });
  expect(intermediate.scanGeneration).toBe(99);
  expect(intermediate.previewId).toBeNull();
  expect(intermediate.busy).toBe(true);
  expect(result.current.activePreviewId).toBe("new-scan-preview");
  expect(mocks.events).toHaveBeenCalledTimes(1);
});

test("unmount rejects a pending preview before reading its events", async () => {
  const pending = deferred<Preview>();
  mocks.preview.mockReturnValueOnce(pending.promise);
  const { result, unmount } = renderHook(() => useSessionImportContentStates());
  let request!: Promise<void>;
  act(() => { request = result.current.open(seed); });
  unmount();
  await act(async () => { pending.resolve(preview("unmounted")); await request; });
  expect(mocks.events).not.toHaveBeenCalled();
});

test("unmount rejects a pending preview error without calling the former error callback", async () => {
  const pending = deferred<Preview>();
  mocks.preview.mockReturnValueOnce(pending.promise);
  const onError = vi.fn();
  const { result, unmount } = renderHook(() => useSessionImportContentStates(onError));
  let request!: Promise<void>;
  act(() => { request = result.current.open(seed); });
  unmount();
  await act(async () => { pending.reject(new Error("unmounted-error")); await request; });
  expect(onError).not.toHaveBeenCalled();
});

test.each(["success", "error"] as const)("invalidating A then B then A rejects obsolete preview %s", async (outcome) => {
  const old = deferred<Preview>();
  const fresh = deferred<Preview>();
  mocks.preview.mockReturnValueOnce(old.promise).mockResolvedValueOnce(preview("b"))
    .mockReturnValueOnce(fresh.promise);
  const onError = vi.fn();
  const { result } = renderHook(() => useSessionImportContentStates(onError));
  let first!: Promise<void>;
  let second!: Promise<void>;
  act(() => { first = result.current.open(seed); });
  act(() => { result.current.invalidateCandidate(seed.candidateId); });
  await act(async () => {
    await result.current.open({ ...seed, candidateId: "b" });
  });
  const retainedB = result.current.byCandidate.b;
  act(() => { second = result.current.open(seed); });
  await act(async () => {
    if (outcome === "success") old.resolve(preview("obsolete"));
    else old.reject(new Error("obsolete-error"));
    await first;
  });
  const intermediate = result.current.byCandidate[seed.candidateId];
  await act(async () => { fresh.resolve(preview("fresh")); await second; });
  expect(intermediate.previewId).toBeNull();
  expect(intermediate.busy).toBe(true);
  expect(intermediate.error).toBeNull();
  expect(result.current.byCandidate.b).toBe(retainedB);
  expect(result.current.activePreviewId).toBe("fresh");
  expect(onError).not.toHaveBeenCalled();
  expect(mocks.events).toHaveBeenCalledTimes(2);
});

test("single-candidate invalidation preserves another candidate's active dirty selection and error", async () => {
  mocks.preview.mockImplementation(async (id: string) => preview(id));
  const { result } = renderHook(() => useSessionImportContentStates());
  await act(async () => {
    await result.current.open(seed);
    await result.current.open({ ...seed, candidateId: "b" });
  });
  act(() => {
    result.current.dispatchSelection("b", { type: "toggleEvent", eventId: "event-b" });
    result.current.dispatchSelection("b", { type: "requestLeaf", leaf: "other-b" });
  });
  mocks.events.mockRejectedValueOnce(new Error("retained-b-error"));
  await act(async () => { await result.current.confirmLeaf("b"); });
  const retainedB = result.current.byCandidate.b;
  act(() => { result.current.invalidateCandidate(seed.candidateId); });
  expect(result.current.byCandidate[seed.candidateId]).toBeUndefined();
  expect(result.current.slotsByPreviewId[seed.candidateId]).toBeUndefined();
  expect(result.current.pendingByCandidate[seed.candidateId]).toBeUndefined();
  expect(result.current.byCandidate.b).toBe(retainedB);
  expect(result.current.active).toBe(retainedB);
  expect(retainedB.dirty).toBe(true);
  expect(retainedB.error).toContain("retained-b-error");
  act(() => { result.current.invalidateCandidate("b"); });
  expect(result.current.activeCandidateId).toBeNull();
  expect(result.current.activePreviewId).toBeNull();
  expect(result.current.active).toBeNull();
});

test("retained background preview cannot restore active content after active-candidate invalidation", async () => {
  const background = deferred<Preview>();
  mocks.preview.mockReturnValueOnce(background.promise).mockResolvedValueOnce(preview("a"));
  const { result } = renderHook(() => useSessionImportContentStates());
  let first!: Promise<void>;
  act(() => { first = result.current.open({ ...seed, candidateId: "b" }); });
  await act(async () => { await result.current.open(seed); });
  act(() => { result.current.invalidateCandidate(seed.candidateId); });
  await act(async () => { background.resolve(preview("b")); await first; });
  expect(result.current.byCandidate.b.previewId).toBe("b");
  expect(result.current.activeCandidateId).toBeNull();
  expect(result.current.activePreviewId).toBeNull();
  expect(result.current.active).toBeNull();
});

type EventPage = {
  events: { eventId: string; kind: string; role: string; text: string; selectable: boolean; defaultSelected: boolean }[];
  nextCursor: string | null;
};

function eventPage(id: string): EventPage {
  return {
    events: [{ eventId: id, kind: "message", role: "user", text: id, selectable: true, defaultSelected: true }],
    nextCursor: "next-page",
  };
}

const operationCases = (["page", "branch"] as const).flatMap((operation) =>
  (["success", "error"] as const).flatMap((outcome) =>
    (["invalidate", "close", "unmount"] as const).map((exit) => ({ operation, outcome, exit })),
  ),
);

test.each(operationCases)("late $operation $outcome after $exit cannot change current content or clear its busy owner", async ({ operation, outcome, exit }) => {
  mocks.preview.mockResolvedValue(preview("same-preview"));
  mocks.events.mockResolvedValue(eventPage("initial"));
  const old = deferred<EventPage>();
  const fresh = deferred<EventPage>();
  const onError = vi.fn();
  const { result, unmount } = renderHook(() => useSessionImportContentStates(onError));
  await act(async () => { await result.current.open(seed); });
  const startOperation = () => {
    if (operation === "page") return result.current.loadMore(seed.candidateId);
    result.current.dispatchSelection(seed.candidateId, { type: "requestLeaf", leaf: "target-leaf" });
    return result.current.confirmLeaf(seed.candidateId);
  };
  mocks.events.mockReturnValueOnce(old.promise);
  let first!: Promise<void>;
  let second: Promise<void> | undefined;
  act(() => { first = startOperation(); });
  const oldRequestSeq = result.current.byCandidate[seed.candidateId].requestSeq;
  if (exit === "unmount") unmount();
  else {
    act(() => {
      if (exit === "close") result.current.closeWizard();
      else result.current.invalidateCandidate(seed.candidateId);
    });
    await act(async () => { await result.current.open(seed); });
    mocks.events.mockReturnValueOnce(fresh.promise);
    act(() => { second = startOperation(); });
  }
  await act(async () => {
    if (outcome === "success") old.resolve(eventPage("obsolete-event"));
    else old.reject(new Error("obsolete-operation-error"));
    await first;
  });
  const intermediate = result.current.byCandidate[seed.candidateId];
  if (second) await act(async () => { fresh.resolve(eventPage("fresh-event")); await second; });
  expect(onError).not.toHaveBeenCalled();
  expect(intermediate.error).toBeNull();
  expect(intermediate.busy).toBe(true);
  expect(intermediate.selection.events.map((event) => event.eventId)).toEqual(["initial"]);
  if (exit !== "unmount") {
    expect(intermediate.requestSeq).toBeGreaterThan(oldRequestSeq);
    expect(result.current.active?.busy).toBe(false);
    expect(result.current.active?.selection.events.map((event) => event.eventId)).toContain("fresh-event");
    expect(result.current.active?.selection.events.map((event) => event.eventId)).not.toContain("obsolete-event");
    expect(result.current.active?.error).toBeNull();
  }
});

test.each(["success", "error"] as const)("unmount rejects first-events %s after preview was already adopted", async (outcome) => {
  const pending = deferred<EventPage>();
  mocks.preview.mockResolvedValue(preview("preview-before-unmount"));
  mocks.events.mockReturnValueOnce(pending.promise);
  const onError = vi.fn();
  const { result, unmount } = renderHook(() => useSessionImportContentStates(onError));
  let request!: Promise<void>;
  await act(async () => { request = result.current.open(seed); });
  expect(result.current.activePreviewId).toBe("preview-before-unmount");
  const before = result.current;
  unmount();
  await act(async () => {
    if (outcome === "success") pending.resolve(eventPage("after-unmount"));
    else pending.reject(new Error("first-events-after-unmount"));
    await request;
  });
  expect(result.current).toBe(before);
  expect(onError).not.toHaveBeenCalled();
  expect(mocks.events).toHaveBeenCalledTimes(1);
});

test("StrictMode effect replay leaves new content operations usable", async () => {
  mocks.preview.mockResolvedValue(preview("strict-preview"));
  const { result } = renderHook(() => useSessionImportContentStates(), { wrapper: StrictMode });
  await act(async () => { await result.current.open(seed); });
  expect(result.current.activePreviewId).toBe("strict-preview");
  expect(result.current.active?.busy).toBe(false);
  expect(mocks.preview).toHaveBeenCalledTimes(1);
  expect(mocks.events).toHaveBeenCalledTimes(1);
});
