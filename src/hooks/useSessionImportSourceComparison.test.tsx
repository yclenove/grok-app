/** @vitest-environment jsdom */
import { StrictMode, type PropsWithChildren } from "react";
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { SourceComparisonPage, SourceComparisonRequest } from "@/lib/api/sessionMigrationComparison";
import type { ImportGroupScope } from "@/lib/sessionImport/groupSelectionState";
import { comparisonDeferred, comparisonOwner, comparisonPage, comparisonScope } from "@/test/sessionImportComparisonFixture";
import { useSessionImportSourceComparison } from "./useSessionImportSourceComparison";

afterEach(cleanup);
function fixture() {
  const page = vi.fn(async (request: Readonly<SourceComparisonRequest>) => comparisonPage(request, request.cursor ? 2 : 0));
  return { client: { page }, page };
}

test("does not read on mount; explicit open compares only the pinned pair with a single page of 50", async () => {
  const { client, page } = fixture();
  const owner = comparisonOwner();
  const { result } = renderHook(() => useSessionImportSourceComparison(comparisonScope, client));
  expect(page).not.toHaveBeenCalled();
  await act(async () => { expect(await result.current.open(owner.groupId, owner.left, owner.right)).toBe(true); });
  expect(page).toHaveBeenCalledExactlyOnceWith({ version: 1, left: owner.left, right: owner.right, limit: 50 });
  expect(result.current.page?.rows).toHaveLength(2);
  expect(result.current.page?.equivalence).toBe("unverified");
  expect(result.current.loading).toBe(false);
});

test("next/previous replace bounded rows; going back renews the signed cursor rather than accumulating rows", async () => {
  const { client, page } = fixture();
  const owner = comparisonOwner();
  const { result } = renderHook(() => useSessionImportSourceComparison(comparisonScope, client));
  await act(async () => { await result.current.open(owner.groupId, owner.left, owner.right); await result.current.nextPage(); });
  expect(result.current.pageIndex).toBe(1);
  expect(result.current.page?.rows).toHaveLength(1);
  expect(result.current.page?.rows[0].right).toBeNull();
  await act(async () => { expect(await result.current.nextPage()).toBe(false); });
  expect(page).toHaveBeenCalledTimes(2);
  page.mockResolvedValueOnce({ ...comparisonPage(owner), nextCursor: "renewed:2" });
  await act(async () => { await result.current.previousPage(); await result.current.nextPage(); });
  expect(page.mock.calls[3][0].cursor).toBe("renewed:2");
  expect(result.current.pageIndex).toBe(1);
});

test("synchronous double next/retry reserve one request and preserve the original caller pair", async () => {
  const { client, page } = fixture();
  const owner = comparisonOwner();
  const oldPair = structuredClone(owner);
  const pending = comparisonDeferred<SourceComparisonPage>();
  page.mockReturnValueOnce(pending.promise);
  const { result } = renderHook(() => useSessionImportSourceComparison(comparisonScope, client));
  let first!: Promise<boolean>;
  act(() => { first = result.current.open(owner.groupId, owner.left, owner.right); });
  owner.left.leafId = "changed-after-call";
  owner.scope.generation = 7;
  expect(await result.current.retry()).toBe(false);
  await act(async () => { pending.resolve(comparisonPage(oldPair)); await first; });
  const tail = comparisonDeferred<SourceComparisonPage>();
  page.mockReturnValueOnce(tail.promise);
  let next!: Promise<boolean>;
  act(() => { next = result.current.nextPage(); });
  expect(await result.current.nextPage()).toBe(false);
  expect(page).toHaveBeenCalledTimes(2);
  expect(page.mock.calls[1][0].left.leafId).toBe("main");
  await act(async () => { tail.resolve(comparisonPage(oldPair, 2)); await next; });
});

test("failed next keeps the accepted page and retry reuses the exact failed cursor", async () => {
  const { client, page } = fixture();
  page.mockResolvedValueOnce(comparisonPage()).mockRejectedValueOnce(new Error("PLAN_STALE: snapshot unavailable"));
  const owner = comparisonOwner();
  const { result } = renderHook(() => useSessionImportSourceComparison(comparisonScope, client));
  await act(async () => { await result.current.open(owner.groupId, owner.left, owner.right); await result.current.nextPage(); });
  expect(result.current.pageIndex).toBe(0);
  expect(result.current.page?.rows).toHaveLength(2);
  expect(result.current.error).toContain("PLAN_STALE");
  expect(await result.current.nextPage()).toBe(false);
  await act(async () => { expect(await result.current.retry()).toBe(true); });
  expect(page.mock.calls[2][0].cursor).toBe("signed-next:2");
  expect(result.current.pageIndex).toBe(1);
});

test.each(["success", "error"] as const)("late %s after close/reopen cannot adopt rows or finish the fresh request", async (outcome) => {
  const { client, page } = fixture();
  const old = comparisonDeferred<SourceComparisonPage>();
  const fresh = comparisonDeferred<SourceComparisonPage>();
  page.mockReturnValueOnce(old.promise).mockReturnValueOnce(fresh.promise);
  const owner = comparisonOwner();
  const onError = vi.fn();
  const { result } = renderHook(() => useSessionImportSourceComparison(comparisonScope, client, { onError }));
  let first!: Promise<boolean>;
  let second!: Promise<boolean>;
  act(() => { first = result.current.open(owner.groupId, owner.left, owner.right); });
  act(() => { result.current.close(); second = result.current.open(owner.groupId, owner.left, owner.right); });
  await act(async () => { if (outcome === "success") old.resolve(comparisonPage()); else old.reject(new Error("obsolete")); await first; });
  expect(await first).toBe(false);
  expect(result.current.loading).toBe(true);
  expect(result.current.page).toBeNull();
  expect(onError).not.toHaveBeenCalled();
  await act(async () => { fresh.resolve(comparisonPage()); await second; });
  expect(result.current.page).not.toBeNull();
});

