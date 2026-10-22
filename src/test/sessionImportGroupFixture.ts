import { vi } from "vitest";
import type { SourceCandidate } from "@/lib/api/sessionMigration";
import type { GroupChoice, GroupSelection, SourceGroup, SourceGroupFilter, SourceGroupPage } from "@/lib/api/sessionMigrationGroups";

export const groupScope = { scanId: "synthetic-scan", epoch: 1, generation: 1 };
export function groupCandidate(id: string): SourceCandidate {
  return {
    candidateId: `candidate:${groupScope.scanId}:${id}`, provider: "cursor",
    nativeSessionId: `native-${id}`, title: id, locator: `/synthetic/${id}.jsonl`,
    sourceRevision: `revision-${id}`, completeness: "metadata-preview",
  };
}
export function groupMetadata(id: string): SourceGroup {
  return {
    groupId: `group:${groupScope.scanId}:${id}`, sourceId: "synthetic-source",
    provider: "cursor", host: "local", nativeSessionId: `native-${id}`,
    proof: "typed-native", firstMember: groupCandidate(id), memberCount: 2,
  };
}
export function groupDescriptor(stateVersion = 1, selectedCount = 0): GroupSelection {
  return {
    version: 1, domain: "fixed-groups", selectionId: "synthetic-selector",
    scanId: groupScope.scanId, epoch: groupScope.epoch, stateVersion,
    selectedCount, choiceCount: selectedCount,
  };
}
export function groupChoice(id: string, selected = false, memberId = id): GroupChoice {
  return {
    groupId: groupMetadata(id).groupId, groupSeq: ["a", "b", "c"].indexOf(id) + 1,
    candidate: { ...groupCandidate(memberId), nativeSessionId: `native-${id}` }, selected, choiceVersion: selected ? 2 : 0,
  };
}
export function groupPage(ids = ["a", "b"], nextCursor: string | null = null): SourceGroupPage {
  return {
    version: 1, groups: ids.map(groupMetadata), nextCursor,
    totalCount: ids.length, complete: true, stateVersion: 1,
  };
}
export function groupClient() {
  const choices = new Map(["a", "b", "c"].map((id) => [groupMetadata(id).groupId, groupChoice(id)]));
  let descriptor = groupDescriptor();
  const update = () => {
    descriptor = {
      ...descriptor, stateVersion: descriptor.stateVersion + 1,
      selectedCount: [...choices.values()].filter((choice) => choice.selected).length,
      choiceCount: [...choices.values()].filter((choice) => choice.choiceVersion > 0).length,
    };
    return descriptor;
  };
  const client = {
    get: vi.fn(async (_scan: string, _epoch: number) => descriptor),
    groups: vi.fn(async (_scan: string, _epoch: number, _options?: { cursor?: string; limit?: number; filter?: SourceGroupFilter }) => groupPage()),
    membership: vi.fn(async (_scan: string, _epoch: number, _selector: string, ids: string[]) => ({
      selectionId: descriptor.selectionId, stateVersion: descriptor.stateVersion,
      choices: ids.map((id) => choices.get(id)!),
    })),
    freeze: vi.fn(async (_owner: unknown) => {
      for (const id of ["a", "b"]) choices.set(groupMetadata(id).groupId, groupChoice(id, true));
      return update();
    }),
    clear: vi.fn(async (_owner: unknown) => {
      for (const [id, choice] of choices) choices.set(id, { ...choice,
        candidate: groupCandidate(id.split(":").at(-1)!), selected: false, choiceVersion: 0 });
      return update();
    }),
    set: vi.fn(async (_owner: unknown, ids: string[], selected: boolean) => {
      for (const id of ids) choices.set(id, { ...choices.get(id)!, selected, choiceVersion: 2 });
      return update();
    }),
    invert: vi.fn(async (_owner: unknown, ids: string[]) => {
      for (const id of ids) {
        const choice = choices.get(id)!;
        choices.set(id, { ...choice, selected: !choice.selected, choiceVersion: 2 });
      }
      return update();
    }),
    choose: vi.fn(async (_owner: unknown, id: string, candidateId: string) => {
      const choice = choices.get(id)!;
      const previousCandidateId = choice.choiceVersion ? choice.candidate.candidateId : null;
      choices.set(id, { ...choice, candidate: { ...groupCandidate(candidateId.split(":").at(-1)!),
        nativeSessionId: choice.candidate.nativeSessionId }, choiceVersion: 2 });
      return { selection: update(), groupId: id, previousCandidateId, candidateId, selected: choice.selected };
    }),
  };
  return { client, choices };
}

export function groupDeferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
