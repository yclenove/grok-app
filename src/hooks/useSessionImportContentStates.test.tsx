/** @vitest-environment jsdom */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { useSessionImportContentStates } from "./useSessionImportContentStates";
import {
  isCurrentRequest,
  emptyPreviewSlot,
  beginRequest,
  beginRevisionReload,
} from "@/lib/sessionImport/contentStates";

const mocks = vi.hoisted(() => ({ preview: vi.fn(), events: vi.fn() }));
vi.mock("@/lib/api/sessionMigration", () => ({
  sessionImportPreview: mocks.preview,
  sessionImportPreviewEvents: mocks.events,
}));

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (err: unknown) => void;
  const promise = new Promise<T>((ok, fail) => {
    resolve = ok;
    reject = fail;
  });
  return { promise, resolve, reject };
}

beforeEach(() => {
  vi.resetAllMocks();
  mocks.preview.mockImplementation(async (id: string) => ({
    previewId: `preview-${id}`,
    adapter: {
      outcome: "complete",
      activeLeaf: `leaf-${id}`,
      graph: { events: [{}, {}], cwd: "/x" },
      inventory: { imported: { messages: 2, toolCalls: 0, attachments: 0 } },
    },
  }));
  mocks.events.mockImplementation(async (previewId: string, cursor: string | null, _limit: number, leaf?: string | null) => ({
    events: [
      {
        eventId: `${previewId}:${leaf ?? "default"}:${cursor ?? "0"}`,
        kind: "message",
        role: "user",
        text: `body-${previewId}-${leaf ?? "d"}-${cursor ?? "0"}`,
        selectable: true,
        defaultSelected: true,
      },
    ],
    nextCursor: cursor ? null : "c1",
  }));
});
afterEach(() => cleanup());

test("owner helper rejects a stale generation or sequence", () => {
  const slot = emptyPreviewSlot("a", "rev", "A");
  const { slot: busy, owner } = beginRequest(slot);
  expect(isCurrentRequest(busy, owner)).toBe(true);
  expect(isCurrentRequest({ ...busy, generation: busy.generation + 1 }, owner)).toBe(false);
  expect(isCurrentRequest({ ...busy, requestSeq: owner.requestSeq + 1 }, owner)).toBe(false);
  expect(isCurrentRequest(undefined, owner)).toBe(false);
});

test("A then B then A restores branch, events, dirty selection and cursor", async () => {
  const { result } = renderHook(() => useSessionImportContentStates());
  await act(async () => {
    await result.current.open({ candidateId: "a", sourceRevision: "ra", title: "A" });
  });
  act(() => {
    result.current.dispatchSelection("a", { type: "toggleEvent", eventId: "preview-a:leaf-a:0" });
  });
  expect(result.current.byCandidate.a.dirty).toBe(true);
  const aEvents = result.current.byCandidate.a.selection.events.map((e) => e.eventId);
  const aCursor = result.current.byCandidate.a.selection.nextCursor;
  await act(async () => {
    await result.current.open({ candidateId: "b", sourceRevision: "rb", title: "B" });
  });
  expect(result.current.activeCandidateId).toBe("b");
  expect(result.current.byCandidate.b.selection.leaf).toBe("leaf-b");
  await act(async () => {
    await result.current.open({ candidateId: "a", sourceRevision: "ra", title: "A" });
  });
  expect(result.current.activeCandidateId).toBe("a");
  expect(result.current.active?.selection.events.map((e) => e.eventId)).toEqual(aEvents);
  expect(result.current.active?.selection.nextCursor).toBe(aCursor);
  expect(result.current.active?.dirty).toBe(true);
  expect(result.current.active?.selection.autoSelectPageDefaults).toBe(false);
});

