/** @vitest-environment jsdom */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { GroupMemberChange, GroupSelectionMembership } from "@/lib/api/sessionMigrationGroups";
import { groupCandidate, groupChoice, groupClient, groupDeferred, groupDescriptor, groupMetadata, groupPage, groupScope } from "@/test/sessionImportGroupFixture";
import { useSessionImportGroups, type GroupChooseAcknowledgement } from "./useSessionImportGroups";

afterEach(cleanup);
const groupId = groupMetadata("a").groupId, candidateId = groupCandidate("alternate").candidateId;
test("exposes authenticated choose acknowledgement separately from later membership-read completion", async () => {
  const { client } = groupClient(), { result } = renderHook(() => useSessionImportGroups(client));
  await act(async () => { expect(await result.current.start(groupScope)).toBe(true); });
  expect(typeof result.current.chooseAcknowledged).toBe("function");
});

test.each(["groups", "membership"])("retains the authenticated ACK when subsequent %s read fails without rollback or repeat CAS", async (failure) => {
  const { client } = groupClient(), onAcknowledged = vi.fn(), onMemberChanged = vi.fn();
  const { result } = renderHook(() => useSessionImportGroups(client, { onMemberChanged }));
  await act(async () => { await result.current.start(groupScope); await result.current.setSelected([groupId], true); });
  if (failure === "groups") client.groups.mockRejectedValueOnce(new Error("PAGE_FAILED: synthetic"));
  else client.membership.mockRejectedValueOnce(new Error("MEMBERSHIP_FAILED: synthetic"));
  let outcome!: GroupChooseAcknowledgement;
  await act(async () => { outcome = await result.current.chooseAcknowledged(groupId, candidateId, onAcknowledged); });
  expect(outcome.reconciled).toBe(false);
  expect(outcome.acknowledgement).toMatchObject({ groupId, candidateId, selected: true,
    previousCandidateId: groupCandidate("a").candidateId, selection: { stateVersion: 3 } });
  expect(result.current.selection?.stateVersion).toBe(3);
  expect(result.current.ready).toBe(false);
  expect(result.current.selectionBusy).toBe(false);
  expect(onAcknowledged).toHaveBeenCalledTimes(1);
  expect(onMemberChanged).toHaveBeenCalledTimes(1);
  expect(client.choose).toHaveBeenCalledTimes(1);
  expect(client.choose.mock.calls[0][0]).toMatchObject({ expectedStateVersion: 2 });
  await act(async () => { expect(await result.current.refresh()).toBe(true); });
  expect(result.current.choices[groupId].candidate.candidateId).toBe(candidateId);
  expect(result.current.choices[groupId].selected).toBe(true);
  expect(client.choose).toHaveBeenCalledTimes(1);
});

test("CAS failure is distinct from ACK plus read failure", async () => {
  const { client } = groupClient(), onAcknowledged = vi.fn(), onMemberChanged = vi.fn();
  const { result } = renderHook(() => useSessionImportGroups(client, { onMemberChanged }));
  await act(async () => { await result.current.start(groupScope); });
  const prior = result.current.choices[groupId], calls = client.groups.mock.calls.length;
  client.choose.mockRejectedValueOnce(new Error("PLAN_STALE: CAS rejected"));
  await act(async () => { expect(await result.current.chooseAcknowledged(groupId, candidateId, onAcknowledged))
    .toEqual({ acknowledgement: null, reconciled: false }); });
  expect(onAcknowledged).not.toHaveBeenCalled();
  expect(onMemberChanged).not.toHaveBeenCalled();
  expect(client.groups).toHaveBeenCalledTimes(calls);
  expect(result.current.choices[groupId]).toBe(prior);
  expect(result.current.ready).toBe(true);
});

