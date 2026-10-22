/** @vitest-environment jsdom */
import { StrictMode, type PropsWithChildren } from "react";
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { SourceGroupMember, SourceGroupMemberPage } from "@/lib/api/sessionMigrationGroups";
import { groupCandidate, groupDeferred, groupMetadata, groupScope } from "@/test/sessionImportGroupFixture";
import { useSessionImportGroupMembers } from "./useSessionImportGroupMembers";

afterEach(cleanup);
const group = groupMetadata("a");
function member(id: string, owner = group): SourceGroupMember {
  return {
    groupId: owner.groupId, format: "cursor-transcript", sourceId: owner.sourceId,
    rootPath: "/synthetic/root", legacyId: `cursor-transcript:${id}`, location: null,
    candidate: { ...groupCandidate(id), nativeSessionId: owner.nativeSessionId },
  };
}
function page(ids = ["a", "alternate"], nextCursor: string | null = null, owner = group): SourceGroupMemberPage {
  return { version: 1, groupId: owner.groupId, members: ids.map((id) => member(id, owner)),
    nextCursor, memberCount: 3, complete: true, stateVersion: 1 };
}
function fixture() {
  const members = vi.fn(async (_scan: string, _epoch: number, _group: string, _options?: { cursor?: string; limit?: number }) => page());
  return { client: { members }, members };
}

test("does not eagerly load members; open requests a single metadata-only page of 50", async () => {
  const { client, members } = fixture();
  const { result } = renderHook(() => useSessionImportGroupMembers(client));
  expect(members).not.toHaveBeenCalled();
  expect(result.current.group).toBeNull();
  await act(async () => { expect(await result.current.open(groupScope, group)).toBe(true); });
  expect(members).toHaveBeenCalledExactlyOnceWith(groupScope.scanId, 1, group.groupId, { limit: 50 });
  expect(result.current.members.map((item) => item.candidate.candidateId)).toEqual([groupCandidate("a").candidateId, groupCandidate("alternate").candidateId]);
  expect(result.current.ready).toBe(true);
  expect(result.current.loading).toBe(false);
});

test("bounded next/previous pages replace rows rather than accumulate every version", async () => {
  const { client, members } = fixture();
  members.mockResolvedValueOnce(page(["a", "alternate"], "signed-next"))
    .mockResolvedValueOnce(page(["last"]))
    .mockResolvedValueOnce(page(["a", "alternate"], "renewed-next"));
  const { result } = renderHook(() => useSessionImportGroupMembers(client));
  await act(async () => { await result.current.open(groupScope, group); });
  await act(async () => { expect(await result.current.nextPage()).toBe(true); });
  expect(result.current.pageIndex).toBe(1);
  expect(result.current.members).toHaveLength(1);
  expect(result.current.members[0].candidate.candidateId).toBe(groupCandidate("last").candidateId);
  expect(members.mock.calls[1][3]).toEqual({ limit: 50, cursor: "signed-next" });
  await act(async () => { expect(await result.current.previousPage()).toBe(true); });
  expect(result.current.pageIndex).toBe(0);
  expect(members.mock.calls[2][3]).toEqual({ limit: 50 });
  expect(result.current.nextCursor).toBe("renewed-next");
});

test("same-turn double next-page clicks only launch one request", async () => {
  const { client, members } = fixture();
  const pending = groupDeferred<SourceGroupMemberPage>();
  members.mockResolvedValueOnce(page(["a"], "signed-next")).mockReturnValueOnce(pending.promise);
  const { result } = renderHook(() => useSessionImportGroupMembers(client));
  await act(async () => { await result.current.open(groupScope, group); });
  let first!: Promise<boolean>;
  let duplicate!: Promise<boolean>;
  act(() => { first = result.current.nextPage(); duplicate = result.current.nextPage(); });
  expect(result.current.loading).toBe(true);
  await act(async () => { pending.resolve(page(["alternate"])); await first; await duplicate; });
  expect(await duplicate).toBe(false);
  expect(members).toHaveBeenCalledTimes(2);
});