test("late preview success after switching candidate is dropped", async () => {
  const pending = deferred<{ previewId: string; adapter: unknown }>();
  mocks.preview.mockReturnValueOnce(pending.promise);
  const { result } = renderHook(() => useSessionImportContentStates());
  let first!: Promise<void>;
  act(() => {
    first = result.current.open({ candidateId: "old", sourceRevision: "r1", title: "Old" });
  });
  await act(async () => {
    await result.current.open({ candidateId: "new", sourceRevision: "r2", title: "New" });
  });
  await act(async () => {
    pending.resolve({
      previewId: "stale-preview",
      adapter: { outcome: "complete", activeLeaf: "stale-leaf", graph: { events: [{}] } },
    });
    await first;
  });
  expect(result.current.activeCandidateId).toBe("new");
  expect(result.current.activePreviewId).toBe("preview-new");
  expect(result.current.active?.candidateId).toBe("new");
  expect(result.current.byCandidate.new.previewId).toBe("preview-new");
  expect(result.current.byCandidate.new.previewId).not.toBe("stale-preview");
  expect(result.current.byCandidate.new.selection.leaf).toBe("leaf-new");
  expect(result.current.byCandidate.old?.previewId).toBe("stale-preview");
});

test("r1 then in-flight r2 then r3: r2 success cannot land", async () => {
  const r1 = deferred<{ previewId: string; adapter: unknown }>();
  const r2 = deferred<{ previewId: string; adapter: unknown }>();
  mocks.preview.mockReturnValueOnce(r1.promise);
  mocks.preview.mockReturnValueOnce(r2.promise);
  const { result } = renderHook(() => useSessionImportContentStates());
  let first!: Promise<void>;
  let second!: Promise<void>;
  act(() => {
    first = result.current.open({ candidateId: "a", sourceRevision: "r1", title: "A" });
  });
  const afterR1 = beginRevisionReload(
    result.current.byCandidate.a,
    "r2",
    "A",
  );
  expect(afterR1.owner.generation).toBe(result.current.byCandidate.a.generation + 1);
  expect(afterR1.owner.requestSeq).toBe(result.current.byCandidate.a.requestSeq + 1);
  act(() => {
    second = result.current.open({ candidateId: "a", sourceRevision: "r2", title: "A" });
  });
  await act(async () => {
    await result.current.open({ candidateId: "a", sourceRevision: "r3", title: "A" });
  });
  await act(async () => {
    r2.resolve({
      previewId: "preview-r2",
      adapter: { outcome: "complete", activeLeaf: "leaf-r2", graph: { events: [{}] } },
    });
    await second;
  });
  expect(result.current.byCandidate.a.previewId).toBe("preview-a");
  expect(result.current.byCandidate.a.sourceRevision).toBe("r3");
  expect(result.current.byCandidate.a.previewId).not.toBe("preview-r2");
  await act(async () => {
    r1.resolve({
      previewId: "preview-r1",
      adapter: { outcome: "complete", activeLeaf: "leaf-r1", graph: { events: [{}] } },
    });
    await first;
  });
  expect(result.current.byCandidate.a.previewId).toBe("preview-a");
  expect(result.current.byCandidate.a.sourceRevision).toBe("r3");
});

test("a later load of the same candidate drops the previous in-flight success", async () => {
  const pending = deferred<{ previewId: string; adapter: unknown }>();
  mocks.preview.mockReturnValueOnce(pending.promise);
  const { result } = renderHook(() => useSessionImportContentStates());
  let first!: Promise<void>;
  act(() => {
    first = result.current.open({ candidateId: "a", sourceRevision: "r1", title: "A" });
  });
  await act(async () => {
    await result.current.open({ candidateId: "a", sourceRevision: "r2", title: "A" });
  });
  await act(async () => {
    pending.resolve({
      previewId: "stale-preview",
      adapter: { outcome: "complete", activeLeaf: "stale-leaf", graph: { events: [{}] } },
    });
    await first;
  });
  expect(result.current.byCandidate.a.previewId).toBe("preview-a");
  expect(result.current.byCandidate.a.sourceRevision).toBe("r2");
  expect(result.current.byCandidate.a.previewId).not.toBe("stale-preview");
});

