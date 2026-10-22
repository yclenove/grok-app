/** @vitest-environment jsdom */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { GroupSelectionMembership } from "@/lib/api/sessionMigrationGroups";
import { useSessionImportGroups } from "./useSessionImportGroups";
import {
  groupCandidate, groupChoice, groupClient, groupDeferred, groupDescriptor,
  groupMetadata, groupPage, groupScope,
} from "@/test/sessionImportGroupFixture";

afterEach(cleanup);
const a = groupMetadata("a").groupId;
const b = groupMetadata("b").groupId;

test("initializes a separate fixed-groups selector and one bounded page", async () => {
  const { client } = groupClient();
  const { result } = renderHook(() => useSessionImportGroups(client));
  await act(async () => { expect(await result.current.start(groupScope)).toBe(true); });
  expect(result.current.selection?.domain).toBe("fixed-groups");
  expect(result.current.ready).toBe(true);
  expect(result.current.groups.map((group) => group.groupId)).toEqual([a, b]);
  expect(result.current.choices[a].candidate.candidateId).toBe(groupCandidate("a").candidateId);
  expect(client.groups).toHaveBeenCalledExactlyOnceWith(groupScope.scanId, 1, { limit: 50, filter: {} });
  expect(client.membership).toHaveBeenCalledExactlyOnceWith(groupScope.scanId, 1, "synthetic-selector", [a, b]);
});

test.each(["set", "invert", "freeze", "clear"] as const)("%s reserves mutation synchronously and forwards the accepted CAS version", async (operation) => {
  const { client } = groupClient();
  const pending = groupDeferred<ReturnType<typeof groupDescriptor>>();
  client[operation].mockReturnValueOnce(pending.promise);
  const { result } = renderHook(() => useSessionImportGroups(client));
  await act(async () => { await result.current.start(groupScope); });
  const invoke = () => operation === "set" ? result.current.setSelected([a], true)
    : operation === "invert" ? result.current.invert([a]) : result.current[operation]();
  let first!: Promise<boolean>;
  let duplicate!: Promise<boolean>;
  act(() => { first = invoke(); duplicate = invoke(); });
  const busy = result.current.selectionBusy;
  const selectedCount = operation === "freeze" ? 2 : operation === "clear" ? 0 : 1;
  client.membership.mockResolvedValue({ selectionId: "synthetic-selector", stateVersion: 2,
    choices: [groupChoice("a", selectedCount > 0), groupChoice("b", selectedCount === 2)] });
  let succeeded!: boolean;
  await act(async () => { pending.resolve(groupDescriptor(2, selectedCount)); succeeded = await first; await duplicate; });
  expect(busy).toBe(true);
  expect(client[operation]).toHaveBeenCalledTimes(1);
  expect(client[operation].mock.calls[0][0]).toEqual({
    scanId: groupScope.scanId, epoch: 1, selectionId: "synthetic-selector", expectedStateVersion: 1,
  });
  expect(await duplicate).toBe(false);
  expect(succeeded).toBe(true);
  expect(result.current.selectionBusy).toBe(false);
});

test("choosing a different member invalidates only its prior candidate after the acknowledged change", async () => {
  const { client } = groupClient();
  const onMemberChanged = vi.fn();
  const { result } = renderHook(() => useSessionImportGroups(client, { onMemberChanged }));
  await act(async () => { await result.current.start(groupScope); });
  await act(async () => { await result.current.choose(a, groupCandidate("alternate").candidateId); });
  expect(onMemberChanged).toHaveBeenCalledTimes(1);
  expect(onMemberChanged.mock.calls[0][1]).toBe(groupCandidate("a").candidateId);
  expect(result.current.choices[a].candidate.candidateId).toBe(groupCandidate("alternate").candidateId);
  expect(result.current.choices[b].candidate.candidateId).toBe(groupCandidate("b").candidateId);
  expect(result.current.choices[a].selected).toBe(false);
});

test("a no-op descriptor is valid and choosing the same member does not invalidate content", async () => {
  const { client } = groupClient();
  client.choose.mockResolvedValueOnce({ selection: groupDescriptor(), groupId: a,
    previousCandidateId: groupCandidate("a").candidateId, candidateId: groupCandidate("a").candidateId, selected: false });
  const onMemberChanged = vi.fn();
  const { result } = renderHook(() => useSessionImportGroups(client, { onMemberChanged }));
  await act(async () => { await result.current.start(groupScope); });
  await act(async () => { expect(await result.current.choose(a, groupCandidate("a").candidateId)).toBe(true); });
  expect(onMemberChanged).not.toHaveBeenCalled();
  expect(result.current.selection?.stateVersion).toBe(1);
});

