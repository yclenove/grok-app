/** Scan groups and separately owned fixed user-member choices. */
import { invoke } from "./host";
import type { ImportProvider, SourceCandidate } from "./sessionMigration";
import type { SourceLocation } from "./sessionMigrationLocations";

export type SourceGroupFilter = { provider?: ImportProvider | null; sourceId?: string | null };
export type SourceGroup = {
  groupId: string; sourceId: string; provider: ImportProvider; host: string;
  nativeSessionId: string; proof: "typed-native" | "artifact";
  firstMember: SourceCandidate; memberCount: number;
};
export type SourceGroupMember = {
  groupId: string;
  format: "cursor-transcript" | "cursor-composer" | "cursor-legacy-chat" | "cursor-store" | "artifact";
  candidate: SourceCandidate; sourceId: string; rootPath: string; legacyId: string;
  location: SourceLocation | null;
};
export type SourceGroupPage = {
  version: 1; groups: SourceGroup[]; nextCursor: string | null;
  totalCount: number; complete: boolean; stateVersion: number;
};
export type SourceGroupMemberPage = {
  version: 1; groupId: string; members: SourceGroupMember[]; nextCursor: string | null;
  memberCount: number; complete: boolean; stateVersion: number;
};

export function sessionImportScanGroups(
  scanId: string, epoch: number,
  options?: { cursor?: string; limit?: number; filter?: SourceGroupFilter },
) {
  return invoke<SourceGroupPage>("session_import_scan_groups", { scanId, epoch, ...options });
}

export function sessionImportScanGroupMembers(
  scanId: string, epoch: number, groupId: string,
  options?: { cursor?: string; limit?: number },
) {
  return invoke<SourceGroupMemberPage>("session_import_scan_group_members", { scanId, epoch, groupId, ...options });
}

/** Exact immutable member metadata; no paging, source body or fallback. */
export function sessionImportScanGroupMember(scanId: string, epoch: number, groupId: string, candidateId: string) {
  return invoke<SourceGroupMember>("session_import_scan_group_member", { scanId, epoch, groupId, candidateId });
}

export type GroupSelection = {
  version: 1; domain: "fixed-groups"; selectionId: string; scanId: string; epoch: number;
  stateVersion: number; selectedCount: number; choiceCount: number;
};
export type GroupChoice = {
  groupId: string; groupSeq: number; candidate: SourceCandidate; selected: boolean; choiceVersion: number;
};
export type GroupSelectionPage = {
  selection: GroupSelection; choices: GroupChoice[]; nextCursor: string | null;
};
export type GroupSelectionMembership = {
  selectionId: string; stateVersion: number; choices: GroupChoice[];
};
export type GroupMemberChange = {
  selection: GroupSelection; groupId: string; previousCandidateId: string | null;
  candidateId: string; selected: boolean;
};
export type GroupSelectionOwner = {
  scanId: string; epoch: number; selectionId: string; expectedStateVersion: number;
};

export function sessionImportGroupSelectionGet(scanId: string, epoch: number) {
  return invoke<GroupSelection>("session_import_group_selection_get", { scanId, epoch });
}
export function sessionImportGroupSelectionFreeze(owner: GroupSelectionOwner) {
  return invoke<GroupSelection>("session_import_group_selection_freeze", { ...owner });
}
export function sessionImportGroupSelectionClear(owner: GroupSelectionOwner) {
  return invoke<GroupSelection>("session_import_group_selection_clear", { ...owner });
}
export function sessionImportGroupSelectionSet(owner: GroupSelectionOwner, groupIds: string[], selected: boolean) {
  return invoke<GroupSelection>("session_import_group_selection_set", { ...owner, groupIds, selected });
}
export function sessionImportGroupSelectionInvert(owner: GroupSelectionOwner, groupIds: string[]) {
  return invoke<GroupSelection>("session_import_group_selection_invert", { ...owner, groupIds });
}
export function sessionImportGroupSelectionChoose(owner: GroupSelectionOwner, groupId: string, candidateId: string) {
  return invoke<GroupMemberChange>("session_import_group_selection_choose", { ...owner, groupId, candidateId });
}
export function sessionImportGroupSelectionPage(
  scanId: string, epoch: number, selectionId: string,
  options?: { cursor?: string; limit?: number; selectedOnly?: boolean },
) {
  return invoke<GroupSelectionPage>("session_import_group_selection_page", { scanId, epoch, selectionId, ...options });
}
export function sessionImportGroupSelectionMembership(scanId: string, epoch: number, selectionId: string, groupIds: string[]) {
  return invoke<GroupSelectionMembership>("session_import_group_selection_membership", { scanId, epoch, selectionId, groupIds });
}
