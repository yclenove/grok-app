/** @vitest-environment jsdom */
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { ImportedRevisionPage } from "@/lib/api/sessionMigration";
import { useImportedRevisions } from "./useImportedRevisions";

afterEach(cleanup);
const revision = (n: number) => `00000000-0000-4000-8000-${String(n).padStart(12, "0")}`;
const page = (importId = "history-a", n = 2): ImportedRevisionPage => ({
  importId, currentRevision: revision(2), snapshot: "a".repeat(64),
  items: [{ revisionId: revision(n), createdAt: "2026-10-15T00:00:00Z", outcome: "complete" }],
  previousCursor: null, nextCursor: "page-2", total: 2,
});
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

test("one page at a time, duplicate next is blocked, failed next retries its original destination", async () => {
  const next = deferred<ImportedRevisionPage>();
  const client = vi.fn().mockResolvedValueOnce(page()).mockRejectedValueOnce(new Error("SOURCE_BUSY"))
    .mockReturnValueOnce(next.promise).mockResolvedValueOnce(page());
  const { result } = renderHook(() => useImportedRevisions("history-a", client));
  await waitFor(() => expect(result.current.busy).toBe(false));
  act(() => { result.current.move("next"); result.current.move("next"); });
  await waitFor(() => expect(result.current.error).toContain("SOURCE_BUSY"));
  expect(client).toHaveBeenCalledTimes(2);
  expect(result.current.page?.items[0].revisionId).toBe(revision(2));
  expect(result.current.canChoose(revision(2))).toBe(false);
  act(() => result.current.retry());
  expect(client).toHaveBeenLastCalledWith("history-a", "page-2");
  await act(async () => next.resolve({ ...page("history-a", 1), previousCursor: "page-1", nextCursor: null }));
  expect(result.current.page?.items.map((item) => item.revisionId)).toEqual([revision(1)]);
  expect(result.current.canChoose(revision(2))).toBe(false);
  act(() => result.current.move("previous"));
  await waitFor(() => expect(client).toHaveBeenLastCalledWith("history-a", "page-1"));
});

test("late A responses cannot replace B or a new A owner", async () => {
  const old = deferred<ImportedRevisionPage>();
  const client = vi.fn().mockReturnValueOnce(old.promise).mockResolvedValueOnce(page("history-b", 1)).mockResolvedValueOnce(page("history-a", 1));
  const view = renderHook(({ id }) => useImportedRevisions(id, client), { initialProps: { id: "history-a" } });
  view.rerender({ id: "history-b" });
  await waitFor(() => expect(view.result.current.page?.importId).toBe("history-b"));
  view.rerender({ id: "history-a" });
  await waitFor(() => expect(view.result.current.page?.items[0].revisionId).toBe(revision(1)));
  await act(async () => old.resolve(page("history-a", 2)));
  expect(view.result.current.page?.items[0].revisionId).toBe(revision(1));
  view.unmount();
});

test("a changed page generation is rejected until an explicit refresh", async () => {
  const changed = { ...page("history-a", 1), snapshot: "b".repeat(64) };
  const client = vi.fn().mockResolvedValueOnce(page()).mockResolvedValueOnce(changed).mockResolvedValueOnce(changed);
  const { result } = renderHook(() => useImportedRevisions("history-a", client));
  await waitFor(() => expect(result.current.busy).toBe(false));
  act(() => result.current.move("next"));
  await waitFor(() => expect(result.current.error).toContain("PLAN_STALE"));
  expect(result.current.page?.snapshot).toBe("a".repeat(64));
  act(() => result.current.refresh());
  await waitFor(() => expect(result.current.page?.snapshot).toBe("b".repeat(64)));
  expect(client).toHaveBeenLastCalledWith("history-a", null);
});

test.each(["foreign", "oversized", "duplicate", "empty"])("%s metadata is never made selectable", async (kind) => {
  const invalid = page();
  if (kind === "foreign") invalid.importId = "history-b";
  if (kind === "oversized") invalid.items = Array.from({ length: 51 }, (_, i) => ({ ...invalid.items[0], revisionId: revision(i + 1) }));
  if (kind === "duplicate") invalid.items.push(invalid.items[0]);
  if (kind === "empty") invalid.items = [];
  const client = vi.fn().mockResolvedValue(invalid);
  const { result } = renderHook(() => useImportedRevisions("history-a", client));
  await waitFor(() => expect(result.current.error).toContain("PLAN_STALE"));
  expect(result.current.page).toBeNull();
  expect(result.current.canChoose(revision(2))).toBe(false);
  expect(client).toHaveBeenCalledOnce();
});
