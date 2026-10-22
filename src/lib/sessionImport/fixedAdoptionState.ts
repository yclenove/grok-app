import type { GroupMemberChange, GroupSelection, SourceGroup } from "@/lib/api/sessionMigrationGroups";
import type { SourceCandidate } from "@/lib/api/sessionMigration";
import { copyFixedPreviewTransfer, fixedScopeKey, type FixedPreviewTransferRequest } from "./fixedPreviewTransfer";
import { groupStateError, validateGroupDescriptor, type ImportGroupState } from "./groupSelectionState";

export type FixedAdoptionCapture = {
  request: FixedPreviewTransferRequest;
  descriptor: GroupSelection;
  selected: boolean;
  groupSeq: number;
  choiceVersion: number;
};
export type FixedAdoptionPhase = "idle" | "adopting" | "adopted" | "failed" | "reconcile-required" | "uncertain" | "stale";
export type FixedAdoptionStatus = { phase: FixedAdoptionPhase; busy: boolean; error: string | null };
export function sameFixedDescriptor(left: GroupSelection | null, right: GroupSelection): boolean {
  return !!left && left.version === right.version && left.domain === right.domain &&
    left.scanId === right.scanId && left.epoch === right.epoch && left.selectionId === right.selectionId &&
    left.stateVersion === right.stateVersion && left.selectedCount === right.selectedCount && left.choiceCount === right.choiceCount;
}
function candidateIdentity(candidate: SourceCandidate): string {
  return JSON.stringify([candidate.candidateId, candidate.sourceRevision, candidate.provider, candidate.nativeSessionId]);
}
function groupIdentity(group: SourceGroup): string {
  return JSON.stringify([group.groupId, group.sourceId, group.provider, group.host, group.nativeSessionId,
    group.proof, group.memberCount, candidateIdentity(group.firstMember)]);
}
function assertContext(state: ImportGroupState, capture: FixedAdoptionCapture) {
  const { context } = capture.request;
  const group = state.groups.find((item) => item.groupId === context.group.groupId);
  if (!state.ready || state.loading || fixedScopeKey(state.scope) !== fixedScopeKey(context.scope) ||
    !group || groupIdentity(group) !== groupIdentity(context.group)) groupStateError("fixed adoption group owner changed");
  return state.choices[context.group.groupId];
}
export function captureFixedAdoption(state: ImportGroupState, input: FixedPreviewTransferRequest): FixedAdoptionCapture {
  const request = copyFixedPreviewTransfer(input);
  if (!state.selection || state.selectionBusy) groupStateError("fixed adoption selection is unavailable");
  validateGroupDescriptor(request.context.scope, state.selection);
  const choice = state.choices[request.context.group.groupId];
  if (!choice) groupStateError("fixed adoption primary is unavailable");
  const capture = { request, descriptor: { ...state.selection }, selected: choice.selected,
    groupSeq: choice.groupSeq, choiceVersion: choice.choiceVersion };
  assertFixedAdoptionPrimary(state, capture);
  return capture;
}
export function assertFixedAdoptionPrimary(state: ImportGroupState, capture: FixedAdoptionCapture) {
  const choice = assertContext(state, capture);
  if (state.selectionBusy || !sameFixedDescriptor(state.selection, capture.descriptor) || !choice ||
    choice.groupSeq !== capture.groupSeq || choice.choiceVersion !== capture.choiceVersion ||
    choice.selected !== capture.selected || candidateIdentity(choice.candidate) !== candidateIdentity(capture.request.context.primary)) {
    groupStateError("fixed adoption primary or selector changed");
  }
}
export function assertFixedAdoptionAck(capture: FixedAdoptionCapture, ack: GroupMemberChange) {
  const { context, member } = capture.request;
  const sameMember = context.primary.candidateId === member.candidate.candidateId;
  const persistedSame = sameMember && capture.choiceVersion > 0;
  const expected = { ...capture.descriptor, stateVersion: capture.descriptor.stateVersion + (persistedSame ? 0 : 1),
    choiceCount: capture.descriptor.choiceCount + (capture.choiceVersion === 0 ? 1 : 0) };
  validateGroupDescriptor(context.scope, ack.selection, capture.descriptor);
  if (!sameFixedDescriptor(ack.selection, expected) || ack.groupId !== context.group.groupId ||
    ack.candidateId !== member.candidate.candidateId || ack.selected !== capture.selected ||
    (ack.previousCandidateId !== null && ack.previousCandidateId !== context.primary.candidateId) ||
    (capture.choiceVersion > 0 && ack.previousCandidateId !== context.primary.candidateId)) {
    groupStateError("fixed adoption acknowledgement changed the captured choice");
  }
}
export function assertFixedAdoptionAccepted(state: ImportGroupState, capture: FixedAdoptionCapture, ack: GroupMemberChange) {
  assertFixedAdoptionAck(capture, ack);
  const choice = assertContext(state, capture);
  const expectedChoiceVersion = capture.request.context.primary.candidateId === ack.candidateId && capture.choiceVersion > 0
    ? capture.choiceVersion : ack.selection.stateVersion;
  if (state.selectionBusy || !sameFixedDescriptor(state.selection, ack.selection) || !choice ||
    choice.groupSeq !== capture.groupSeq || choice.choiceVersion !== expectedChoiceVersion ||
    choice.selected !== capture.selected || candidateIdentity(choice.candidate) !== candidateIdentity(capture.request.member.candidate)) {
    groupStateError("fixed adoption acknowledgement was superseded or cannot be reconciled");
  }
}
export function sameFixedAck(left: GroupMemberChange, right: GroupMemberChange) {
  return sameFixedDescriptor(left.selection, right.selection) && left.groupId === right.groupId &&
    left.previousCandidateId === right.previousCandidateId && left.candidateId === right.candidateId && left.selected === right.selected;
}