test("source-revision reload adopts a fresh preview UUID and drops the old slot", async () => {
  mocks.preview.mockImplementationOnce(async () => ({
    previewId: "11111111-1111-4111-8111-111111111111",
    adapter: { outcome: "complete", activeLeaf: "leaf-1", graph: { events: [{}] } },
  }));
  mocks.events.mockImplementationOnce(async (previewId: string) => ({
    events: [
      {
        eventId: `${previewId}:e0`,
        kind: "message",
        role: "user",
        text: "rev-1-body",
        selectable: true,
        defaultSelected: true,
      },
    ],
    nextCursor: "c-old",
  }));
  const { result } = renderHook(() => useSessionImportContentStates());
  await act(async () => {
    await result.current.open({ candidateId: "a", sourceRevision: "rev-1", title: "A" });
  });
  expect(result.current.slotsByPreviewId["11111111-1111-4111-8111-111111111111"]?.candidateId).toBe(
    "a",
  );
  expect(result.current.active?.selection.events.map((e) => e.eventId)).toEqual([
    "11111111-1111-4111-8111-111111111111:e0",
  ]);

  mocks.preview.mockImplementationOnce(async () => ({
    previewId: "22222222-2222-4222-8222-222222222222",
    adapter: { outcome: "complete", activeLeaf: "leaf-2", graph: { events: [{}] } },
  }));
  mocks.events.mockImplementationOnce(async (previewId: string) => ({
    events: [
      {
        eventId: `${previewId}:e0`,
        kind: "message",
        role: "user",
        text: "rev-2-body",
        selectable: true,
        defaultSelected: true,
      },
    ],
    nextCursor: "c-new",
  }));
  await act(async () => {
    await result.current.open({ candidateId: "a", sourceRevision: "rev-2", title: "A" });
  });
  expect(result.current.slotsByPreviewId["11111111-1111-4111-8111-111111111111"]).toBeUndefined();
  expect(result.current.slotsByPreviewId["22222222-2222-4222-8222-222222222222"]?.candidateId).toBe(
    "a",
  );
  expect(
    Object.values(result.current.slotsByPreviewId).filter((slot) => slot.candidateId === "a"),
  ).toHaveLength(1);
  expect(result.current.activePreviewId).toBe("22222222-2222-4222-8222-222222222222");
  expect(result.current.active?.busy).toBe(false);
  expect(result.current.active?.busyOperation).toBe("idle");
  expect(result.current.active?.sourceRevision).toBe("rev-2");
  expect(result.current.active?.selection.leaf).toBe("leaf-2");
  expect(result.current.active?.selection.events.map((e) => e.eventId)).toEqual([
    "22222222-2222-4222-8222-222222222222:e0",
  ]);
  expect(result.current.active?.selection.nextCursor).toBe("c-new");
});

test("ensureSelected records ownership without starting preview fanout", async () => {
  const { result } = renderHook(() => useSessionImportContentStates());
  await act(async () => {
    result.current.ensureSelected([
      { candidateId: "a", sourceRevision: "ra", title: "A" },
      { candidateId: "b", sourceRevision: "rb", title: "B" },
    ]);
  });
  expect(mocks.preview).not.toHaveBeenCalled();
  expect(result.current.byCandidate).toEqual({});
});

test("branch confirm loads the new leaf with a new cursor, not the old page cursor", async () => {
  const { result } = renderHook(() => useSessionImportContentStates());
  await act(async () => {
    await result.current.open({ candidateId: "a", sourceRevision: "ra", title: "A" });
  });
  act(() => {
    result.current.dispatchSelection("a", { type: "requestLeaf", leaf: "other" });
  });
  mocks.events.mockClear();
  await act(async () => {
    await result.current.confirmLeaf("a");
  });
  expect(mocks.events).toHaveBeenCalledWith("preview-a", null, 20, "other");
  expect(result.current.byCandidate.a.selection.leaf).toBe("other");
  expect(result.current.byCandidate.a.selection.nextCursor).toBe("c1");
});