test("failed CAS retains the prior member and requires explicit refresh before a new version is used", async () => {
  const { client } = groupClient();
  client.choose.mockRejectedValueOnce(new Error("PLAN_STALE: selection changed"));
  const onMemberChanged = vi.fn();
  const { result } = renderHook(() => useSessionImportGroups(client, { onMemberChanged }));
  await act(async () => { await result.current.start(groupScope); });
  const prior = result.current.choices[a];
  await act(async () => { expect(await result.current.choose(a, groupCandidate("alternate").candidateId)).toBe(false); });
  expect(result.current.choices[a]).toBe(prior);
  expect(result.current.selectionBusy).toBe(false);
  expect(result.current.error).toContain("PLAN_STALE");
  expect(onMemberChanged).not.toHaveBeenCalled();
  client.get.mockResolvedValue(groupDescriptor(5));
  client.membership.mockResolvedValue({ selectionId: "synthetic-selector", stateVersion: 5, choices: [groupChoice("a"), groupChoice("b")] });
  await act(async () => { await result.current.refresh(); });
  client.choose.mockResolvedValueOnce({ selection: groupDescriptor(5), groupId: a,
    previousCandidateId: groupCandidate("a").candidateId, candidateId: groupCandidate("a").candidateId, selected: false });
  await act(async () => { await result.current.choose(a, groupCandidate("a").candidateId); });
  expect(client.choose.mock.calls[1][0]).toMatchObject({ expectedStateVersion: 5 });
});

test("newer membership is reconciled with the authoritative descriptor before adopting either", async () => {
  const { client } = groupClient();
  client.get.mockResolvedValueOnce(groupDescriptor()).mockResolvedValue(groupDescriptor(3, 1));
  client.membership.mockResolvedValue({ selectionId: "synthetic-selector", stateVersion: 3,
    choices: [groupChoice("a", true), groupChoice("b")] });
  const { result } = renderHook(() => useSessionImportGroups(client));
  await act(async () => { await result.current.start(groupScope); });
  expect(client.get).toHaveBeenCalledTimes(2);
  expect(client.membership).toHaveBeenCalledTimes(2);
  expect(result.current.selection?.selectedCount).toBe(1);
  expect(result.current.choices[a].selected).toBe(true);
  expect(result.current.ready).toBe(true);
});

test("an older descriptor cannot rewind a currently accepted selection", async () => {
  const { client } = groupClient();
  client.get.mockResolvedValueOnce(groupDescriptor(3));
  client.membership.mockResolvedValue({ selectionId: "synthetic-selector", stateVersion: 3, choices: [groupChoice("a"), groupChoice("b")] });
  const { result } = renderHook(() => useSessionImportGroups(client));
  await act(async () => { await result.current.start(groupScope); });
  client.get.mockResolvedValueOnce(groupDescriptor(2));
  await act(async () => { expect(await result.current.refresh()).toBe(false); });
  expect(result.current.selection?.stateVersion).toBe(3);
  expect(result.current.error).toContain("PLAN_STALE");
});

test.each(["duplicate", "foreign", "missing"] as const)("rejects %s membership instead of adopting a partial successful page", async (kind) => {
  const { client } = groupClient();
  const choices = kind === "duplicate" ? [groupChoice("a"), groupChoice("a")]
    : kind === "foreign" ? [groupChoice("a"), groupChoice("c")] : [groupChoice("a")];
  client.membership.mockResolvedValueOnce({ selectionId: "synthetic-selector", stateVersion: 1, choices });
  const { result } = renderHook(() => useSessionImportGroups(client));
  await act(async () => { expect(await result.current.start(groupScope)).toBe(false); });
  expect(result.current.ready).toBe(false);
  expect(result.current.groups).toEqual([]);
  expect(result.current.choices).toEqual({});
  expect(result.current.error).toContain("PLAN_STALE");
});

