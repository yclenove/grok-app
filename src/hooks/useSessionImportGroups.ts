import { useCallback, useEffect, useRef, useState } from "react";
import * as api from "@/lib/api/sessionMigrationGroups";
import {
  emptyImportGroups, GROUP_UI_PAGE_SIZE, groupStateError, sameGroupIds,
  validateGroupCandidateId, validateGroupDescriptor, validateGroupIds,
  validateGroupMembership, validateGroupPage, validateGroupScope,
  type ImportGroupScope, type ImportGroupState,
} from "@/lib/sessionImport/groupSelectionState";

export type SessionImportGroupsClient = {
  get: typeof api.sessionImportGroupSelectionGet;
  groups: typeof api.sessionImportScanGroups;
  membership: typeof api.sessionImportGroupSelectionMembership;
  freeze: typeof api.sessionImportGroupSelectionFreeze;
  clear: typeof api.sessionImportGroupSelectionClear;
  set: typeof api.sessionImportGroupSelectionSet;
  invert: typeof api.sessionImportGroupSelectionInvert;
  choose: typeof api.sessionImportGroupSelectionChoose;
};
const liveClient: SessionImportGroupsClient = {
  get: api.sessionImportGroupSelectionGet, groups: api.sessionImportScanGroups,
  membership: api.sessionImportGroupSelectionMembership,
  freeze: api.sessionImportGroupSelectionFreeze, clear: api.sessionImportGroupSelectionClear,
  set: api.sessionImportGroupSelectionSet, invert: api.sessionImportGroupSelectionInvert,
  choose: api.sessionImportGroupSelectionChoose,
};
type Options = {
  onError?: (message: string) => void;
  onMemberChanged?: (change: api.GroupMemberChange, priorCandidateId: string) => void;
  onChoicesAccepted?: (state: ImportGroupState) => void;
  onCleared?: () => void;
};
type ReadOwner = { lifetime: number; request: number };
export type GroupChooseAcknowledgement = {
  /** Authenticated ACK only; retained even when its later page read fails. */
  acknowledgement: api.GroupMemberChange | null;
  /** Coherent current page was read. A later descriptor may still supersede ACK. */
  reconciled: boolean;
};