test.each(["scan", "epoch", "generation", "null", "unmount"] as const)(
  "committed %s scope change invalidates a late response without needing caller close", async (change) => {
    const { client, page } = fixture();
    const pending = comparisonDeferred<SourceComparisonPage>();
    page.mockReturnValueOnce(pending.promise);
    const onError = vi.fn();
    const { result, rerender, unmount } = renderHook(({ scope }: { scope: ImportGroupScope | null }) =>
      useSessionImportSourceComparison(scope, client, { onError }), { initialProps: { scope: comparisonScope as ImportGroupScope | null } });
    const owner = comparisonOwner();
    let first!: Promise<boolean>;
    act(() => { first = result.current.open(owner.groupId, owner.left, owner.right); });
    if (change === "unmount") unmount();
    else rerender({ scope: change === "null" ? null : { ...comparisonScope,
      ...(change === "scan" ? { scanId: "different" } : change === "epoch" ? { epoch: 2 } : { generation: 2 }) } });
    await act(async () => { pending.resolve(comparisonPage()); await first; });
    expect(await first).toBe(false);
    expect(onError).not.toHaveBeenCalled();
    if (change !== "unmount") expect(result.current.owner).toBeNull();
  },
);

test("changing either branch opens a fresh pair; its late predecessor cannot clear fresh busy state", async () => {
  const { client, page } = fixture();
  const old = comparisonDeferred<SourceComparisonPage>();
  const fresh = comparisonDeferred<SourceComparisonPage>();
  page.mockReturnValueOnce(old.promise).mockReturnValueOnce(fresh.promise);
  const owner = comparisonOwner();
  const next = { ...owner, right: { ...owner.right, leafId: "alternate-leaf" } };
  const { result } = renderHook(() => useSessionImportSourceComparison(comparisonScope, client));
  let first!: Promise<boolean>;
  let second!: Promise<boolean>;
  act(() => { first = result.current.open(owner.groupId, owner.left, owner.right); });
  act(() => { second = result.current.open(next.groupId, next.left, next.right); });
  await act(async () => { old.resolve(comparisonPage(owner)); await first; });
  expect(result.current.loading).toBe(true);
  expect(result.current.owner?.right.leafId).toBe("alternate-leaf");
  await act(async () => { fresh.resolve(comparisonPage(next)); await second; });
  expect(result.current.page?.right.leafId).toBe("alternate-leaf");
});

test("wrong pair and corrupt DTO fail closed without selecting a fallback member", async () => {
  const { client, page } = fixture();
  const owner = comparisonOwner();
  const { result } = renderHook(() => useSessionImportSourceComparison(comparisonScope, client));
  await act(async () => { expect(await result.current.open(owner.groupId, owner.left, owner.left)).toBe(false); });
  expect(page).not.toHaveBeenCalled();
  page.mockResolvedValueOnce({ ...comparisonPage(), right: { ...comparisonPage().right, previewId: "foreign" } });
  await act(async () => { expect(await result.current.open(owner.groupId, owner.left, owner.right)).toBe(false); });
  expect(result.current.page).toBeNull();
  expect(result.current.error).toContain("PLAN_STALE");
  expect(page).toHaveBeenCalledTimes(1);
});

test("an error callback can open another pair without obsolete finally overwriting its busy owner", async () => {
  const { client, page } = fixture();
  const fresh = comparisonDeferred<SourceComparisonPage>();
  page.mockRejectedValueOnce(new Error("failed-old")).mockReturnValueOnce(fresh.promise);
  const owner = comparisonOwner();
  let second: Promise<boolean> | undefined;
  const onError = vi.fn(() => { second = result.current.open(owner.groupId, owner.left, owner.right); });
  const { result } = renderHook(() => useSessionImportSourceComparison(comparisonScope, client, { onError }));
  await act(async () => { await result.current.open(owner.groupId, owner.left, owner.right); });
  expect(onError).toHaveBeenCalledTimes(1);
  expect(result.current.error).toBeNull();
  expect(result.current.loading).toBe(true);
  await act(async () => { fresh.resolve(comparisonPage()); await second; });
});

test("StrictMode does not eagerly read and expires the single explicit owner on close", async () => {
  const { client, page } = fixture();
  const owner = comparisonOwner();
  const { result } = renderHook(() => useSessionImportSourceComparison(comparisonScope, client), {
    wrapper: ({ children }: PropsWithChildren) => <StrictMode>{children}</StrictMode>,
  });
  expect(page).not.toHaveBeenCalled();
  await act(async () => { await result.current.open(owner.groupId, owner.left, owner.right); });
  act(() => result.current.close());
  expect(result.current.owner).toBeNull();
  expect(result.current.page).toBeNull();
  expect(page).toHaveBeenCalledTimes(1);
});