test("page replacement stays bounded and previous-page navigation uses the old page's cursor", async () => {
  const { client } = groupClient();
  client.groups.mockResolvedValueOnce(groupPage(["a", "b"], "signed-next"))
    .mockResolvedValueOnce(groupPage(["c"])).mockResolvedValueOnce(groupPage(["a", "b"], "renewed-next"));
  const { result } = renderHook(() => useSessionImportGroups(client));
  await act(async () => { await result.current.start(groupScope); });
  await act(async () => { await result.current.nextPage(); });
  expect(result.current.pageIndex).toBe(1);
  expect(result.current.groups.map((group) => group.groupId)).toEqual([groupMetadata("c").groupId]);
  expect(Object.keys(result.current.choices)).toEqual([groupMetadata("c").groupId]);
  expect(client.groups.mock.calls[1][2]).toMatchObject({ cursor: "signed-next", limit: 50 });
  await act(async () => { await result.current.previousPage(); });
  expect(result.current.pageIndex).toBe(0);
  expect(client.groups.mock.calls[2][2]?.cursor).toBeUndefined();
  expect(result.current.nextCursor).toBe("renewed-next");
});

test("metadata-only refresh updates member counts without rereading signed choices or replacing the chosen member", async () => {
  const { client, choices } = groupClient();
  choices.set(a, groupChoice("a", true, "preferred"));
  client.get.mockResolvedValue(groupDescriptor(2, 1));
  client.membership.mockResolvedValue({ selectionId: "synthetic-selector", stateVersion: 2, choices: [choices.get(a)!, choices.get(b)!] });
  const { result } = renderHook(() => useSessionImportGroups(client));
  await act(async () => { await result.current.start(groupScope); });
  client.membership.mockClear();
  client.groups.mockResolvedValueOnce({ ...groupPage(), stateVersion: 5,
    groups: [{ ...groupMetadata("a"), memberCount: 4 }, groupMetadata("b")] });
  await act(async () => { await result.current.refreshMetadata(); });
  expect(result.current.groups[0].memberCount).toBe(4);
  expect(result.current.choices[a].candidate.candidateId).toBe(groupCandidate("preferred").candidateId);
  expect(result.current.choices[a].selected).toBe(true);
  expect(client.membership).not.toHaveBeenCalled();
});

test.each(["success", "error"] as const)("reset then reopen the same scope drops obsolete mutation %s and finally", async (outcome) => {
  const { client } = groupClient();
  const old = groupDeferred<ReturnType<typeof groupDescriptor>>();
  const fresh = groupDeferred<ReturnType<typeof groupDescriptor>>();
  client.freeze.mockReturnValueOnce(old.promise).mockReturnValueOnce(fresh.promise);
  const onError = vi.fn();
  const { result } = renderHook(() => useSessionImportGroups(client, { onError }));
  await act(async () => { await result.current.start(groupScope); });
  let first!: Promise<boolean>;
  let second!: Promise<boolean>;
  act(() => { first = result.current.freeze(); result.current.reset(); });
  await act(async () => { await result.current.start(groupScope); });
  act(() => { second = result.current.freeze(); });
  await act(async () => {
    if (outcome === "success") old.resolve(groupDescriptor()); else old.reject(new Error("obsolete-mutation"));
    await first;
  });
  const intermediate = result.current;
  await act(async () => { fresh.resolve(groupDescriptor()); await second; });
  expect(intermediate.selectionBusy).toBe(true);
  expect(intermediate.error).toBeNull();
  expect(onError).not.toHaveBeenCalled();
  expect(result.current.selectionBusy).toBe(false);
});

test.each(["source", "epoch", "generation", "reset", "unmount"] as const)("%s change drops old initialization success and error callback", async (change) => {
  const { client } = groupClient();
  const old = groupDeferred<ReturnType<typeof groupDescriptor>>();
  client.get.mockReturnValueOnce(old.promise);
  const onError = vi.fn();
  const { result, unmount } = renderHook(() => useSessionImportGroups(client, { onError }));
  let first!: Promise<boolean>;
  act(() => { first = result.current.start(groupScope); });
  if (change === "unmount") unmount();
  else if (change === "reset") act(() => { result.current.reset(); });
  else {
    const scope = { ...groupScope,
      ...(change === "source" ? { scanId: "different-scan" } : change === "epoch" ? { epoch: 2 } : { generation: 2 }),
    };
    client.get.mockResolvedValueOnce({ ...groupDescriptor(), scanId: scope.scanId, epoch: scope.epoch });
    client.groups.mockResolvedValueOnce(groupPage([]));
    await act(async () => { await result.current.start(scope); });
  }
  await act(async () => { old.reject(new Error("old-scope")); await first; });
  expect(onError).not.toHaveBeenCalled();
  expect(result.current.error).toBeNull();
  expect(client.groups).toHaveBeenCalledTimes(["reset", "unmount"].includes(change) ? 0 : 1);
});

