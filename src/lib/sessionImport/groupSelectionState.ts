import type { SourceCandidate } from "@/lib/api/sessionMigration";
import type {
  GroupChoice, GroupSelection, GroupSelectionMembership, SourceGroup,
  SourceGroupFilter, SourceGroupPage,
} from "@/lib/api/sessionMigrationGroups";

export type ImportGroupScope = { scanId: string; epoch: number; generation: number };
export type ImportGroupState = {
  scope: ImportGroupScope | null;
  selection: GroupSelection | null;
  groups: SourceGroup[];
  choices: Record<string, GroupChoice>;
  filter: SourceGroupFilter;
  nextCursor: string | null;
  pageIndex: number;
  totalCount: number;
  complete: boolean;
  metadataVersion: number;
  ready: boolean;
  loading: boolean;
  selectionBusy: boolean;
  error: string | null;
};

export const GROUP_UI_PAGE_SIZE = 50;
export function emptyImportGroups(): ImportGroupState {
  return {
    scope: null, selection: null, groups: [], choices: {}, filter: {},
    nextCursor: null, pageIndex: 0, totalCount: 0, complete: false, metadataVersion: 0,
    ready: false, loading: false, selectionBusy: false, error: null,
  };
}

export function groupStateError(detail: string): never {
  throw new Error(`PLAN_STALE: ${detail}`);
}
const count = (value: number) => Number.isSafeInteger(value) && value >= 0;
const id = (value: string) => typeof value === "string" && value.length > 0;

export function validateGroupScope(scope: ImportGroupScope) {
  if (!id(scope.scanId) || !count(scope.epoch) || scope.epoch < 1 || !count(scope.generation)) {
    groupStateError("invalid group scan owner");
  }
}

export function validateGroupDescriptor(
  scope: ImportGroupScope, selection: GroupSelection, previous?: GroupSelection | null,
) {
  if (selection.version !== 1 || selection.domain !== "fixed-groups") {
    throw new Error("SCHEMA_UNSUPPORTED: fixed group selection domain");
  }
  if (selection.scanId !== scope.scanId || selection.epoch !== scope.epoch ||
    !id(selection.selectionId) || !count(selection.stateVersion) ||
    !count(selection.selectedCount) || !count(selection.choiceCount) ||
    selection.selectedCount > selection.choiceCount) {
    groupStateError("invalid group selection descriptor");
  }
  if (previous && (selection.selectionId !== previous.selectionId ||
    selection.stateVersion < previous.stateVersion)) {
    groupStateError("group selector changed or returned an older version");
  }
  if (previous?.stateVersion === selection.stateVersion &&
    (previous.selectedCount !== selection.selectedCount || previous.choiceCount !== selection.choiceCount)) {
    groupStateError("group counts changed without a selection version");
  }
}

export function validateGroupCandidateId(scope: ImportGroupScope, candidateId: string) {
  if (!id(candidateId) || !candidateId.startsWith(`candidate:${scope.scanId}:`)) {
    groupStateError("invalid or foreign group candidate identifier");
  }
}

export function validateGroupCandidate(scope: ImportGroupScope, candidate: SourceCandidate) {
  validateGroupCandidateId(scope, candidate.candidateId);
  if (
    !id(candidate.sourceRevision) || !id(candidate.provider) || typeof candidate.title !== "string" ||
    !id(candidate.nativeSessionId) || typeof candidate.locator !== "string") {
    groupStateError("invalid or foreign group candidate");
  }
}

export function validateGroupIds(ids: string[], known?: Set<string>) {
  if (ids.length > GROUP_UI_PAGE_SIZE || new Set(ids).size !== ids.length ||
    ids.some((value) => !id(value) || (known && !known.has(value)))) {
    groupStateError("duplicate, foreign or oversized group identifiers");
  }
}