test.each(["candidate", "group", "selected", "selector", "version"])("foreign %s ACK cannot be exposed or invalidate content", async (field) => {
  const { client } = groupClient(), onAcknowledged = vi.fn(), onMemberChanged = vi.fn();
  const { result } = renderHook(() => useSessionImportGroups(client, { onMemberChanged }));
  await act(async () => { await result.current.start(groupScope); });
  const ack: GroupMemberChange = { selection: groupDescriptor(2), groupId, candidateId, selected: false, previousCandidateId: null };
  if (field === "candidate") ack.candidateId = "foreign";
  if (field === "group") ack.groupId = "foreign";
  if (field === "selected") ack.selected = true;
  if (field === "selector") ack.selection.selectionId = "foreign";
  if (field === "version") ack.selection.stateVersion = 0;
  client.choose.mockResolvedValueOnce(ack);
  await act(async () => { expect(await result.current.chooseAcknowledged(groupId, candidateId, onAcknowledged))
    .toEqual({ acknowledgement: null, reconciled: false }); });
  expect(onAcknowledged).not.toHaveBeenCalled();
  expect(onMemberChanged).not.toHaveBeenCalled();
  expect(result.current.selection?.stateVersion).toBe(1);
});

test("same-member no-op ACK remains observable without invalidating retained content", async () => {
  const { client } = groupClient(), onAcknowledged = vi.fn(), onMemberChanged = vi.fn();
  const { result } = renderHook(() => useSessionImportGroups(client, { onMemberChanged }));
  await act(async () => { await result.current.start(groupScope); });
  const sameCandidate = groupCandidate("a").candidateId;
  client.choose.mockResolvedValueOnce({ selection: groupDescriptor(), groupId, candidateId: sameCandidate,
    selected: false, previousCandidateId: sameCandidate });
  await act(async () => {
    const outcome = await result.current.chooseAcknowledged(groupId, sameCandidate, onAcknowledged);
    expect(outcome.reconciled).toBe(true);
    expect(outcome.acknowledgement?.selection.stateVersion).toBe(1);
  });
  expect(onAcknowledged).toHaveBeenCalledTimes(1);
  expect(onMemberChanged).not.toHaveBeenCalled();
});

test("one same-turn choice reserves CAS for both detailed and ordinary callers", async () => {
  const { client } = groupClient(), pending = groupDeferred<GroupMemberChange>(), onAcknowledged = vi.fn();
  const { result } = renderHook(() => useSessionImportGroups(client));
  await act(async () => { await result.current.start(groupScope); });
  client.choose.mockReturnValueOnce(pending.promise);
  let first!: Promise<GroupChooseAcknowledgement>, duplicate!: Promise<GroupChooseAcknowledgement>, ordinary!: Promise<boolean>;
  act(() => {
    first = result.current.chooseAcknowledged(groupId, candidateId, onAcknowledged);
    duplicate = result.current.chooseAcknowledged(groupId, candidateId, onAcknowledged);
    ordinary = result.current.choose(groupId, candidateId);
  });
  expect(await duplicate).toEqual({ acknowledgement: null, reconciled: false });
  expect(await ordinary).toBe(false);
  expect(client.choose).toHaveBeenCalledTimes(1);
  client.membership.mockResolvedValueOnce({ selectionId: "synthetic-selector", stateVersion: 2,
    choices: [{ ...groupChoice("a", false, "alternate"), choiceVersion: 2 }, groupChoice("b")] });
  await act(async () => { pending.resolve({ selection: { ...groupDescriptor(2), choiceCount: 1 }, groupId, candidateId,
    selected: false, previousCandidateId: null }); expect((await first).reconciled).toBe(true); });
  expect(onAcknowledged).toHaveBeenCalledTimes(1);
});

test.each(["reset", "unmount"])("owner %s before ACK receipt cannot expose obsolete response or callback", async (exit) => {
  const { client } = groupClient(), pending = groupDeferred<GroupMemberChange>(), onAcknowledged = vi.fn();
  const { result, unmount } = renderHook(() => useSessionImportGroups(client));
  await act(async () => { await result.current.start(groupScope); });
  client.choose.mockReturnValueOnce(pending.promise);
  let flight!: Promise<GroupChooseAcknowledgement>;
  act(() => { flight = result.current.chooseAcknowledged(groupId, candidateId, onAcknowledged); });
  if (exit === "unmount") unmount();
  else act(() => { result.current.reset(); });
  await act(async () => { pending.resolve({ selection: { ...groupDescriptor(2), choiceCount: 1 }, groupId,
    candidateId, selected: false, previousCandidateId: null });
    expect(await flight).toEqual({ acknowledgement: null, reconciled: false }); });
  expect(onAcknowledged).not.toHaveBeenCalled();
  expect(client.membership).toHaveBeenCalledTimes(1);
});

