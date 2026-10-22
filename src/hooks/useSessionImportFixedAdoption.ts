import { useCallback, useEffect, useRef, useState } from "react";
import type { GroupMemberChange } from "@/lib/api/sessionMigrationGroups";
import { fixedScopeKey, type FixedPreviewTransfer, type FixedPreviewTransferRequest } from "@/lib/sessionImport/fixedPreviewTransfer";
import { assertFixedAdoptionAccepted, assertFixedAdoptionAck, assertFixedAdoptionPrimary, captureFixedAdoption,
  sameFixedAck, sameFixedDescriptor, type FixedAdoptionCapture, type FixedAdoptionStatus } from "@/lib/sessionImport/fixedAdoptionState";
import type { ImportGroupState } from "@/lib/sessionImport/groupSelectionState";
import type { ImportSelectionView, PreviewGroupBinding } from "@/lib/sessionImport/selectionView";
import type { useSessionImportGroups } from "./useSessionImportGroups";
import { comparisonContextKey, type ComparisonPreviewContext } from "@/lib/sessionImport/comparisonPreviewState";

export type FixedAdoptionTransfer = FixedPreviewTransfer;
type Groups = ReturnType<typeof useSessionImportGroups>;
type Flight = {
  capture: FixedAdoptionCapture; generation: number; valid: boolean; dispatched: boolean;
  ack: GroupMemberChange | null; binding: PreviewGroupBinding | null; reconcilable: boolean;
};
type Options = {
  groups: Pick<Groups, "getState" | "chooseAcknowledged" | "refresh">;
  getView: () => ImportSelectionView | null;
  canRetain: (candidateId: string) => boolean;
  register: (candidateId: string) => PreviewGroupBinding | null;
  quarantine: (candidateId: string) => void;
  revoke: (candidateId: string, binding: PreviewGroupBinding) => void;
};
const idle = (): FixedAdoptionStatus => ({ phase: "idle", busy: false, error: null });