test("failure keeps the accepted page and retry uses the attempted cursor", async () => {
  const { client, members } = fixture();
  members.mockResolvedValueOnce(page(["a"], "signed-next"))
    .mockRejectedValueOnce(new Error("NETWORK: members unavailable")).mockResolvedValueOnce(page(["last"]));
  const { result } = renderHook(() => useSessionImportGroupMembers(client));
  await act(async () => { await result.current.open(groupScope, group); });
  await act(async () => { expect(await result.current.nextPage()).toBe(false); });
  expect(result.current.error).toContain("NETWORK");
  expect(result.current.pageIndex).toBe(0);
  expect(result.current.members[0].candidate.candidateId).toBe(groupCandidate("a").candidateId);
  expect(result.current.loading).toBe(false);
  await act(async () => { expect(await result.current.retry()).toBe(true); });
  expect(result.current.pageIndex).toBe(1);
  expect(result.current.error).toBeNull();
  expect(members.mock.calls[2][3]?.cursor).toBe("signed-next");
});

const ownerCases = (["success", "error"] as const).flatMap((outcome) =>
  (["close", "same-group-reopen", "other-group", "scan", "epoch", "generation", "unmount"] as const).map((change) => ({ outcome, change })));
test.each(ownerCases)("late $outcome after $change cannot replace the open menu or clear its loading", async ({ outcome, change }) => {
  const { client, members } = fixture();
  const old = groupDeferred<SourceGroupMemberPage>();
  const fresh = groupDeferred<SourceGroupMemberPage>();
  members.mockReturnValueOnce(old.promise).mockReturnValueOnce(fresh.promise);
  const onError = vi.fn();
  const { result, unmount } = renderHook(() => useSessionImportGroupMembers(client, { onError }));
  let oldRead!: Promise<boolean>;
  act(() => { oldRead = result.current.open(groupScope, group); });
  let freshRead: Promise<boolean> | undefined;
  const scope = { ...groupScope, ...(change === "scan" ? { scanId: "new-scan" }
    : change === "epoch" ? { epoch: 2 } : { generation: 2 }) };
  const nextGroup = change === "other-group" ? groupMetadata("b")
    : { ...group, groupId: group.groupId.replace(groupScope.scanId, scope.scanId),
      firstMember: { ...group.firstMember, candidateId: group.firstMember.candidateId.replace(groupScope.scanId, scope.scanId) } };
  if (change === "unmount") unmount();
  else if (change === "close") act(() => { result.current.close(); });
  else act(() => { freshRead = result.current.open(scope, nextGroup); });
  let oldResult!: boolean;
  await act(async () => {
    if (outcome === "success") old.resolve(page()); else old.reject(new Error("obsolete-menu"));
    oldResult = await oldRead;
  });
  const intermediate = result.current;
  if (freshRead) await act(async () => {
    fresh.resolve({ ...page(["new"], null, nextGroup), members: [
      { ...member("new", nextGroup), candidate: { ...member("new", nextGroup).candidate,
        candidateId: groupCandidate("new").candidateId.replace(groupScope.scanId, scope.scanId) } },
    ] });
    await freshRead;
  });
  expect(oldResult).toBe(false);
  expect(onError).not.toHaveBeenCalled();
  expect(intermediate.error).toBeNull();
  if (freshRead) {
    expect(intermediate.loading).toBe(true);
    expect(intermediate.ready).toBe(false);
    expect(intermediate.group?.groupId).toBe(nextGroup.groupId);
    expect(result.current.members[0].candidate.title).toBe("new");
    expect(result.current.ready).toBe(true);
  } else if (change === "close") {
    expect(intermediate.group).toBeNull();
    expect(intermediate.members).toEqual([]);
  }
});