test.each(["reset", "throw"])("ACK callback %s keeps the captured fact but prevents subsequent member/read work", async (exit) => {
  const { client } = groupClient(), onMemberChanged = vi.fn();
  const { result } = renderHook(() => useSessionImportGroups(client, { onMemberChanged }));
  await act(async () => { await result.current.start(groupScope); });
  let outcome!: GroupChooseAcknowledgement;
  await act(async () => { outcome = await result.current.chooseAcknowledged(groupId, candidateId, () => {
    if (exit === "reset") result.current.reset();
    else throw new Error("ACK_CALLBACK_FAILED: synthetic");
  }); });
  expect(outcome.acknowledgement?.candidateId).toBe(candidateId);
  expect(outcome.reconciled).toBe(false);
  expect(onMemberChanged).not.toHaveBeenCalled();
  expect(client.groups).toHaveBeenCalledTimes(1);
  expect(client.choose).toHaveBeenCalledTimes(1);
});

test("ACK callback and returned result cannot mutate the accepted live descriptor or each other", async () => {
  const { client } = groupClient(), onMemberChanged = vi.fn();
  const { result } = renderHook(() => useSessionImportGroups(client, { onMemberChanged }));
  await act(async () => { await result.current.start(groupScope); });
  let outcome!: GroupChooseAcknowledgement;
  await act(async () => { outcome = await result.current.chooseAcknowledged(groupId, candidateId, (ack) => {
    ack.selection.stateVersion = 99;
    ack.candidateId = "mutated-callback";
  }); });
  expect(outcome.reconciled).toBe(true);
  expect(outcome.acknowledgement?.candidateId).toBe(candidateId);
  expect(onMemberChanged.mock.calls[0][0].selection.stateVersion).toBe(2);
  outcome.acknowledgement!.selection.stateVersion = 100;
  expect(result.current.selection?.stateVersion).toBe(2);
  expect(result.current.choices[groupId].candidate.candidateId).toBe(candidateId);
});

test("a coherent page advanced by another writer is distinguishable from the original accepted ACK", async () => {
  const { client } = groupClient(), { result } = renderHook(() => useSessionImportGroups(client));
  await act(async () => { await result.current.start(groupScope); });
  const newer: GroupSelectionMembership = { selectionId: "synthetic-selector", stateVersion: 3,
    choices: [{ ...groupChoice("a", false, "other-writer"), choiceVersion: 3 }, groupChoice("b")] };
  client.membership.mockResolvedValue(newer);
  client.get.mockResolvedValue({ ...groupDescriptor(3), choiceCount: 1 });
  let outcome!: GroupChooseAcknowledgement;
  await act(async () => { outcome = await result.current.chooseAcknowledged(groupId, candidateId); });
  expect(outcome.acknowledgement?.selection.stateVersion).toBe(2);
  expect(outcome.acknowledgement?.candidateId).toBe(candidateId);
  expect(outcome.reconciled).toBe(true);
  expect(result.current.error).toBeNull();
  expect(result.current.selection?.stateVersion).toBe(3);
  expect(result.current.choices[groupId].candidate.candidateId).toBe(groupCandidate("other-writer").candidateId);
  expect(client.choose).toHaveBeenCalledTimes(1);
});

test("reset after authenticated ACK but before membership completion retains fact without resurrecting ownership", async () => {
  const { client } = groupClient(), pending = groupDeferred<ReturnType<typeof groupPage>>(), onAcknowledged = vi.fn();
  const { result } = renderHook(() => useSessionImportGroups(client));
  await act(async () => { await result.current.start(groupScope); });
  client.groups.mockReturnValueOnce(pending.promise);
  let flight!: Promise<GroupChooseAcknowledgement>;
  await act(async () => { flight = result.current.chooseAcknowledged(groupId, candidateId, onAcknowledged); await Promise.resolve(); });
  expect(onAcknowledged).toHaveBeenCalledTimes(1);
  act(() => { result.current.reset(); });
  await act(async () => { pending.resolve(groupPage());
    const outcome = await flight;
    expect(outcome.acknowledgement?.candidateId).toBe(candidateId);
    expect(outcome.reconciled).toBe(false);
  });
  expect(result.current.scope).toBeNull();
  expect(result.current.ready).toBe(false);
  expect(result.current.selectionBusy).toBe(false);
});
