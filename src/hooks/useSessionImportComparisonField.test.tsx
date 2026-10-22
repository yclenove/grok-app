/** @vitest-environment jsdom */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { SourceComparisonFieldRequest, SourceComparisonFieldPage } from "@/lib/api/sessionMigrationComparisonField";
import type { ComparisonFieldContext } from "@/lib/sessionImport/comparisonFieldState";
import { comparisonDeferred } from "@/test/sessionImportComparisonFixture";
import { comparisonFieldContext, comparisonFieldPage } from "@/test/sessionImportComparisonFieldFixture";
import { useSessionImportComparisonField } from "./useSessionImportComparisonField";
afterEach(cleanup);
function fixture() {
  const context = comparisonFieldContext();
  const page = vi.fn(async (r: Readonly<SourceComparisonFieldRequest>) => comparisonFieldPage(context, r.field, Number(r.cursor?.split(":").at(-1) ?? 0)));
  return { context, page, client: { page } };
}
test("mount is read-free; explicit field read reserves synchronously and requests only 4096 bytes", async () => {
  const { context, page, client } = fixture();
  const pending = comparisonDeferred<SourceComparisonFieldPage>(); page.mockReturnValueOnce(pending.promise);
  const { result } = renderHook(() => useSessionImportComparisonField(context, client));
  expect(page).not.toHaveBeenCalled();
  let first!: Promise<boolean>;
  act(() => { first = result.current.open("kind"); });
  expect(await result.current.open("kind")).toBe(false);
  expect(await result.current.retry()).toBe(false);
  expect(page).toHaveBeenCalledExactlyOnceWith({ version: 1, left: context.owner.left, right: context.owner.right,
    side: context.side, position: 0, field: "kind", expectedFieldsDigest: context.fieldsDigest, limitBytes: 4096 });
  await act(async () => { pending.resolve(comparisonFieldPage(context)); expect(await first).toBe(true); });
  expect(result.current.loading).toBe(false);
});
test("next/previous replace text and renew the next field cursor; no accumulated full body", async () => {
  const { context, client, page } = fixture();
  const { result } = renderHook(() => useSessionImportComparisonField(context, client));
  await act(async () => { await result.current.open("kind"); await result.current.nextPage(); });
  expect(result.current.pageIndex).toBe(1);
  expect(result.current.page?.text).toBe(comparisonFieldPage(context, "kind", 4).text);
  await act(async () => { await result.current.previousPage(); });
  expect(page.mock.calls[2][0].cursor).toBeUndefined();
  await act(async () => { await result.current.nextPage(); await result.current.nextPage(); });
  expect(result.current.page?.complete).toBe(true);
  expect(await result.current.nextPage()).toBe(false);
});
test("failed next keeps the accepted segment and retries the exact field cursor", async () => {
  const { context, client, page } = fixture();
  page.mockResolvedValueOnce(comparisonFieldPage(context)).mockRejectedValueOnce(new Error("PLAN_STALE: field"));
  const { result } = renderHook(() => useSessionImportComparisonField(context, client));
  await act(async () => { await result.current.open("kind"); await result.current.nextPage(); });
  expect(result.current.error).toContain("PLAN_STALE");
  expect(result.current.pageIndex).toBe(0);
  expect(result.current.page?.startByte).toBe(0);
  expect(await result.current.nextPage()).toBe(false);
  await act(async () => { expect(await result.current.retry()).toBe(true); });
  expect(page.mock.calls[1][0].cursor).toBe(page.mock.calls[2][0].cursor);
});
test.each(["success", "error"])("switching field expires old %s and finally without clearing new busy state", async (mode) => {
  const { context, client, page } = fixture();
  const old = comparisonDeferred<SourceComparisonFieldPage>(), fresh = comparisonDeferred<SourceComparisonFieldPage>();
  page.mockReturnValueOnce(old.promise).mockReturnValueOnce(fresh.promise);
  const { result } = renderHook(() => useSessionImportComparisonField(context, client));
  let a!: Promise<boolean>, b!: Promise<boolean>;
  act(() => { a = result.current.open("kind"); b = result.current.open("blocks"); });
  await act(async () => { if (mode === "success") old.resolve(comparisonFieldPage(context)); else old.reject(new Error("old")); expect(await a).toBe(false); });
  expect(result.current.field).toBe("blocks"); expect(result.current.loading).toBe(true); expect(result.current.error).toBeNull();
  await act(async () => { fresh.resolve(comparisonFieldPage(context, "blocks")); expect(await b).toBe(true); });
});
test.each(["scan", "epoch", "generation", "leaf", "side", "row", "digest"])("committed %s change expires late fields", async (change) => {
  const { context, client, page } = fixture();
  const old = comparisonDeferred<SourceComparisonFieldPage>(); page.mockReturnValueOnce(old.promise);
  const { result, rerender } = renderHook(({ context }: { context: ComparisonFieldContext | null }) => useSessionImportComparisonField(context, client), { initialProps: { context } });
  let a!: Promise<boolean>; act(() => { a = result.current.open("kind"); });
  const next = structuredClone(context);
  switch (change) {
    case "scan": next.owner.scope.scanId = "other"; break;
    case "epoch": ++next.owner.scope.epoch; break;
    case "generation": ++next.owner.scope.generation; break;
    case "leaf": next.owner.right.leafId = "other"; break;
    case "side": next.side = "right"; break;
    case "row": ++next.position; break;
    default: next.fieldsDigest = "b".repeat(64);
  }
  rerender({ context: next });
  await act(async () => { old.resolve(comparisonFieldPage(context)); expect(await a).toBe(false); });
  expect(result.current.page).toBeNull(); expect(result.current.field).toBeNull(); expect(result.current.loading).toBe(false);
});
test("close/reopen the same context cannot reuse an old request owner", async () => {
  const { context, client, page } = fixture();
  const old = comparisonDeferred<SourceComparisonFieldPage>(), fresh = comparisonDeferred<SourceComparisonFieldPage>();
  page.mockReturnValueOnce(old.promise).mockReturnValueOnce(fresh.promise);
  const { result } = renderHook(() => useSessionImportComparisonField(context, client));
  let a!: Promise<boolean>, b!: Promise<boolean>;
  act(() => { a = result.current.open("kind"); result.current.close(); b = result.current.open("kind"); });
  await act(async () => { old.reject(new Error("old")); await a; });
  expect(result.current.loading).toBe(true); expect(result.current.error).toBeNull();
  await act(async () => { fresh.resolve(comparisonFieldPage(context)); expect(await b).toBe(true); });
});
test("unmount invalidates pending fields and never starts another read", async () => {
  const { context, client, page } = fixture();
  const old = comparisonDeferred<SourceComparisonFieldPage>(); page.mockReturnValueOnce(old.promise);
  const { result, unmount } = renderHook(() => useSessionImportComparisonField(context, client));
  let a!: Promise<boolean>; act(() => { a = result.current.open("kind"); });
  unmount(); old.resolve(comparisonFieldPage(context)); expect(await a).toBe(false); expect(page).toHaveBeenCalledTimes(1);
});
test("malformed field page fails closed; retry does not change fixed source", async () => {
  const { context, client, page } = fixture();
  page.mockResolvedValueOnce({ ...comparisonFieldPage(context), eventId: "foreign" });
  const { result } = renderHook(() => useSessionImportComparisonField(context, client));
  await act(async () => { expect(await result.current.open("kind")).toBe(false); });
  expect(result.current.page).toBeNull(); expect(result.current.error).not.toBeNull();
  await act(async () => { expect(await result.current.retry()).toBe(true); });
  expect(page.mock.calls[0][0]).toEqual(page.mock.calls[1][0]);
});
test("no context and unsafe field paths are not sent to native", async () => {
  const { context, client, page } = fixture();
  const { result, rerender } = renderHook(({ context }: { context: ComparisonFieldContext | null }) => useSessionImportComparisonField(context, client), { initialProps: { context: null as ComparisonFieldContext | null } });
  expect(await result.current.open("kind")).toBe(false); rerender({ context });
  expect(await result.current.open("/blocks/path")).toBe(false); expect(page).not.toHaveBeenCalled();
});