test("explicit empty selection is not expanded by load-more", async () => {
  const { result } = renderHook(() => useSessionImportContentStates());
  await act(async () => {
    await result.current.open({ candidateId: "a", sourceRevision: "ra", title: "A" });
  });
  const firstId = result.current.byCandidate.a.selection.events[0]?.eventId;
  expect(firstId).toBeTruthy();
  act(() => {
    result.current.dispatchSelection("a", {
      type: "toggleEvent",
      eventId: firstId!,
    });
  });
  expect(result.current.byCandidate.a.selection.selectedEventIds).toEqual([]);
  await act(async () => {
    await result.current.loadMore("a");
  });
  expect(result.current.byCandidate.a.selection.selectedEventIds).toEqual([]);
  expect(result.current.byCandidate.a.selection.events.length).toBe(2);
});

test("failed load-more resets busy and keeps a copyable error", async () => {
  const { result } = renderHook(() => useSessionImportContentStates());
  await act(async () => {
    await result.current.open({ candidateId: "a", sourceRevision: "ra", title: "A" });
  });
  mocks.events.mockRejectedValueOnce(new Error("page-failed"));
  await act(async () => {
    await result.current.loadMore("a");
  });
  expect(result.current.byCandidate.a.busy).toBe(false);
  expect(result.current.byCandidate.a.error).toContain("page-failed");
});

test("previewId becomes the slot key after the first preview returns", async () => {
  const { result } = renderHook(() => useSessionImportContentStates());
  await act(async () => {
    await result.current.open({ candidateId: "a", sourceRevision: "ra", title: "A" });
  });
  expect(result.current.pendingByCandidate.a).toBeUndefined();
  expect(result.current.slotsByPreviewId["preview-a"]?.candidateId).toBe("a");
  expect(result.current.activePreviewId).toBe("preview-a");
  expect(result.current.active?.previewId).toBe("preview-a");
});

test("double open while preview is busy invokes preview IPC once", async () => {
  const pending = deferred<{ previewId: string; adapter: unknown }>();
  mocks.preview.mockReset();
  mocks.preview.mockReturnValueOnce(pending.promise);
  const { result } = renderHook(() => useSessionImportContentStates());
  act(() => {
    void result.current.open({ candidateId: "a", sourceRevision: "ra", title: "A" });
  });
  act(() => {
    void result.current.open({ candidateId: "a", sourceRevision: "ra", title: "A" });
  });
  expect(mocks.preview).toHaveBeenCalledTimes(1);
  await act(async () => {
    pending.resolve({
      previewId: "preview-a",
      adapter: { outcome: "complete", activeLeaf: "leaf-a", graph: { events: [{}] } },
    });
  });
});

test("double confirmLeaf while branch is busy invokes events IPC once", async () => {
  const { result } = renderHook(() => useSessionImportContentStates());
  await act(async () => {
    await result.current.open({ candidateId: "a", sourceRevision: "ra", title: "A" });
  });
  act(() => {
    result.current.dispatchSelection("a", { type: "requestLeaf", leaf: "other" });
  });
  const pending = deferred<{ events: unknown[]; nextCursor: string | null }>();
  mocks.events.mockClear();
  mocks.events.mockReturnValueOnce(pending.promise);
  act(() => {
    void result.current.confirmLeaf("a");
  });
  act(() => {
    void result.current.confirmLeaf("a");
  });
  expect(mocks.events).toHaveBeenCalledTimes(1);
  await act(async () => {
    pending.resolve({ events: [], nextCursor: null });
  });
});

test("double loadMore does not send a second IPC while page is busy", async () => {
  const pending = deferred<{ events: unknown[]; nextCursor: string | null }>();
  const { result } = renderHook(() => useSessionImportContentStates());
  await act(async () => {
    await result.current.open({ candidateId: "a", sourceRevision: "ra", title: "A" });
  });
  mocks.events.mockClear();
  mocks.events.mockReturnValueOnce(pending.promise);
  act(() => {
    void result.current.loadMore("a");
  });
  act(() => {
    void result.current.loadMore("a");
  });
  expect(mocks.events).toHaveBeenCalledTimes(1);
  await act(async () => {
    pending.resolve({ events: [{ eventId: "p2", kind: "message", role: "user", text: "p2", selectable: true, defaultSelected: true }], nextCursor: null });
  });
});