test("a late page cannot replace a newer filter's page or clear its busy flag", async () => {
  const { client } = groupClient();
  const old = groupDeferred<ReturnType<typeof groupPage>>();
  const fresh = groupDeferred<ReturnType<typeof groupPage>>();
  const { result } = renderHook(() => useSessionImportGroups(client));
  await act(async () => { await result.current.start(groupScope); });
  client.groups.mockReturnValueOnce(old.promise).mockReturnValueOnce(fresh.promise);
  let first!: Promise<boolean>;
  let second!: Promise<boolean>;
  await act(async () => { first = result.current.refresh(); });
  expect(client.groups).toHaveBeenCalledTimes(2);
  await act(async () => { second = result.current.setFilter({ provider: "cursor" }); });
  expect(client.groups).toHaveBeenCalledTimes(3);
  await act(async () => { old.resolve(groupPage(["a"])); await first; });
  const intermediate = result.current;
  await act(async () => { fresh.resolve(groupPage(["b"])); await second; });
  expect(intermediate.loading).toBe(true);
  expect(result.current.groups.map((group) => group.groupId)).toEqual([b]);
  expect(result.current.loading).toBe(false);
});

test("a synchronous member callback changing scope cannot let the old mutation steal the new page owner", async () => {
  const { client } = groupClient();
  const fresh = groupDeferred<ReturnType<typeof groupDescriptor>>();
  let startFresh!: () => void;
  let freshStart!: Promise<boolean>;
  const onMemberChanged = vi.fn(() => startFresh());
  const { result } = renderHook(() => useSessionImportGroups(client, { onMemberChanged }));
  await act(async () => { await result.current.start(groupScope); });
  client.get.mockReturnValueOnce(fresh.promise);
  startFresh = () => { freshStart = result.current.start({ ...groupScope, generation: 2 }); };
  let changed!: boolean;
  await act(async () => { changed = await result.current.choose(a, groupCandidate("alternate").candidateId); });
  const intermediate = result.current;
  const calls = { get: client.get.mock.calls.length, groups: client.groups.mock.calls.length };
  await act(async () => { fresh.resolve({ ...groupDescriptor(2), choiceCount: 1 }); await freshStart; });
  expect(changed).toBe(false);
  expect(onMemberChanged).toHaveBeenCalledTimes(1);
  expect(calls).toEqual({ get: 2, groups: 1 });
  expect(intermediate.scope?.generation).toBe(2);
  expect(intermediate.loading).toBe(true);
  expect(intermediate.ready).toBe(false);
  expect(result.current.ready).toBe(true);
  expect(result.current.selectionBusy).toBe(false);
});

test("start snapshots a caller's mutable filter before awaiting the descriptor", async () => {
  const { client } = groupClient();
  const descriptor = groupDeferred<ReturnType<typeof groupDescriptor>>();
  client.get.mockReturnValueOnce(descriptor.promise);
  client.groups.mockResolvedValueOnce(groupPage([]));
  const { result } = renderHook(() => useSessionImportGroups(client));
  const filter = { sourceId: "original-source" };
  let started!: Promise<boolean>;
  act(() => { started = result.current.start(groupScope, filter); });
  filter.sourceId = "caller-mutated-source";
  await act(async () => { descriptor.resolve(groupDescriptor()); await started; });
  expect(client.groups.mock.calls[0][2]?.filter).toEqual({ sourceId: "original-source" });
  expect(result.current.filter).toEqual({ sourceId: "original-source" });
  expect(result.current.filter).not.toBe(filter);
});

const lateReadCases = (["groups", "membership"] as const).flatMap((phase) =>
  (["success", "error"] as const).flatMap((outcome) =>
    (["source", "epoch", "generation", "reset", "unmount"] as const).map((change) => ({ phase, outcome, change }))));

