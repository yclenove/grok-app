import type { SourceGroup, SourceGroupMember, SourceGroupMemberPage } from "@/lib/api/sessionMigrationGroups";
import { GROUP_UI_PAGE_SIZE, groupStateError, validateGroupCandidate, type ImportGroupScope } from "./groupSelectionState";

export type ImportGroupMemberState = {
  scope: ImportGroupScope | null;
  group: SourceGroup | null;
  members: SourceGroupMember[];
  nextCursor: string | null;
  pageIndex: number;
  memberCount: number;
  complete: boolean;
  metadataVersion: number;
  ready: boolean;
  loading: boolean;
  error: string | null;
};
export function emptyImportGroupMembers(): ImportGroupMemberState {
  return { scope: null, group: null, members: [], nextCursor: null, pageIndex: 0,
    memberCount: 0, complete: false, metadataVersion: 0, ready: false, loading: false, error: null };
}
const count = (value: number) => Number.isSafeInteger(value) && value >= 0;
const id = (value: string) => typeof value === "string" && value.length > 0;
const formats = new Set(["cursor-transcript", "cursor-composer", "cursor-legacy-chat", "cursor-store", "artifact"]);

/** DTO checks do not prove body equivalence or authorize a source location. */
export function validateGroupMemberPage(
  scope: ImportGroupScope, group: SourceGroup, page: SourceGroupMemberPage, previous: ImportGroupMemberState,
) {
  if (page.version !== 1) throw new Error("SCHEMA_UNSUPPORTED: source group member metadata");
  if (page.groupId !== group.groupId || !count(page.memberCount) || page.memberCount < group.memberCount ||
    !count(page.stateVersion) || page.stateVersion < previous.metadataVersion ||
    typeof page.complete !== "boolean" || (page.nextCursor !== null && !id(page.nextCursor)) ||
    page.members.length > GROUP_UI_PAGE_SIZE || page.members.length > page.memberCount ||
    (previous.ready && (page.memberCount < previous.memberCount ||
      (page.stateVersion === previous.metadataVersion && page.memberCount !== previous.memberCount)))) {
    groupStateError("invalid or older group member page");
  }
  const candidates = new Set<string>();
  for (const member of page.members) {
    validateGroupCandidate(scope, member.candidate);
    if (member.groupId !== group.groupId || member.sourceId !== group.sourceId ||
      member.candidate.provider !== group.provider || !id(member.rootPath) || !id(member.legacyId) ||
      !formats.has(member.format) || (member.format !== "artifact" && group.provider !== "cursor") ||
      (group.proof === "typed-native" && member.candidate.nativeSessionId !== group.nativeSessionId) ||
      candidates.has(member.candidate.candidateId)) groupStateError("invalid or foreign source group member");
    candidates.add(member.candidate.candidateId);
    const location = member.location;
    if (location && (location.version !== 1 || !id(location.locationId) ||
      location.sourceId !== member.sourceId || location.provider !== group.provider ||
      location.host !== group.host || location.rootPath !== member.rootPath ||
      !["primary", "associated"].includes(location.kind))) groupStateError("foreign source group member location");
    if (!location && group.host !== "local") groupStateError("missing source group member host");
  }
}
