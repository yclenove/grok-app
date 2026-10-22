import type { CandidateSelection, PrepareOverride, SourceCandidate } from "@/lib/api/sessionMigration";
import type { GroupSelection, SourceGroup } from "@/lib/api/sessionMigrationGroups";
import { GROUP_UI_PAGE_SIZE, groupStateError, validateGroupDescriptor, validateGroupMembership, type ImportGroupScope, type ImportGroupState } from "./groupSelectionState";
import { sessionImportGroupSelectionMembership } from "@/lib/api/sessionMigrationGroups";
import { sessionImportScanSelectionMembership } from "@/lib/api/sessionMigration";

export type ImportSelectionRow = { rowId: string; candidate: SourceCandidate; selected: boolean; group?: SourceGroup };
export type ImportSelectionView =
  | { domain: "fixed-groups"; scope: ImportGroupScope; descriptor: GroupSelection | null; ready: boolean }
  | { domain: "independent-candidates"; scope: ImportGroupScope; descriptor: CandidateSelection | null; ready: boolean };
export const IMPORT_OVERRIDE_LIMIT = 200;
export function groupRows(state: ImportGroupState): ImportSelectionRow[] {
  return state.ready ? state.groups.map((group) => ({ rowId: group.groupId, group,
    candidate: state.choices[group.groupId].candidate, selected: state.choices[group.groupId].selected })) : [];
}
export function candidateRows(candidates: SourceCandidate[], selected: Set<string>): ImportSelectionRow[] {
  return candidates.map((candidate) => ({ rowId: candidate.candidateId, candidate, selected: selected.has(candidate.candidateId) }));
}
export type PreviewGroupBinding = { scope: ImportGroupScope; group: SourceGroup };
export type OverrideMembershipClient = {
  groups: typeof sessionImportGroupSelectionMembership;
  candidates: typeof sessionImportScanSelectionMembership;
};
const liveMembership: OverrideMembershipClient = {
  groups: sessionImportGroupSelectionMembership, candidates: sessionImportScanSelectionMembership,
};

/** Check only retained overrides, not the potentially 10k Host-owned selection. */
export async function selectedOverrides(
  view: ImportSelectionView, slots: PrepareOverride[], bindings: Map<string, PreviewGroupBinding>,
  client: OverrideMembershipClient = liveMembership,
  isCurrent: () => boolean = () => true,
): Promise<PrepareOverride[]> {
  const assertCurrent = () => { if (!isCurrent()) groupStateError("content verification owner expired"); };
  assertCurrent();
  const descriptor = view.descriptor;
  if (!view.ready || !descriptor || descriptor.scanId !== view.scope.scanId || descriptor.epoch !== view.scope.epoch) {
    groupStateError("active selection is not ready for preparation");
  }
  if (slots.length > IMPORT_OVERRIDE_LIMIT) throw new Error("RESOURCE_LIMIT: prepare overrides exceed one bounded page");
  if (new Set(slots.map((slot) => slot.candidateId)).size !== slots.length ||
    slots.some((slot) => !slot.candidateId || !slot.previewId || !slot.leafId)) groupStateError("invalid content override identities");
  const accepted = new Set<string>();
  if (view.domain === "fixed-groups") {
    const groupDescriptor = view.descriptor!;
    validateGroupDescriptor(view.scope, groupDescriptor);
    const groups = new Map<string, SourceGroup>();
    for (const slot of slots) {
      const binding = bindings.get(slot.candidateId);
      if (!binding || binding.scope.scanId !== view.scope.scanId || binding.scope.epoch !== view.scope.epoch ||
        binding.scope.generation !== view.scope.generation) groupStateError("content override has no current group owner");
      groups.set(binding.group.groupId, binding.group);
    }
    const metadata = [...groups.values()];
    for (let offset = 0; offset < metadata.length; offset += GROUP_UI_PAGE_SIZE) {
      assertCurrent();
      const page = metadata.slice(offset, offset + GROUP_UI_PAGE_SIZE);
      const membership = await client.groups(view.scope.scanId, view.scope.epoch, descriptor.selectionId, page.map((group) => group.groupId));
      assertCurrent();
      const choices = validateGroupMembership(view.scope, groupDescriptor, page, membership);
      for (const choice of Object.values(choices)) if (choice.selected) accepted.add(choice.candidate.candidateId);
    }
  } else {
    for (let offset = 0; offset < slots.length; offset += GROUP_UI_PAGE_SIZE) {
      assertCurrent();
      const ids = slots.slice(offset, offset + GROUP_UI_PAGE_SIZE).map((slot) => slot.candidateId);
      const membership = await client.candidates(view.scope.scanId, view.scope.epoch, descriptor.selectionId, ids);
      assertCurrent();
      if (membership.selectionId !== descriptor.selectionId || membership.stateVersion !== descriptor.stateVersion ||
        new Set(membership.selectedIds).size !== membership.selectedIds.length || membership.selectedIds.some((id) => !ids.includes(id))) {
        groupStateError("candidate override membership changed or is foreign");
      }
      membership.selectedIds.forEach((id) => accepted.add(id));
    }
  }
  assertCurrent();
  return slots.filter((slot) => accepted.has(slot.candidateId));
}
