/** @vitest-environment jsdom */
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { useSessionImportAssociations, type AssociationClient } from "./useSessionImportAssociations";
import type { CatalogEntry, PrepareBatchItem, PrepareBatchStatus } from "@/lib/api/sessionMigration";
import type { PreparedAssociationRequest, PreparedAssociationReview } from "@/lib/api/sessionMigrationIdentityAssociation";

afterEach(cleanup);
const batch: PrepareBatchStatus = {
  version: 1, batchId: "batch", scanId: "scan", epoch: 1, selectionId: "selection", selectionStateVersion: 1,
  state: "completed", stateVersion: 7, total: 2, prepared: 2, failed: 0, skipped: 0, active: 0,
  createdAt: "2026-10-15", updatedAt: "2026-10-15", cancelRequested: false,
};
const member: PrepareBatchItem = { candidateId: "candidate", provider: "cursor", title: "Source",
  sourceRevision: "source-rev", state: "prepared", attempt: 1, previewId: "preview" };
const target: CatalogEntry = { importId: "imp:target", installationId: "installation", provider: "cursor",
  nativeSessionId: "old", title: "Target", hidden: false, currentRevision: "revision", createdAt: "2026-10-15" };
function inspected(request: PreparedAssociationRequest): PreparedAssociationReview {
  return { batchId: request.batchId, stateVersion: request.expectedStateVersion, candidateId: request.candidateId,
    leafId: "leaf", review: { version: 1, importId: request.importId, expectedRevision: request.expectedRevision,
      previewId: request.previewId, sourceId: "installation", oldNativeSessionId: "old", currentNativeSessionId: "new",
      oldNormalizerVersion: "1.0.0", currentNormalizerVersion: "1.0.6", selectionHash: "selection-hash", reviewDigest: "a".repeat(64) } };
}
function client() {
  return { entries: vi.fn<AssociationClient["entries"]>().mockResolvedValue({ items: [target], total: 30, nextCursor: "page-2" }),
    entry: vi.fn<AssociationClient["entry"]>().mockResolvedValue(target),
    inspect: vi.fn<AssociationClient["inspect"]>().mockImplementation(async (request) => inspected(request)) };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

test("explicit review uses prepared bindings, bounded catalog pages and a fresh target", async () => {
  const api = client();
  const { result } = renderHook(() => useSessionImportAssociations(batch, true, () => false, api));
  act(() => result.current.open(member));
  await waitFor(() => expect(result.current.busy).toBe(false));
  expect(api.entries).toHaveBeenCalledWith(null, 20, { provider: "cursor" });
  expect(() => result.current.snapshot()).toThrow();
  await act(async () => { await result.current.choose(target.importId); });
  expect(api.entry).toHaveBeenCalledWith(target.importId);
  expect(api.inspect).toHaveBeenCalledWith({ version: 1, batchId: "batch", expectedStateVersion: 7,
    candidateId: "candidate", previewId: "preview", importId: "imp:target", expectedRevision: "revision" });
  act(() => { expect(result.current.confirm()).toBe(false); });
  act(() => result.current.markReady("a".repeat(64), true));
  act(() => { expect(result.current.confirm()).toBe(true); });
  expect(result.current.dialog).toBeNull();
  const saved = result.current.snapshot();
  expect(saved).toEqual([{ version: 1, importId: "imp:target", expectedRevision: "revision", previewId: "preview", reviewDigest: "a".repeat(64) }]);
  saved[0].importId = "mutated";
  expect(result.current.snapshot()[0].importId).toBe(target.importId);
  act(() => result.current.clear(member.candidateId));
  expect(result.current.snapshot()).toEqual([]);
});

test.each(["close", "new-batch", "new-version", "leave"])("late inspection cannot arm a review after %s", async (transition) => {
  const api = client(); const pending = deferred<PreparedAssociationReview>();
  api.inspect.mockReturnValueOnce(pending.promise);
  const { result, rerender } = renderHook(({ status, active }) => useSessionImportAssociations(status, active, () => false, api),
    { initialProps: { status: batch, active: true } });
  act(() => result.current.open(member));
  await waitFor(() => expect(result.current.busy).toBe(false));
  let request!: Promise<void>;
  act(() => { request = result.current.choose(target.importId); });
  await waitFor(() => expect(api.inspect).toHaveBeenCalledOnce());
  if (transition === "close") act(() => result.current.close());
  else rerender({ status: { ...batch, batchId: transition === "new-batch" ? "other" : batch.batchId,
    stateVersion: transition === "new-version" ? 8 : 7 }, active: transition !== "leave" });
  await act(async () => { pending.resolve(inspected(api.inspect.mock.calls[0][0])); await request; });
  expect(result.current.dialog).toBeNull(); expect(result.current.confirmed).toEqual([]);
  expect(result.current.busy).toBe(false);
});

test("target changes, duplicate assignments and batch changes require fresh confirmation", async () => {
  const api = client();
  const { result, rerender } = renderHook(({ status }) => useSessionImportAssociations(status, true, () => false, api), { initialProps: { status: batch } });
  act(() => result.current.open(member));
  await waitFor(() => expect(result.current.busy).toBe(false));
  await act(async () => { await result.current.choose(target.importId); });
  act(() => result.current.markReady("a".repeat(64), true));
  act(() => { expect(result.current.confirm()).toBe(true); });
  act(() => result.current.open({ ...member, candidateId: "second", previewId: "second-preview" }));
  await waitFor(() => expect(result.current.busy).toBe(false));
  await act(async () => { await result.current.choose(target.importId); });
  expect(result.current.error).toContain("MAPPING_CONFLICT");
  expect(api.inspect).toHaveBeenCalledOnce();
  act(() => result.current.close());
  rerender({ status: { ...batch, stateVersion: 8 } });
  expect(result.current.snapshot()).toEqual([]);
});

test("synchronous ownership prevents duplicate inspection and locked confirmation", async () => {
  const api = client(); let locked = false;
  const pending = deferred<CatalogEntry | null>(); api.entry.mockReturnValueOnce(pending.promise);
  const { result } = renderHook(() => useSessionImportAssociations(batch, true, () => locked, api));
  act(() => result.current.open(member));
  await waitFor(() => expect(result.current.busy).toBe(false));
  let first!: Promise<void>;
  act(() => { first = result.current.choose(target.importId); void result.current.choose("other"); });
  expect(api.entry).toHaveBeenCalledOnce();
  await act(async () => { pending.resolve(target); await first; });
  act(() => result.current.markReady("a".repeat(64), true));
  locked = true;
  act(() => { expect(result.current.confirm()).toBe(false); result.current.clear(member.candidateId); });
  expect(result.current.confirmed).toEqual([]);
  locked = false;
  await act(async () => { await result.current.nextTargets(); });
  expect(api.entries).toHaveBeenLastCalledWith("page-2", 20, { provider: "cursor" });
  expect(result.current.dialog?.ready).toBe(true);
});