test.each(lateReadCases)("late $phase $outcome after $change cannot change the new owner", async ({ phase, outcome, change }) => {
  const { client } = groupClient();
  const oldPage = groupDeferred<ReturnType<typeof groupPage>>();
  const oldMembership = groupDeferred<GroupSelectionMembership>();
  const fresh = groupDeferred<ReturnType<typeof groupDescriptor>>();
  const onError = vi.fn();
  const { result, unmount } = renderHook(() => useSessionImportGroups(client, { onError }));
  await act(async () => { await result.current.start(groupScope); });
  if (phase === "groups") client.groups.mockReturnValueOnce(oldPage.promise);
  else client.membership.mockReturnValueOnce(oldMembership.promise);
  let oldRead!: Promise<boolean>;
  await act(async () => { oldRead = result.current.refresh(); });
  expect(client.groups).toHaveBeenCalledTimes(2);
  expect(client.membership).toHaveBeenCalledTimes(phase === "membership" ? 2 : 1);
  let newRead: Promise<boolean> | undefined;
  const scope = { ...groupScope,
    ...(change === "source" ? { scanId: "different-scan" } : change === "epoch" ? { epoch: 2 } : { generation: 2 }),
  };
  if (change === "unmount") unmount();
  else if (change === "reset") act(() => { result.current.reset(); });
  else {
    client.get.mockReturnValueOnce(fresh.promise);
    act(() => { newRead = result.current.start(scope); });
  }
  let oldResult!: boolean;
  await act(async () => {
    if (outcome === "error") (phase === "groups" ? oldPage : oldMembership).reject(new Error("obsolete-read"));
    else if (phase === "groups") oldPage.resolve(groupPage());
    else oldMembership.resolve({ selectionId: "synthetic-selector", stateVersion: 1, choices: [groupChoice("a"), groupChoice("b")] });
    oldResult = await oldRead;
  });
  const intermediate = result.current;
  const reads = { groups: client.groups.mock.calls.length, membership: client.membership.mock.calls.length };
  if (newRead) {
    client.groups.mockResolvedValueOnce(groupPage([]));
    await act(async () => { fresh.resolve({ ...groupDescriptor(), scanId: scope.scanId, epoch: scope.epoch }); await newRead; });
  }
  expect(oldResult).toBe(false);
  expect(onError).not.toHaveBeenCalled();
  expect(intermediate.error).toBeNull();
  expect(reads).toEqual({ groups: 2, membership: phase === "membership" ? 2 : 1 });
  if (newRead) {
    expect(intermediate.scope).toEqual(scope);
    expect(intermediate.loading).toBe(true);
    expect(intermediate.ready).toBe(false);
    expect(result.current.ready).toBe(true);
  } else if (change === "reset") {
    expect(intermediate.scope).toBeNull();
    expect(intermediate.loading).toBe(false);
  }
});

test.each(["schema", "domain", "selector", "counts"] as const)("rejects an invalid $0 descriptor before requesting any rows", async (kind) => {
  const { client } = groupClient();
  const descriptor = groupDescriptor();
  if (kind === "schema") Object.assign(descriptor, { version: 2 });
  if (kind === "domain") Object.assign(descriptor, { domain: "candidates" });
  if (kind === "selector") descriptor.scanId = "foreign-scan";
  if (kind === "counts") descriptor.selectedCount = 1;
  client.get.mockResolvedValueOnce(descriptor);
  const { result } = renderHook(() => useSessionImportGroups(client));
  await act(async () => { expect(await result.current.start(groupScope)).toBe(false); });
  expect(result.current.ready).toBe(false);
  expect(result.current.loading).toBe(false);
  expect(client.groups).not.toHaveBeenCalled();
  expect(result.current.error).toContain(["schema", "domain"].includes(kind) ? "SCHEMA_UNSUPPORTED" : "PLAN_STALE");
});

test("membership reconciliation is bounded and never adopts the inconsistent page", async () => {
  const { client } = groupClient();
  client.membership.mockResolvedValue({ selectionId: "synthetic-selector", stateVersion: 2, choices: [groupChoice("a"), groupChoice("b")] });
  const { result } = renderHook(() => useSessionImportGroups(client));
  await act(async () => { expect(await result.current.start(groupScope)).toBe(false); });
  expect(client.membership).toHaveBeenCalledTimes(3);
  expect(client.get).toHaveBeenCalledTimes(4);
  expect(result.current.ready).toBe(false);
  expect(result.current.choices).toEqual({});
  expect(result.current.error).toContain("PLAN_STALE");
});