test("late page success after leaf change is dropped by owner fence", async () => {
  const pending = deferred<{ events: unknown[]; nextCursor: string | null }>();
  const { result } = renderHook(() => useSessionImportContentStates());
  await act(async () => {
    await result.current.open({ candidateId: "a", sourceRevision: "ra", title: "A" });
  });
  mocks.events.mockClear();
  mocks.events.mockReturnValueOnce(pending.promise);
  act(() => {
    void result.current.loadMore("a");
  });
  act(() => {
    result.current.dispatchSelection("a", { type: "requestLeaf", leaf: "other" });
  });
  await act(async () => {
    await result.current.confirmLeaf("a");
  });
  const leafAfter = result.current.byCandidate.a.selection.leaf;
  await act(async () => {
    pending.resolve({
      events: [{ eventId: "stale-page", kind: "message", role: "user", text: "stale", selectable: true, defaultSelected: true }],
      nextCursor: "stale",
    });
  });
  expect(result.current.byCandidate.a.selection.leaf).toBe(leafAfter);
  expect(result.current.byCandidate.a.selection.events.map((e) => e.eventId)).not.toContain("stale-page");
});

test("closeWizard releases unreclaimable slots", async () => {
  const { result } = renderHook(() => useSessionImportContentStates());
  await act(async () => {
    await result.current.open({ candidateId: "a", sourceRevision: "ra", title: "A" });
  });
  expect(Object.keys(result.current.slotsByPreviewId).length).toBe(1);
  act(() => {
    result.current.closeWizard();
  });
  expect(result.current.slotsByPreviewId).toEqual({});
  expect(result.current.pendingByCandidate).toEqual({});
  expect(result.current.active).toBeNull();
});

test("first preview lives only in pendingByCandidate until previewId exists", async () => {
  const pending = deferred<{ previewId: string; adapter: unknown }>();
  mocks.preview.mockReturnValueOnce(pending.promise);
  const { result } = renderHook(() => useSessionImportContentStates());
  act(() => {
    void result.current.open({ candidateId: "a", sourceRevision: "ra", title: "A" });
  });
  expect(result.current.pendingByCandidate.a?.previewId).toBeNull();
  expect(result.current.pendingByCandidate.a?.busyOperation).toBe("preview");
  expect(result.current.slotsByPreviewId).toEqual({});
  expect(result.current.activePreviewId).toBeNull();
  expect(result.current.active?.candidateId).toBe("a");
  await act(async () => {
    pending.resolve({
      previewId: "preview-a",
      adapter: { outcome: "complete", activeLeaf: "leaf-a", graph: { events: [{}] } },
    });
  });
  expect(result.current.pendingByCandidate.a).toBeUndefined();
  expect(result.current.slotsByPreviewId["preview-a"]?.candidateId).toBe("a");
  expect(result.current.activePreviewId).toBe("preview-a");
  expect(result.current.active?.filterKey).toBe("leaf:leaf-a");
  expect(result.current.active?.selection.leaves).toEqual(["leaf-a"]);
  expect(result.current.active?.selection.events.length).toBeGreaterThan(0);
  expect(result.current.active?.selection.nextCursor).toBe("c1");
  expect(result.current.active?.generation).toBeGreaterThanOrEqual(1);
  expect(result.current.active?.requestSeq).toBeGreaterThan(0);
  expect(result.current.active?.busyOperation).toBe("idle");
});

