/** @vitest-environment jsdom */
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { ImportJobHistoryPage } from "@/lib/api/sessionMigrationJobs";
import { useSessionImportJobHistory } from "./useSessionImportJobHistory";

const mocks = vi.hoisted(() => ({ page: vi.fn() }));
vi.mock("@/lib/api/sessionMigrationJobs", () => ({ sessionImportJobHistory: mocks.page }));
afterEach(() => { cleanup(); vi.resetAllMocks(); });
const page = (id: string, nextCursor: string | null = null): ImportJobHistoryPage => ({
  items: [{ jobId: id, state: "completed", createdAt: "2026-10-16T00:00:00Z", itemCount: 3 }], nextCursor,
});
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (cause: unknown) => void;
  const promise = new Promise<T>((ok, fail) => { resolve = ok; reject = fail; });
  return { promise, resolve, reject };
}

test("does not read until opened and keeps bounded pages with retryable navigation", async () => {
  mocks.page.mockResolvedValueOnce(page("new", "older")).mockRejectedValueOnce(new Error("SOURCE_BUSY: locked"))
    .mockResolvedValueOnce(page("old")).mockResolvedValueOnce(page("new", "older"));
  const select = vi.fn();
  const { result } = renderHook(() => useSessionImportJobHistory(true, select));
  expect(mocks.page).not.toHaveBeenCalled();
  act(() => result.current.show());
  await waitFor(() => expect(result.current.items[0]?.jobId).toBe("new"));
  await act(async () => result.current.next());
  expect(result.current.page).toBe(0);
  expect(result.current.error).toContain("SOURCE_BUSY");
  expect(result.current.items[0].jobId).toBe("new");
  await act(async () => result.current.retry());
  expect(result.current.items[0].jobId).toBe("old");
  expect(result.current.page).toBe(1);
  expect(mocks.page.mock.calls).toEqual([[null], ["older"], ["older"]]);
  await act(async () => result.current.previous());
  expect(result.current.items[0].jobId).toBe("new");
  expect(select).not.toHaveBeenCalled();
});

test("closed and older list replies cannot replace the reopened page", async () => {
  const old = deferred<ImportJobHistoryPage>();
  mocks.page.mockReturnValueOnce(old.promise).mockResolvedValueOnce(page("fresh"));
  const { result } = renderHook(() => useSessionImportJobHistory(true, vi.fn()));
  act(() => result.current.show());
  act(() => result.current.close());
  act(() => result.current.show());
  await waitFor(() => expect(result.current.items[0]?.jobId).toBe("fresh"));
  await act(async () => old.resolve(page("stale")));
  expect(result.current.items[0].jobId).toBe("fresh");
  expect(result.current.busy).toBe(false);
});

test("selection has a synchronous owner, cancel invalidates it, and errors allow another selection", async () => {
  mocks.page.mockResolvedValue(page("one"));
  const old = deferred<boolean>();
  let ownsOld!: () => boolean;
  const select = vi.fn().mockImplementationOnce((_id, owns) => { ownsOld = owns; return old.promise; })
    .mockRejectedValueOnce(new Error("SNAPSHOT_UNAVAILABLE: corrupt"))
    .mockResolvedValueOnce(true);
  const { result } = renderHook(() => useSessionImportJobHistory(true, select));
  act(() => result.current.show());
  await waitFor(() => expect(result.current.busy).toBe(false));
  let flight!: Promise<void>;
  act(() => { flight = result.current.select("one"); void result.current.select("duplicate"); });
  expect(select).toHaveBeenCalledTimes(1);
  expect(ownsOld()).toBe(true);
  act(() => result.current.close());
  expect(ownsOld()).toBe(false);
  act(() => result.current.show());
  await waitFor(() => expect(result.current.busy).toBe(false));
  await act(async () => { old.resolve(true); await flight; });
  expect(result.current.open).toBe(true);
  await act(async () => result.current.select("one"));
  expect(result.current.error).toContain("SNAPSHOT_UNAVAILABLE");
  expect(result.current.open).toBe(true);
  await act(async () => result.current.select("one"));
  expect(result.current.open).toBe(false);
});

test("new busy intent and unmount invalidate pending selections", async () => {
  mocks.page.mockResolvedValue(page("one"));
  const pending = deferred<boolean>();
  let owns!: () => boolean;
  const select = vi.fn((_id, isCurrent) => { owns = isCurrent; return pending.promise; });
  const { result, rerender, unmount } = renderHook(({ enabled }) => useSessionImportJobHistory(enabled, select), {
    initialProps: { enabled: true },
  });
  act(() => result.current.show());
  await waitFor(() => expect(result.current.busy).toBe(false));
  act(() => { void result.current.select("one"); });
  rerender({ enabled: false });
  expect(owns()).toBe(false);
  expect(result.current.open).toBe(false);
  act(() => result.current.show());
  expect(result.current.open).toBe(false);
  unmount();
  await act(async () => pending.resolve(true));
  expect(owns()).toBe(false);
});
