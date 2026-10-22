/** @vitest-environment jsdom */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { sessionImportScanGroupMember, SourceGroupMember } from "@/lib/api/sessionMigrationGroups";
import { comparisonDeferred } from "@/test/sessionImportComparisonFixture";
import { previewComparisonContext, previewComparisonMember } from "@/test/sessionImportComparisonPreviewFixture";
import { useSessionImportComparisonPrimaryMember } from "./useSessionImportComparisonPrimaryMember";

afterEach(cleanup);
const fixture = () => vi.fn<typeof sessionImportScanGroupMember>(async () => previewComparisonMember("a"));

test("lazy exact metadata is independent of member paging and fixed body reads", async () => {
  const client = fixture(), context = previewComparisonContext();
  const { result } = renderHook(() => useSessionImportComparisonPrimaryMember(context, client));
  expect(client).not.toHaveBeenCalled();
  await act(async () => { expect(await result.current.load()).toBe(true); });
  expect(client).toHaveBeenCalledExactlyOnceWith(context.scope.scanId, context.scope.epoch, context.group.groupId, context.primary.candidateId);
  expect(result.current.member).toEqual(previewComparisonMember("a"));
});

test("same-turn reservation prevents duplicates; retry pins the original scope and metadata identity", async () => {
  const client = fixture(), old = comparisonDeferred<SourceGroupMember>(), context = previewComparisonContext();
  client.mockReturnValueOnce(old.promise);
  const { result } = renderHook(() => useSessionImportComparisonPrimaryMember(context, client));
  let pending!: Promise<boolean>;
  act(() => { pending = result.current.load(); });
  expect(await result.current.load()).toBe(false);
  context.primary.title = "caller display-only change";
  await act(async () => { old.reject(new Error("SOURCE_NOT_FOUND: original")); await pending; });
  expect(result.current.error).toContain("SOURCE_NOT_FOUND");
  expect(result.current.member).toBeNull();
  await act(async () => { expect(await result.current.load()).toBe(true); });
  expect(client.mock.calls[1]).toEqual(client.mock.calls[0]);
});

test.each(["success", "error"] as const)("late %s after close/reopen cannot finish the fresh owner", async (outcome) => {
  const client = fixture(), old = comparisonDeferred<SourceGroupMember>(), fresh = comparisonDeferred<SourceGroupMember>();
  client.mockReturnValueOnce(old.promise).mockReturnValueOnce(fresh.promise);
  const context = previewComparisonContext();
  const { result, rerender } = renderHook(({ context }) => useSessionImportComparisonPrimaryMember(context, client), {
    initialProps: { context: context as typeof context | null },
  });
  let first!: Promise<boolean>, second!: Promise<boolean>;
  act(() => { first = result.current.load(); });
  rerender({ context: null }); rerender({ context });
  act(() => { second = result.current.load(); });
  await act(async () => { if (outcome === "success") old.resolve(previewComparisonMember("a")); else old.reject(new Error("obsolete")); await first; });
  expect(await first).toBe(false);
  expect(result.current.loading).toBe(true);
  expect(result.current.error).toBeNull(); expect(result.current.member).toBeNull();
  await act(async () => { fresh.resolve(previewComparisonMember("a")); await second; });
  expect(await second).toBe(true);
});

test.each(["scan", "epoch", "generation", "group", "primary", "revision", "native", "source", "host", "unmount"] as const)(
  "committed %s change expires old success/error/finally", async (change) => {
    const client = fixture(), old = comparisonDeferred<SourceGroupMember>(), context = previewComparisonContext();
    client.mockReturnValueOnce(old.promise);
    const { result, rerender, unmount } = renderHook(({ context }) => useSessionImportComparisonPrimaryMember(context, client), { initialProps: { context } });
    let pending!: Promise<boolean>;
    act(() => { pending = result.current.load(); });
    const next = structuredClone(context);
    if (change === "scan") next.scope.scanId = "other";
    if (change === "epoch") next.scope.epoch++;
    if (change === "generation") next.scope.generation++;
    if (change === "group") next.group.groupId = "other";
    if (change === "primary") next.primary.candidateId = "candidate:synthetic-scan:b";
    if (change === "revision") next.primary.sourceRevision = "changed";
    if (change === "native") next.primary.nativeSessionId = "changed";
    if (change === "source") next.group.sourceId = "other";
    if (change === "host") next.group.host = "other";
    if (change === "unmount") unmount(); else rerender({ context: next });
    await act(async () => { old.reject(new Error("old-failure")); await pending; });
    expect(await pending).toBe(false);
    if (change !== "unmount") expect(result.current).toMatchObject({ member: null, error: null, loading: false });
  },
);

test.each(["candidate", "revision", "source", "group", "native", "format", "budget"] as const)(
  "foreign or invalid %s metadata cannot become A", async (kind) => {
    const member = previewComparisonMember("a"), client = fixture();
    if (kind === "candidate") member.candidate.candidateId = previewComparisonMember("b").candidate.candidateId;
    if (kind === "revision") member.candidate.sourceRevision = "changed";
    if (kind === "source") member.sourceId = "foreign";
    if (kind === "group") member.groupId = "foreign";
    if (kind === "native") member.candidate.nativeSessionId = "foreign";
    if (kind === "format") Object.assign(member, { format: "unknown" });
    if (kind === "budget") member.candidate.title = "中".repeat(100000);
    client.mockResolvedValue(member);
    const { result } = renderHook(() => useSessionImportComparisonPrimaryMember(previewComparisonContext(), client));
    await act(async () => { expect(await result.current.load()).toBe(false); });
    expect(result.current.error).toContain("PLAN_STALE"); expect(result.current.member).toBeNull();
    expect(client).toHaveBeenCalledTimes(1);
  },
);

test("accepted metadata is detached from caller objects and display-only growth keeps its owner", async () => {
  const client = fixture(), member = previewComparisonMember("a"), context = previewComparisonContext();
  client.mockResolvedValue(member);
  const { result, rerender } = renderHook(({ context }) => useSessionImportComparisonPrimaryMember(context, client), { initialProps: { context } });
  await act(async () => { await result.current.load(); });
  member.rootPath = "/caller-mutated";
  rerender({ context: { ...context, primary: { ...context.primary, title: "new display title" }, group: { ...context.group, memberCount: 3 } } });
  expect(result.current.member?.rootPath).toBe("/synthetic/other-root");
  expect(client).toHaveBeenCalledTimes(1);
});