export function validateGroupPage(scope: ImportGroupScope, page: SourceGroupPage, filter: SourceGroupFilter = {}) {
  if (page.version !== 1 || !count(page.totalCount) || !count(page.stateVersion) ||
    typeof page.complete !== "boolean" || (page.nextCursor !== null && !id(page.nextCursor))) {
    groupStateError("invalid group metadata page");
  }
  validateGroupIds(page.groups.map((group) => group.groupId));
  if (page.groups.length > page.totalCount) groupStateError("group page exceeds its count");
  const candidates = new Set<string>();
  for (const group of page.groups) {
    if (!group.groupId.startsWith(`group:${scope.scanId}:`) || !id(group.sourceId) ||
      !count(group.memberCount) || group.memberCount < 1 ||
      !["typed-native", "artifact"].includes(group.proof) || !id(group.host) || !id(group.nativeSessionId) ||
      group.provider !== group.firstMember.provider ||
      (group.proof === "typed-native" && group.nativeSessionId !== group.firstMember.nativeSessionId) ||
      candidates.has(group.firstMember.candidateId) ||
      (filter.provider != null && group.provider !== filter.provider) ||
      (filter.sourceId != null && group.sourceId !== filter.sourceId)) groupStateError("invalid or foreign source group");
    validateGroupCandidate(scope, group.firstMember);
    candidates.add(group.firstMember.candidateId);
  }
}

export function validateGroupMembership(
  scope: ImportGroupScope, selection: GroupSelection, groups: SourceGroup[],
  membership: GroupSelectionMembership,
  previous?: Record<string, GroupChoice>,
): Record<string, GroupChoice> {
  if (membership.selectionId !== selection.selectionId || membership.stateVersion !== selection.stateVersion) {
    groupStateError("group membership version does not match its descriptor");
  }
  const known = new Map(groups.map((group) => [group.groupId, group]));
  validateGroupIds(membership.choices.map((choice) => choice.groupId), new Set(known.keys()));
  if (membership.choices.length !== groups.length) groupStateError("group membership is incomplete");
  const choices: Record<string, GroupChoice> = {};
  const sequences = new Set<number>();
  const candidates = new Set<string>();
  for (const choice of membership.choices) {
    validateGroupCandidate(scope, choice.candidate);
    if (!count(choice.groupSeq) || choice.groupSeq < 1 || !count(choice.choiceVersion) ||
      choice.choiceVersion > selection.stateVersion || typeof choice.selected !== "boolean" ||
      (choice.selected && choice.choiceVersion === 0) || sequences.has(choice.groupSeq) ||
      (choice.choiceVersion === 0 && (choice.candidate.candidateId !== known.get(choice.groupId)!.firstMember.candidateId ||
        choice.candidate.sourceRevision !== known.get(choice.groupId)!.firstMember.sourceRevision)) ||
      candidates.has(choice.candidate.candidateId) ||
      choice.candidate.provider !== known.get(choice.groupId)!.provider ||
      (known.get(choice.groupId)!.proof === "typed-native" &&
        choice.candidate.nativeSessionId !== known.get(choice.groupId)!.nativeSessionId)) {
      groupStateError("invalid group member choice");
    }
    const prior = previous?.[choice.groupId];
    if (prior && (prior.groupSeq !== choice.groupSeq || prior.choiceVersion !== choice.choiceVersion ||
      prior.selected !== choice.selected || prior.candidate.candidateId !== choice.candidate.candidateId ||
      prior.candidate.sourceRevision !== choice.candidate.sourceRevision)) {
      groupStateError("group membership changed without a selection version");
    }
    sequences.add(choice.groupSeq);
    candidates.add(choice.candidate.candidateId);
    choices[choice.groupId] = choice;
  }
  if (membership.choices.filter((choice) => choice.selected).length > selection.selectedCount ||
    membership.choices.filter((choice) => choice.choiceVersion > 0).length > selection.choiceCount) {
    groupStateError("group membership exceeds descriptor counts");
  }
  return choices;
}

export function sameGroupIds(groups: SourceGroup[], other: SourceGroup[]) {
  return groups.length === other.length && groups.every((group, index) => group.groupId === other[index].groupId);
}
