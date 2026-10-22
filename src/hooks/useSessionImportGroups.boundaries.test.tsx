/** @vitest-environment jsdom */
import { StrictMode, type PropsWithChildren } from "react";
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { GroupMemberChange } from "@/lib/api/sessionMigrationGroups";
import { groupCandidate, groupClient, groupDeferred, groupDescriptor, groupMetadata, groupScope } from "@/test/sessionImportGroupFixture";
import { useSessionImportGroups } from "./useSessionImportGroups";

afterEach(cleanup);
const a = groupMetadata("a").groupId;
test("a clear acknowledgement with retained choices cannot invalidate previews", async () => {
  const { client } = groupClient();
  const onCleared = vi.fn();
  const { result } = renderHook(() => useSessionImportGroups(client, { onCleared }));
  await act(async () => { await result.current.start(groupScope);
    await result.current.choose(a, groupCandidate("alternate").candidateId); });
  const prior = result.current.selection;
  client.clear.mockResolvedValueOnce({ ...prior!, stateVersion: prior!.stateVersion + 1 });
  await act(async () => { expect(await result.current.clear()).toBe(false); });
  expect(onCleared).not.toHaveBeenCalled();
  expect(result.current.selection).toEqual(prior);
  expect(result.current.choices[a].candidate.candidateId).toBe(groupCandidate("alternate").candidateId);
  expect(result.current.error).toContain("PLAN_STALE");
});

test.each(["member", "selected", "choice-version", "sequence"] as const)("same-version refresh cannot change signed %s membership", async (kind) => {
  const { client, choices } = groupClient();
  const onChoicesAccepted = vi.fn();
  const { result } = renderHook(() => useSessionImportGroups(client, { onChoicesAccepted }));
  await act(async () => { await result.current.start(groupScope); await result.current.setSelected([a], true); });
  const prior = result.current.choices[a];
  const changed = { ...prior };
  if (kind === "member") changed.candidate = { ...groupCandidate("alternate"), nativeSessionId: prior.candidate.nativeSessionId };
  if (kind === "selected") changed.selected = false;
  if (kind === "choice-version") changed.choiceVersion = 1;
  if (kind === "sequence") changed.groupSeq = 3;
  choices.set(a, changed);
  onChoicesAccepted.mockClear();
  await act(async () => { expect(await result.current.refresh()).toBe(false); });
  expect(result.current.choices[a]).toEqual(prior);
  expect(onChoicesAccepted).not.toHaveBeenCalled();
  expect(result.current.error).toContain("PLAN_STALE");
});
test("same-turn choosing two different members sends only the first CAS", async () => {
  const { client } = groupClient();
  const pending = groupDeferred<GroupMemberChange>();
  const onMemberChanged = vi.fn();
  const { result } = renderHook(() => useSessionImportGroups(client, { onMemberChanged }));
  await act(async () => { await result.current.start(groupScope); });
  client.choose.mockReturnValueOnce(pending.promise);
  let first!: Promise<boolean>;
  let second!: Promise<boolean>;
  act(() => { first = result.current.choose(a, groupCandidate("alternate").candidateId);
    second = result.current.choose(a, groupCandidate("other").candidateId); });
  await act(async () => {
    pending.resolve({ selection: groupDescriptor(), groupId: a, previousCandidateId: null,
      candidateId: groupCandidate("alternate").candidateId, selected: false });
    await first; await second;
  });
  expect(client.choose).toHaveBeenCalledTimes(1);
  expect(await second).toBe(false);
  expect(onMemberChanged).not.toHaveBeenCalled();
  expect(result.current.selectionBusy).toBe(false);
});

test.each(["group", "candidate", "previous", "selected"] as const)("invalid $0 member response keeps prior content and choice", async (kind) => {
  const { client } = groupClient();
  const onMemberChanged = vi.fn();
  const { result } = renderHook(() => useSessionImportGroups(client, { onMemberChanged }));
  await act(async () => { await result.current.start(groupScope); });
  const response: GroupMemberChange = { selection: { ...groupDescriptor(2), choiceCount: 1 }, groupId: a,
    previousCandidateId: groupCandidate("a").candidateId, candidateId: groupCandidate("alternate").candidateId, selected: false };
  if (kind === "group") response.groupId = groupMetadata("b").groupId;
  if (kind === "candidate") response.candidateId = groupCandidate("other").candidateId;
  if (kind === "previous") response.previousCandidateId = groupCandidate("other").candidateId;
  if (kind === "selected") response.selected = true;
  client.choose.mockResolvedValueOnce(response);
  await act(async () => { expect(await result.current.choose(a, groupCandidate("alternate").candidateId)).toBe(false); });
  expect(result.current.choices[a].candidate.candidateId).toBe(groupCandidate("a").candidateId);
  expect(result.current.selection?.stateVersion).toBe(1);
  expect(result.current.selectionBusy).toBe(false);
  expect(result.current.error).toContain("PLAN_STALE");
  expect(onMemberChanged).not.toHaveBeenCalled();
});

test("StrictMode replay permits initialization; unmount discards an old member-change success", async () => {
  const { client } = groupClient();
  const pending = groupDeferred<GroupMemberChange>();
  const onMemberChanged = vi.fn();
  const onError = vi.fn();
  const { result, unmount } = renderHook(() => useSessionImportGroups(client, { onMemberChanged, onError }), {
    wrapper: ({ children }: PropsWithChildren) => <StrictMode>{children}</StrictMode>,
  });
  await act(async () => { expect(await result.current.start(groupScope)).toBe(true); });
  client.choose.mockReturnValueOnce(pending.promise);
  let old!: Promise<boolean>;
  act(() => { old = result.current.choose(a, groupCandidate("alternate").candidateId); });
  unmount();
  await act(async () => { pending.resolve({ selection: { ...groupDescriptor(2), choiceCount: 1 }, groupId: a,
    previousCandidateId: null, candidateId: groupCandidate("alternate").candidateId, selected: false });
    expect(await old).toBe(false); });
  expect(onMemberChanged).not.toHaveBeenCalled();
  expect(onError).not.toHaveBeenCalled();
  expect(client.groups).toHaveBeenCalledTimes(1);
});
