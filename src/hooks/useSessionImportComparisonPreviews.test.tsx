/** @vitest-environment jsdom */
import { StrictMode, type PropsWithChildren } from "react";
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { sessionImportPreview } from "@/lib/api/sessionMigration";
import { comparisonDeferred } from "@/test/sessionImportComparisonFixture";
import { previewComparisonContext, previewComparisonMember, previewComparisonResponse } from "@/test/sessionImportComparisonPreviewFixture";
import { useSessionImportComparisonPreviews } from "./useSessionImportComparisonPreviews";

afterEach(cleanup);
function fixture() {
  const preview = vi.fn<typeof sessionImportPreview>(async (candidate: string, _revision?: string | null) => previewComparisonResponse(candidate.split(":").at(-1)));
  return { client: { preview }, preview };
}
type Reply = Awaited<ReturnType<typeof sessionImportPreview>>;

test("no eager bodies; explicit start reads A only and explicit B pins the expected revision", async () => {
  const { client, preview } = fixture();
  const context = previewComparisonContext();
  const member = previewComparisonMember();
  const { result } = renderHook(() => useSessionImportComparisonPreviews(context, client));
  expect(preview).not.toHaveBeenCalled();
  await act(async () => { expect(await result.current.start()).toBe(true); });
  expect(preview).toHaveBeenCalledExactlyOnceWith(context.primary.candidateId, context.primary.sourceRevision);
  expect(result.current.right).toBeNull();
  expect(result.current.pair).toBeNull();
  await act(async () => { expect(await result.current.chooseRight(member)).toBe(true); });
  expect(preview).toHaveBeenLastCalledWith(member.candidate.candidateId, member.candidate.sourceRevision);
  expect(result.current.pair?.left.leafId).toBe("leaf-a");
  expect(result.current.pair?.right.leafId).toBe("leaf-b");
  expect(context.primary).toEqual(previewComparisonContext().primary);
});

test("A and B can load independently, while same-turn duplicate B reads are refused", async () => {
  const { client, preview } = fixture();
  const left = comparisonDeferred<Reply>();
  const right = comparisonDeferred<Reply>();
  preview.mockReturnValueOnce(left.promise).mockReturnValueOnce(right.promise);
  const { result } = renderHook(() => useSessionImportComparisonPreviews(previewComparisonContext(), client));
  let a!: Promise<boolean>;
  let b!: Promise<boolean>;
  act(() => { a = result.current.start(); b = result.current.chooseRight(previewComparisonMember()); });
  expect(await result.current.chooseRight(previewComparisonMember("c"))).toBe(false);
  await act(async () => { left.resolve(previewComparisonResponse()); await a; });
  expect(result.current.right?.loading).toBe(true);
  expect(result.current.pair).toBeNull();
  await act(async () => { right.resolve(previewComparisonResponse("b")); await b; });
  expect(result.current.pair).not.toBeNull();
  expect(preview).toHaveBeenCalledTimes(2);
});

test("changing B replaces only its fixed owner; choosing a branch never reads or edits main selection", async () => {
  const { client, preview } = fixture();
  const { result } = renderHook(() => useSessionImportComparisonPreviews(previewComparisonContext(), client));
  await act(async () => { await result.current.start(); await result.current.chooseRight(previewComparisonMember()); });
  const left = result.current.left;
  act(() => { expect(result.current.chooseLeaf("left", "branch-a")).toBe(true); });
  expect(preview).toHaveBeenCalledTimes(2);
  expect(result.current.pair?.left.leafId).toBe("branch-a");
  expect(result.current.left?.summary?.leaf).toBe("leaf-a");
  const currentLeft = result.current.left;
  expect(currentLeft?.adapter).toBe(left?.adapter);
  expect(result.current.chooseLeaf("right", "foreign-leaf")).toBe(false);
  await act(async () => { await result.current.chooseRight(previewComparisonMember("c")); });
  expect(result.current.left).toBe(currentLeft);
  expect(result.current.pair?.right.candidateId).toBe(previewComparisonMember("c").candidate.candidateId);
});

test("failed B retains A and retries only B's original candidate/revision with no alternate-source fallback", async () => {
  const { client, preview } = fixture();
  preview.mockResolvedValueOnce(previewComparisonResponse()).mockRejectedValueOnce(new Error("SOURCE_CHANGED: B changed"));
  const { result } = renderHook(() => useSessionImportComparisonPreviews(previewComparisonContext(), client));
  const member = previewComparisonMember();
  await act(async () => { await result.current.start(); expect(await result.current.chooseRight(member)).toBe(false); });
  member.candidate.sourceRevision = "mutated-caller";
  expect(result.current.left?.previewId).toBe("fixed-preview-a");
  expect(result.current.right?.error).toContain("SOURCE_CHANGED");
  expect(result.current.pair).toBeNull();
  await act(async () => { expect(await result.current.retry("right")).toBe(true); });
  expect(preview.mock.calls[2]).toEqual([previewComparisonMember().candidate.candidateId, "revision-b"]);
  expect(result.current.pair).not.toBeNull();
});