/** Explicit scope ownership; never adopts a group descriptor into candidate scan state. */
export function useSessionImportGroups(client: SessionImportGroupsClient = liveClient, options: Options = {}) {
  const [state, setState] = useState<ImportGroupState>(emptyImportGroups);
  const current = useRef(state);
  const lifetime = useRef(0);
  const pageRequest = useRef(0);
  const mutationSerial = useRef(0);
  const mutationFlight = useRef<number | null>(null);
  const pageCursors = useRef<Array<string | undefined>>([undefined]);
  const commit = useCallback((next: ImportGroupState) => { current.current = next; setState(next); }, []);
  const owns = useCallback((owner: ReadOwner) =>
    lifetime.current === owner.lifetime && pageRequest.current === owner.request, []);
  const report = useCallback((message: string) => {
    commit({ ...current.current, error: message });
    options.onError?.(message);
  }, [commit, options.onError]);

  const reset = useCallback(() => {
    ++lifetime.current;
    ++pageRequest.current;
    mutationFlight.current = null;
    pageCursors.current = [undefined];
    commit(emptyImportGroups());
  }, [commit]);
  useEffect(() => () => {
    ++lifetime.current;
    ++pageRequest.current;
    mutationFlight.current = null;
    current.current = emptyImportGroups();
  }, []);

  const readPage = useCallback(async (settings: {
    index: number; filter: api.SourceGroupFilter; rereadDescriptor?: boolean; metadataOnly?: boolean;
  }): Promise<boolean> => {
    const scope = current.current.scope;
    if (!scope) return false;
    const owner = { lifetime: lifetime.current, request: ++pageRequest.current };
    const snapshot = current.current;
    const filter = { ...settings.filter };
    commit({ ...snapshot, loading: true, error: null });
    try {
      let descriptor = snapshot.selection;
      if (!descriptor || settings.rereadDescriptor) {
        descriptor = await client.get(scope.scanId, scope.epoch);
        if (!owns(owner)) return false;
        validateGroupDescriptor(scope, descriptor, snapshot.selection);
      }
      const page = await client.groups(scope.scanId, scope.epoch, {
        ...(pageCursors.current[settings.index] ? { cursor: pageCursors.current[settings.index] } : {}),
        limit: GROUP_UI_PAGE_SIZE, filter,
      });
      if (!owns(owner)) return false;
      validateGroupPage(scope, page, filter);
      if (page.stateVersion < snapshot.metadataVersion) groupStateError("older group metadata page");
      let choices = snapshot.choices;
      const canReuse = settings.metadataOnly && snapshot.ready &&
        descriptor.stateVersion === snapshot.selection?.stateVersion && sameGroupIds(snapshot.groups, page.groups);
      if (!canReuse) {
        let coherent = false;
        // Bound reconciliation: another writer can advance between get and membership.
        // No row/count is adopted until one descriptor/membership version agrees.
        for (let attempt = 0; attempt < 3; attempt += 1) {
          const membership = page.groups.length
            ? await client.membership(scope.scanId, scope.epoch, descriptor.selectionId, page.groups.map((group) => group.groupId))
            : { selectionId: descriptor.selectionId, stateVersion: descriptor.stateVersion, choices: [] };
          if (!owns(owner)) return false;
          if (membership.selectionId !== descriptor.selectionId) groupStateError("foreign group membership selector");
          if (membership.stateVersion === descriptor.stateVersion) {
            choices = validateGroupMembership(scope, descriptor, page.groups, membership,
              descriptor.stateVersion === snapshot.selection?.stateVersion ? snapshot.choices : undefined);
            coherent = true;
            break;
          }
          const next = await client.get(scope.scanId, scope.epoch);
          if (!owns(owner)) return false;
          validateGroupDescriptor(scope, next, descriptor);
          descriptor = next;
        }
        if (!coherent) groupStateError("group selection changed during membership reconciliation");
      }
      if (!owns(owner)) return false;
      const next: ImportGroupState = {
        ...current.current, selection: descriptor, groups: page.groups, choices,
        nextCursor: page.nextCursor, pageIndex: settings.index, filter,
        totalCount: page.totalCount, complete: page.complete, metadataVersion: page.stateVersion,
        ready: true, loading: false, error: null,
      };
      commit(next);
      options.onChoicesAccepted?.(next);
      return owns(owner);
    } catch (cause) {
      if (owns(owner)) report(String(cause));
      return false;
    } finally {
      if (owns(owner)) commit({ ...current.current, loading: false });
    }
  }, [client, commit, owns, options.onChoicesAccepted, report]);

  const start = useCallback(async (scope: ImportGroupScope, filter: api.SourceGroupFilter = {}) => {
    reset();
    try { validateGroupScope(scope); } catch (cause) { report(String(cause)); return false; }
    commit({ ...emptyImportGroups(), scope: { ...scope }, filter: { ...filter } });
    return readPage({ index: 0, filter, rereadDescriptor: true });
  }, [commit, readPage, report, reset]);
  const refresh = useCallback(async () => {
    if (mutationFlight.current !== null) return false;
    return readPage({ index: current.current.pageIndex, filter: current.current.filter, rereadDescriptor: true });
  }, [readPage]);
  const refreshMetadata = useCallback(async () => {
    if (mutationFlight.current !== null || current.current.loading || !current.current.ready) return false;
    return readPage({ index: current.current.pageIndex, filter: current.current.filter, metadataOnly: true });
  }, [readPage]);
  const setFilter = useCallback(async (filter: api.SourceGroupFilter) => {
    if (mutationFlight.current !== null) return false;
    pageCursors.current = [undefined];
    return readPage({ index: 0, filter: { ...filter }, rereadDescriptor: true });
  }, [readPage]);
  const nextPage = useCallback(async () => {
    const snapshot = current.current;
    if (!snapshot.ready || snapshot.loading || mutationFlight.current !== null || !snapshot.nextCursor || !snapshot.groups.length) return false;
    const index = snapshot.pageIndex + 1;
    pageCursors.current[index] = snapshot.nextCursor;
    return readPage({ index, filter: snapshot.filter, rereadDescriptor: true });
  }, [readPage]);
  const previousPage = useCallback(async () => {
    const snapshot = current.current;
    if (!snapshot.ready || snapshot.loading || mutationFlight.current !== null || snapshot.pageIndex === 0) return false;
    return readPage({ index: snapshot.pageIndex - 1, filter: snapshot.filter, rereadDescriptor: true });
  }, [readPage]);

  const mutate = useCallback(async (
    run: (owner: api.GroupSelectionOwner) => Promise<api.GroupSelection | api.GroupMemberChange>,
    target?: { groupId: string; candidateId: string },
    cleared = false,
    onAcknowledged?: (change: api.GroupMemberChange) => void,
  ): Promise<boolean> => {
    const snapshot = current.current;
    const descriptor = snapshot.selection;
    const scope = snapshot.scope;
    if (!scope || !descriptor || !snapshot.ready || snapshot.loading || mutationFlight.current !== null) return false;
    const generation = lifetime.current;
    const request = ++mutationSerial.current;
    mutationFlight.current = request; // Reserve before IPC or a React render.
    ++pageRequest.current;
    commit({ ...snapshot, selectionBusy: true, error: null });
    const ownsMutation = () => lifetime.current === generation && mutationFlight.current === request;
    try {
      const received = await run({ scanId: scope.scanId, epoch: scope.epoch,
        selectionId: descriptor.selectionId, expectedStateVersion: descriptor.stateVersion });
      if (!ownsMutation()) return false;
      const response = structuredClone(received);
      const change = "selection" in response ? response : null;
      const next = change?.selection ?? response as api.GroupSelection;
      validateGroupDescriptor(scope, next, descriptor);
      if (cleared && (next.selectedCount !== 0 || next.choiceCount !== 0)) {
        groupStateError("clear acknowledgement retained group choices");
      }
      let priorCandidateId: string | null = null;
      if (target) {
        const prior = snapshot.choices[target.groupId];
        if (!change || !prior || change.groupId !== target.groupId || change.candidateId !== target.candidateId ||
          change.selected !== prior.selected || (change.previousCandidateId !== null &&
            change.previousCandidateId !== prior.candidate.candidateId)) groupStateError("foreign member change response");
        // An uncaptured row has no persisted previous choice, but may own a preview.
        priorCandidateId = prior.candidate.candidateId;
        if (priorCandidateId !== change.candidateId && next.stateVersion === descriptor.stateVersion) {
          groupStateError("group member changed without a selection version");
        }
      }
      commit({ ...current.current, selection: next, choices: {}, ready: false });
      if (change) onAcknowledged?.(structuredClone(change));
      if (!ownsMutation()) return false;
      if (cleared) options.onCleared?.();
      if (!ownsMutation()) return false;
      if (change && priorCandidateId && priorCandidateId !== change.candidateId) {
        options.onMemberChanged?.(structuredClone(change), priorCandidateId);
      }
      if (!ownsMutation()) return false;
      return await readPage({ index: current.current.pageIndex, filter: current.current.filter });
    } catch (cause) {
      if (ownsMutation()) report(String(cause));
      return false;
    } finally {
      if (ownsMutation()) {
        mutationFlight.current = null;
        commit({ ...current.current, selectionBusy: false });
      }
    }
  }, [commit, options.onCleared, options.onMemberChanged, readPage, report]);

  const withGroups = useCallback(async (ids: string[], run: (owner: api.GroupSelectionOwner) => Promise<api.GroupSelection>) => {
    if (!current.current.scope || !current.current.ready) return false;
    try { validateGroupIds(ids, new Set(current.current.groups.map((group) => group.groupId))); }
    catch (cause) { report(String(cause)); return false; }
    return mutate(run);
  }, [mutate, report]);
  const setSelected = useCallback((ids: string[], selected: boolean) =>
    withGroups(ids, (owner) => client.set(owner, [...ids], selected)), [client, withGroups]);
  const invert = useCallback((ids: string[]) =>
    withGroups(ids, (owner) => client.invert(owner, [...ids])), [client, withGroups]);
  const freeze = useCallback(() => mutate((owner) => client.freeze(owner)), [client, mutate]);
  const clear = useCallback(() => mutate((owner) => client.clear(owner), undefined, true), [client, mutate]);
  const chooseAcknowledged = useCallback(async (groupId: string, candidateId: string,
    onAcknowledged?: (change: api.GroupMemberChange) => void,
  ): Promise<GroupChooseAcknowledgement> => {
    const rejected: GroupChooseAcknowledgement = { acknowledgement: null, reconciled: false };
    const snapshot = current.current;
    if (!snapshot.scope || !snapshot.ready) return rejected;
    try {
      validateGroupIds([groupId], new Set(snapshot.groups.map((group) => group.groupId)));
      validateGroupCandidateId(snapshot.scope, candidateId);
    } catch (cause) { report(String(cause)); return rejected; }
    let acknowledgement: api.GroupMemberChange | null = null;
    const reconciled = await mutate((owner) => client.choose(owner, groupId, candidateId), { groupId, candidateId }, false,
      (change) => {
        acknowledgement = structuredClone(change);
        onAcknowledged?.(structuredClone(change));
      });
    return { acknowledgement, reconciled };
  }, [client, mutate, report]);
  const choose = useCallback(async (groupId: string, candidateId: string) =>
    (await chooseAcknowledged(groupId, candidateId)).reconciled, [chooseAcknowledged]);

  const getState = useCallback(() => current.current, []);
  return { ...state, getState, start, reset, refresh, refreshMetadata, setFilter, nextPage, previousPage,
    setSelected, invert, freeze, clear, choose, chooseAcknowledged };
}