test("an acknowledged member change followed by a failed page read is not rolled back", async () => {
  const { client } = groupClient();
  const onMemberChanged = vi.fn();
  const { result } = renderHook(() => useSessionImportGroups(client, { onMemberChanged }));
  await act(async () => { await result.current.start(groupScope); });
  client.membership.mockRejectedValueOnce(new Error("NETWORK: membership unavailable"));
  await act(async () => { expect(await result.current.choose(a, groupCandidate("alternate").candidateId)).toBe(false); });
  expect(onMemberChanged).toHaveBeenCalledTimes(1);
  expect(result.current.selection?.stateVersion).toBe(2);
  expect(result.current.ready).toBe(false);
  expect(result.current.choices).toEqual({});
  expect(result.current.selectionBusy).toBe(false);
  expect(result.current.error).toContain("NETWORK");
  await act(async () => { expect(await result.current.refresh()).toBe(true); });
  expect(result.current.choices[a].candidate.candidateId).toBe(groupCandidate("alternate").candidateId);
});

test("newly discovered groups stay unchecked and an existing preference survives metadata growth", async () => {
  const { client, choices } = groupClient();
  choices.set(a, { ...groupChoice("a", true, "preferred"), choiceVersion: 2 });
  client.get.mockResolvedValue(groupDescriptor(2, 1));
  client.membership.mockImplementation(async (_scan, _epoch, _selector, ids) => ({
    selectionId: "synthetic-selector", stateVersion: 2, choices: ids.map((id) => choices.get(id)!),
  }));
  const { result } = renderHook(() => useSessionImportGroups(client));
  await act(async () => { await result.current.start(groupScope); });
  client.groups.mockResolvedValueOnce({ ...groupPage(["a", "b", "c"]), stateVersion: 2 });
  await act(async () => { expect(await result.current.refreshMetadata()).toBe(true); });
  expect(result.current.choices[groupMetadata("c").groupId].selected).toBe(false);
  expect(result.current.choices[a].candidate.candidateId).toBe(groupCandidate("preferred").candidateId);
  expect(result.current.selection?.selectedCount).toBe(1);
});

test("an empty live discovery page refreshes in place instead of stacking identical cursor pages", async () => {
  const { client } = groupClient();
  client.groups.mockResolvedValueOnce({ ...groupPage([], "signed-offset-zero"), complete: false });
  const { result } = renderHook(() => useSessionImportGroups(client));
  await act(async () => { await result.current.start(groupScope); });
  await act(async () => { expect(await result.current.nextPage()).toBe(false); });
  expect(result.current.pageIndex).toBe(0);
  expect(client.groups).toHaveBeenCalledTimes(1);
  await act(async () => { expect(await result.current.refreshMetadata()).toBe(true); });
  expect(result.current.groups).toHaveLength(2);
});

test.each(["selected-zero", "duplicate-sequence"] as const)("rejects invalid $0 membership", async (kind) => {
  const { client } = groupClient();
  client.get.mockResolvedValueOnce(groupDescriptor(2, 1));
  const choices = [groupChoice("a", true), groupChoice("b")];
  if (kind === "selected-zero") choices[0].choiceVersion = 0;
  else choices[1].groupSeq = choices[0].groupSeq;
  client.membership.mockResolvedValueOnce({ selectionId: "synthetic-selector", stateVersion: 2, choices });
  const { result } = renderHook(() => useSessionImportGroups(client));
  await act(async () => { expect(await result.current.start(groupScope)).toBe(false); });
  expect(result.current.choices).toEqual({});
  expect(result.current.error).toContain("PLAN_STALE");
});

test("a changed member without an advanced descriptor version is rejected before invalidating content", async () => {
  const { client } = groupClient();
  const onMemberChanged = vi.fn();
  const { result } = renderHook(() => useSessionImportGroups(client, { onMemberChanged }));
  await act(async () => { await result.current.start(groupScope); });
  client.choose.mockResolvedValueOnce({ selection: groupDescriptor(), groupId: a,
    previousCandidateId: null, candidateId: groupCandidate("alternate").candidateId, selected: false });
  await act(async () => { expect(await result.current.choose(a, groupCandidate("alternate").candidateId)).toBe(false); });
  expect(onMemberChanged).not.toHaveBeenCalled();
  expect(result.current.choices[a].candidate.candidateId).toBe(groupCandidate("a").candidateId);
});