test.each(["success", "error"] as const)("late A %s after close/reopen cannot finish or resurrect a new pair", async (outcome) => {
  const { client, preview } = fixture();
  const old = comparisonDeferred<Reply>();
  const fresh = comparisonDeferred<Reply>();
  preview.mockReturnValueOnce(old.promise).mockReturnValueOnce(fresh.promise);
  const { result } = renderHook(() => useSessionImportComparisonPreviews(previewComparisonContext(), client));
  let first!: Promise<boolean>;
  let second!: Promise<boolean>;
  act(() => { first = result.current.start(); });
  act(() => { result.current.close(); second = result.current.start(); });
  await act(async () => { if (outcome === "success") old.resolve(previewComparisonResponse()); else old.reject(new Error("obsolete")); await first; });
  expect(await first).toBe(false);
  expect(result.current.left?.loading).toBe(true);
  expect(result.current.left?.error).toBeNull();
  expect(result.current.left?.previewId).toBeNull();
  await act(async () => { fresh.resolve(previewComparisonResponse()); await second; });
  expect(result.current.left?.previewId).toBe("fixed-preview-a");
});

test.each(["scan", "epoch", "generation", "group", "primary", "revision", "source", "null", "unmount"] as const)(
  "committed %s changes expire B success/error/finally", async (change) => {
    const { client, preview } = fixture();
    const old = comparisonDeferred<Reply>();
    preview.mockResolvedValueOnce(previewComparisonResponse()).mockReturnValueOnce(old.promise);
    const context = previewComparisonContext();
    const { result, rerender, unmount } = renderHook(({ context }: { context: ReturnType<typeof previewComparisonContext> | null }) =>
      useSessionImportComparisonPreviews(context, client), { initialProps: { context: context as ReturnType<typeof previewComparisonContext> | null } });
    await act(async () => { await result.current.start(); });
    let pending!: Promise<boolean>;
    act(() => { pending = result.current.chooseRight(previewComparisonMember()); });
    const next = structuredClone(context);
    if (change === "scan") next.scope.scanId = "another-scan";
    if (change === "epoch") next.scope.epoch++;
    if (change === "generation") next.scope.generation++;
    if (change === "group") next.group.groupId = "another-group";
    if (change === "primary") next.primary.candidateId = "candidate:synthetic-scan:c";
    if (change === "revision") next.primary.sourceRevision = "changed";
    if (change === "source") next.group.sourceId = "changed-installation";
    if (change === "unmount") unmount(); else rerender({ context: change === "null" ? null : next });
    await act(async () => { old.reject(new Error("old-B-failure")); await pending; });
    expect(await pending).toBe(false);
    if (change !== "unmount") {
      expect(result.current.open).toBe(false);
      expect(result.current.left).toBeNull();
      expect(result.current.right).toBeNull();
      expect(result.current.error).toBeNull();
    }
  },
);

test("foreign B and malformed previews fail closed rather than becoming a pair", async () => {
  const { client, preview } = fixture();
  const { result } = renderHook(() => useSessionImportComparisonPreviews(previewComparisonContext(), client));
  await act(async () => { await result.current.start(); });
  const member = previewComparisonMember();
  member.sourceId = "foreign";
  await act(async () => { expect(await result.current.chooseRight(member)).toBe(false); });
  expect(preview).toHaveBeenCalledTimes(1);
  expect(result.current.error).toContain("PLAN_STALE");
  const invalid = previewComparisonResponse("b");
  invalid.adapter.activeLeaf = "";
  preview.mockResolvedValueOnce(invalid);
  await act(async () => { expect(await result.current.chooseRight(previewComparisonMember())).toBe(false); });
  expect(result.current.right?.error).toContain("PLAN_STALE");
  expect(result.current.pair).toBeNull();
});

test("a shared preview ID cannot be accepted for both sides", async () => {
  const { client, preview } = fixture();
  preview.mockResolvedValue(previewComparisonResponse());
  const { result } = renderHook(() => useSessionImportComparisonPreviews(previewComparisonContext(), client));
  await act(async () => { await result.current.start(); expect(await result.current.chooseRight(previewComparisonMember())).toBe(false); });
  expect(result.current.right?.error).toContain("snapshot identity");
  expect(result.current.pair).toBeNull();
});

test("metadata growth neither rereads bodies nor changes fixed primary revision", async () => {
  const { client, preview } = fixture();
  const context = previewComparisonContext();
  const { result, rerender } = renderHook(({ context }) => useSessionImportComparisonPreviews(context, client), { initialProps: { context } });
  await act(async () => { await result.current.start(); });
  rerender({ context: { ...context, primary: { ...context.primary, title: "updated display" }, group: { ...context.group, memberCount: 3 } } });
  expect(result.current.open).toBe(true);
  expect(result.current.left?.previewId).toBe("fixed-preview-a");
  expect(preview).toHaveBeenCalledTimes(1);
});

test("StrictMode stays lazy and explicit close releases UI slots without claiming native deletion", async () => {
  const { client, preview } = fixture();
  const { result } = renderHook(() => useSessionImportComparisonPreviews(previewComparisonContext(), client), {
    wrapper: ({ children }: PropsWithChildren) => <StrictMode>{children}</StrictMode>,
  });
  expect(preview).not.toHaveBeenCalled();
  await act(async () => { await result.current.start(); });
  act(() => result.current.close());
  expect(result.current.open).toBe(false);
  expect(result.current.context).toBeNull();
  expect(result.current.pair).toBeNull();
  expect(preview).toHaveBeenCalledTimes(1);
});