test("reselecting the same source revision does not mark stale or re-issue preview IPC", async () => {
  const { result } = renderHook(() => useSessionImportContentStates());
  await act(async () => {
    await result.current.open({ candidateId: "a", sourceRevision: "ra", title: "A" });
  });
  mocks.preview.mockClear();
  mocks.events.mockClear();
  await act(async () => {
    await result.current.open({ candidateId: "a", sourceRevision: "ra", title: "A" });
  });
  expect(mocks.preview).not.toHaveBeenCalled();
  expect(mocks.events).not.toHaveBeenCalled();
  expect(result.current.activePreviewId).toBe("preview-a");
});

test("scan generation change releases old preview slots without starting new work", async () => {
  const { result } = renderHook(() => useSessionImportContentStates());
  await act(async () => {
    await result.current.open({ candidateId: "a", sourceRevision: "ra", title: "A" });
  });
  act(() => {
    result.current.ensureSelected(
      [{ candidateId: "a", sourceRevision: "ra", title: "A" }],
      9,
    );
  });
  expect(result.current.slotsByPreviewId).toEqual({});
  expect(result.current.pendingByCandidate).toEqual({});
  expect(mocks.preview).toHaveBeenCalledTimes(1);
  expect(result.current.slotsByPreviewId["preview-a"]).toBeUndefined();
});

test("A then B then A restores a copyable error on A", async () => {
  const { result } = renderHook(() => useSessionImportContentStates());
  await act(async () => {
    await result.current.open({ candidateId: "a", sourceRevision: "ra", title: "A" });
  });
  mocks.events.mockRejectedValueOnce(new Error("page-failed"));
  await act(async () => {
    await result.current.loadMore("a");
  });
  expect(result.current.byCandidate.a.error).toContain("page-failed");
  await act(async () => {
    await result.current.open({ candidateId: "b", sourceRevision: "rb", title: "B" });
  });
  expect(result.current.active?.candidateId).toBe("b");
  expect(result.current.active?.error).toBeNull();
  await act(async () => {
    await result.current.open({ candidateId: "a", sourceRevision: "ra", title: "A" });
  });
  expect(result.current.active?.candidateId).toBe("a");
  expect(result.current.active?.error).toContain("page-failed");
  expect(result.current.active?.dirty).toBe(false);
});

test("late preview error after switching candidate does not land on the active slot", async () => {
  const pending = deferred<{ previewId: string; adapter: unknown }>();
  mocks.preview.mockReturnValueOnce(pending.promise);
  const { result } = renderHook(() => useSessionImportContentStates());
  let first!: Promise<void>;
  act(() => {
    first = result.current.open({ candidateId: "old", sourceRevision: "r1", title: "Old" });
  });
  await act(async () => {
    await result.current.open({ candidateId: "new", sourceRevision: "r2", title: "New" });
  });
  await act(async () => {
    pending.reject(new Error("late-fail"));
    await first.catch(() => undefined);
  });
  expect(result.current.activeCandidateId).toBe("new");
  expect(result.current.activePreviewId).toBe("preview-new");
  expect(result.current.active?.error).toBeNull();
  expect(result.current.byCandidate.old?.error).toContain("late-fail");
});

test("switching to a pending candidate clears the previous previewId active key", async () => {
  const { result } = renderHook(() => useSessionImportContentStates());
  await act(async () => {
    await result.current.open({ candidateId: "a", sourceRevision: "ra", title: "A" });
  });
  const pending = deferred<{ previewId: string; adapter: unknown }>();
  mocks.preview.mockReturnValueOnce(pending.promise);
  act(() => {
    void result.current.open({ candidateId: "b", sourceRevision: "rb", title: "B" });
  });
  expect(result.current.activeCandidateId).toBe("b");
  expect(result.current.activePreviewId).toBeNull();
  expect(result.current.active?.candidateId).toBe("b");
  expect(result.current.active?.previewId).toBeNull();
  expect(result.current.pendingByCandidate.b).toBeTruthy();
  await act(async () => {
    pending.resolve({
      previewId: "preview-b",
      adapter: { outcome: "complete", activeLeaf: "leaf-b", graph: { events: [{}] } },
    });
  });
  expect(result.current.activePreviewId).toBe("preview-b");
});