/** One bounded request/ACK, owned above the comparison dialog's lifetime. */
export function useSessionImportFixedAdoption(options: Options) {
  const [status, renderStatus] = useState<FixedAdoptionStatus>(idle);
  const currentStatus = useRef(status);
  const setStatus = useCallback((next: FixedAdoptionStatus) => {
    currentStatus.current = next;
    renderStatus(next);
  }, []);
  const getStatus = useCallback(() => ({ ...currentStatus.current }), []);
  const lifetime = useRef(0);
  const flight = useRef<Flight | null>(null);
  const pending = useRef<Flight | null>(null);
  const owns = useCallback((owner: Flight) => {
    const view = options.getView();
    return owner.valid && owner.generation === lifetime.current && flight.current === owner &&
      view?.domain === "fixed-groups" && fixedScopeKey(view.scope) === fixedScopeKey(owner.capture.request.context.scope);
  }, [options.getView]);
  const revoke = useCallback((owner: Flight) => {
    if (owner.binding) options.revoke(owner.capture.request.member.candidate.candidateId, owner.binding);
    owner.binding = null;
  }, [options.revoke]);
  const reset = useCallback(() => {
    ++lifetime.current;
    if (flight.current) { flight.current.valid = false; revoke(flight.current); }
    flight.current = null;
    pending.current = null;
    setStatus(idle());
  }, [revoke, setStatus]);
  useEffect(() => () => {
    ++lifetime.current;
    if (flight.current) flight.current.valid = false;
    flight.current = null; pending.current = null;
  }, []);
  const cancel = useCallback(() => {
    const owner = flight.current;
    if (owner) {
      owner.valid = false;
      revoke(owner);
      // An already-dispatched command may still ACK. Retain only this exact
      // tombstone until it settles, so its callbacks cannot destroy old edits.
      if (!owner.dispatched) flight.current = null;
    }
    pending.current = null;
    setStatus({ phase: "stale", busy: !!owner?.dispatched, error: null });
  }, [revoke, setStatus]);
  const preservesChange = useCallback((change: GroupMemberChange, prior: string) => {
    const owner = flight.current ?? pending.current;
    return !!owner?.ack && owner.generation === lifetime.current &&
      prior === owner.capture.request.context.primary.candidateId && sameFixedAck(owner.ack, change);
  }, []);
  const preservesChoice = useCallback((candidateId: string, groupId: string, state: ImportGroupState) => {
    const owner = flight.current ?? pending.current;
    const target = owner?.capture.request.member.candidate;
    const choice = state.choices[groupId];
    return !!owner?.ack && owner.generation === lifetime.current &&
      candidateId === owner.capture.request.context.primary.candidateId && groupId === owner.ack.groupId &&
      fixedScopeKey(state.scope) === fixedScopeKey(owner.capture.request.context.scope) &&
      sameFixedDescriptor(state.selection, owner.ack.selection) && choice?.selected === owner.ack.selected && !!target &&
      choice.candidate.candidateId === target.candidateId && choice.candidate.sourceRevision === target.sourceRevision;
  }, []);
  const isBlocked = useCallback(() => flight.current !== null || pending.current !== null, []);
  const ownsComparison = useCallback((context: ComparisonPreviewContext) => {
    const owner = flight.current ?? pending.current;
    const view = options.getView();
    return !!owner?.valid && owner.generation === lifetime.current && view?.domain === "fixed-groups" &&
      fixedScopeKey(view.scope) === fixedScopeKey(context.scope) &&
      comparisonContextKey(context) === comparisonContextKey(owner.capture.request.context);
  }, [options.getView]);
  const dismissStatus = useCallback(() => {
    if (isBlocked()) return false;
    setStatus(idle());
    return true;
  }, [isBlocked, setStatus]);
  const execute = useCallback(async (owner: Flight, transfer: FixedAdoptionTransfer, reconciliation: boolean) => {
    flight.current = owner; pending.current = null;
    setStatus({ phase: "adopting", busy: true, error: null });
    let error: string | null = null;
    let reconciled = false;
    let completed = false;
    let authorityUsed = false;
    try {
      const adopted = await transfer.run(structuredClone(owner.capture.request), async (request, contentOwns) => {
        if (authorityUsed || !owns(owner) || !contentOwns() ||
          JSON.stringify(request) !== JSON.stringify(owner.capture.request)) return false;
        authorityUsed = true;
        if (reconciliation) {
          reconciled = await options.groups.refresh(); // Read only; never repeat CAS.
        } else {
          assertFixedAdoptionPrimary(options.groups.getState(), owner.capture);
          if (!owns(owner) || !contentOwns()) return false;
          owner.dispatched = true;
          const result = await options.groups.chooseAcknowledged(owner.capture.request.context.group.groupId,
            owner.capture.request.member.candidate.candidateId, (ack) => {
              assertFixedAdoptionAck(owner.capture, ack);
              owner.ack = structuredClone(ack);
              options.quarantine(owner.capture.request.context.primary.candidateId);
            });
          reconciled = result.reconciled;
        }
        if (!owns(owner) || !contentOwns() || !owner.ack || !reconciled) return false;
        assertFixedAdoptionAccepted(options.groups.getState(), owner.capture, owner.ack);
        owner.binding = options.register(owner.capture.request.member.candidate.candidateId);
        return !!owner.binding && owns(owner) && contentOwns();
      });
      if (!owns(owner)) return false;
      if (!adopted) error = transfer.getError();
      if (adopted && owner.binding && owner.ack && transfer.isCurrent(owner.capture.request)) {
        assertFixedAdoptionAccepted(options.groups.getState(), owner.capture, owner.ack);
        completed = true;
        setStatus({ phase: "adopted", busy: false, error: null });
        return true;
      }
    } catch (cause) { error = String(cause); }
    finally {
      if (flight.current === owner) {
        revokeFailed();
        flight.current = null;
        if (!owner.valid || owner.generation !== lifetime.current) {
          setStatus({ phase: "stale", busy: false, error: null });
        }
      }
    }
    return false;

    function revokeFailed() {
      // Success keeps the exact private binding. Any other path removes only
      // this flight's binding, never a later registration with the same ID.
      if (completed) return;
      revoke(owner);
      if (!owner.valid || owner.generation !== lifetime.current) return;
      const invalidSnapshot = /(?:PLAN_STALE|SCHEMA_UNSUPPORTED|RESOURCE_LIMIT):/.test(error ?? "");
      if (owner.ack) {
        owner.reconcilable = !reconciled && !invalidSnapshot;
        pending.current = owner;
        setStatus({ phase: owner.reconcilable ? "reconcile-required" : "stale", busy: false,
          error: error ?? (owner.reconcilable ? "PLAN_STALE: accepted fixed preference needs explicit reconciliation"
            : "PLAN_STALE: fixed adoption must be compared again") });
      } else if (owner.dispatched) {
        pending.current = owner;
        setStatus({ phase: "uncertain", busy: false, error: error ?? "PLAN_STALE: member command has no current acknowledgement" });
      } else if (invalidSnapshot) {
        pending.current = owner;
        setStatus({ phase: "stale", busy: false, error });
      } else setStatus({ phase: "failed", busy: false, error });
    }
  }, [options.groups, options.quarantine, options.register, owns, revoke, setStatus]);

  const adopt = useCallback(async (request: FixedPreviewTransferRequest, transfer: FixedAdoptionTransfer) => {
    if (isBlocked()) return false;
    try {
      const capture = captureFixedAdoption(options.groups.getState(), request);
      if (!options.canRetain(capture.request.member.candidate.candidateId)) {
        throw new Error("RESOURCE_LIMIT: retained content overrides exceed one bounded page");
      }
      return await execute({ capture, generation: lifetime.current, valid: true, dispatched: false,
        ack: null, binding: null, reconcilable: false }, transfer, false);
    } catch (cause) { setStatus({ phase: "failed", busy: false, error: String(cause) }); return false; }
  }, [execute, isBlocked, options.canRetain, options.groups, setStatus]);
  const reconcile = useCallback(async (transfer: FixedAdoptionTransfer) => {
    const owner = pending.current;
    if (flight.current || !owner?.ack || !owner.reconcilable || !owner.valid || owner.generation !== lifetime.current) return false;
    return execute(owner, transfer, true);
  }, [execute]);
  return { status, getStatus, adopt, reconcile, cancel, reset, isBlocked, preservesChange, preservesChoice, ownsComparison, dismissStatus };
}