test.each(["foreign-group", "foreign-candidate", "duplicate", "oversized", "format", "count", "source", "native-identity", "location", "schema"] as const)(
  "rejects $0 member metadata without adopting a partial page", async (kind) => {
    const { client, members } = fixture();
    const invalid = page();
    if (kind === "foreign-group") invalid.groupId = groupMetadata("b").groupId;
    if (kind === "foreign-candidate") invalid.members[0].candidate.candidateId = "candidate:foreign:a";
    if (kind === "duplicate") invalid.members[1] = invalid.members[0];
    if (kind === "oversized") invalid.members = Array.from({ length: 51 }, (_, index) => member(String(index)));
    if (kind === "format") Object.assign(invalid.members[0], { format: "unknown" });
    if (kind === "count") invalid.memberCount = 1;
    if (kind === "source") invalid.members[0].sourceId = "foreign-source";
    if (kind === "native-identity") invalid.members[0].candidate.nativeSessionId = "other-native-id";
    if (kind === "location") invalid.members[0].location = { version: 1, locationId: "location", sourceId: group.sourceId,
      provider: "cursor", host: "local", rootPath: "/different-root", kind: "associated" };
    if (kind === "schema") Object.assign(invalid, { version: 2 });
    members.mockResolvedValueOnce(invalid);
    const { result } = renderHook(() => useSessionImportGroupMembers(client));
    await act(async () => { expect(await result.current.open(groupScope, group)).toBe(false); });
    expect(result.current.ready).toBe(false);
    expect(result.current.members).toEqual([]);
    expect(result.current.loading).toBe(false);
    expect(result.current.error).toContain(kind === "schema" ? "SCHEMA_UNSUPPORTED" : "PLAN_STALE");
  },
);

test("empty live tail does not advance the cursor stack and can be retried for new discovery", async () => {
  const { client, members } = fixture();
  members.mockResolvedValueOnce(page(["a"], "signed-next"))
    .mockResolvedValueOnce({ ...page([], "signed-same-offset"), complete: false })
    .mockResolvedValueOnce({ ...page(["late"]), stateVersion: 2 });
  const { result } = renderHook(() => useSessionImportGroupMembers(client));
  await act(async () => { await result.current.open(groupScope, group); await result.current.nextPage(); });
  await act(async () => { expect(await result.current.nextPage()).toBe(false); });
  expect(result.current.pageIndex).toBe(1);
  expect(members).toHaveBeenCalledTimes(2);
  await act(async () => { expect(await result.current.retry()).toBe(true); });
  expect(result.current.members[0].candidate.title).toBe("late");
  expect(members.mock.calls[2][3]?.cursor).toBe("signed-next");
});

test("metadata versions cannot rewind an accepted page", async () => {
  const { client, members } = fixture();
  members.mockResolvedValueOnce({ ...page(), stateVersion: 3 }).mockResolvedValueOnce({ ...page(), stateVersion: 2 });
  const { result } = renderHook(() => useSessionImportGroupMembers(client));
  await act(async () => { await result.current.open(groupScope, group); });
  await act(async () => { expect(await result.current.retry()).toBe(false); });
  expect(result.current.metadataVersion).toBe(3);
  expect(result.current.error).toContain("PLAN_STALE");
});

test("invalid scope/group cannot launch IPC", async () => {
  const { client, members } = fixture();
  const { result } = renderHook(() => useSessionImportGroupMembers(client));
  await act(async () => { expect(await result.current.open({ ...groupScope, epoch: 0 }, group)).toBe(false); });
  expect(members).not.toHaveBeenCalled();
  await act(async () => { expect(await result.current.open(groupScope, { ...group, groupId: "group:foreign:a" })).toBe(false); });
  expect(members).not.toHaveBeenCalled();
});

test("StrictMode effect replay still allows one lazy request and unmount discards late errors", async () => {
  const { client, members } = fixture();
  const late = groupDeferred<SourceGroupMemberPage>();
  const onError = vi.fn();
  const { result, unmount } = renderHook(() => useSessionImportGroupMembers(client, { onError }), {
    wrapper: ({ children }: PropsWithChildren) => <StrictMode>{children}</StrictMode>,
  });
  await act(async () => { expect(await result.current.open(groupScope, group)).toBe(true); });
  members.mockReturnValueOnce(late.promise);
  let retry!: Promise<boolean>;
  act(() => { retry = result.current.retry(); });
  unmount();
  await act(async () => { late.reject(new Error("after-unmount")); expect(await retry).toBe(false); });
  expect(members).toHaveBeenCalledTimes(2);
  expect(onError).not.toHaveBeenCalled();
});
