import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { PrepareBatchStatus, PrepareOverride, ScanSource } from "@/lib/api/sessionMigration";
import type { GroupMemberChange } from "@/lib/api/sessionMigrationGroups";
import { groupRows, candidateRows, selectedOverrides, IMPORT_OVERRIDE_LIMIT,
  type ImportSelectionView, type PreviewGroupBinding } from "@/lib/sessionImport/selectionView";
import { groupStateError, type ImportGroupState } from "@/lib/sessionImport/groupSelectionState";
import { extrasNotOnPage } from "@/lib/sessionImport/scanList";
import { useSessionImportGroups } from "./useSessionImportGroups";
import { useSessionImportScan } from "./useSessionImportScan";
import { useSessionImportFixedAdoption, type FixedAdoptionTransfer } from "./useSessionImportFixedAdoption";
import type { FixedPreviewTransferRequest } from "@/lib/sessionImport/fixedPreviewTransfer";
import { copyComparisonContext, type ComparisonPreviewContext } from "@/lib/sessionImport/comparisonPreviewState";

export type ImportSelectionOptions = {
  onCandidateInvalidated?: (candidateId: string) => void;
  isLocked?: () => boolean;
};
type Mode = ImportSelectionView["domain"];

/** Owns the live selection domain; batches and legacy scan descriptors stay separate. */
export function useSessionImportSelection(onError: (message: string) => void, options: ImportSelectionOptions = {}) {
  const [mode, setMode] = useState<Mode>("fixed-groups");
  const modeRef = useRef<Mode>(mode);
  const lifetime = useRef(0);
  const prepareSerial = useRef(0);
  const prepareFlight = useRef<number | null>(null);
  const mutationFlight = useRef(false);
  const [preparing, setPreparing] = useState(false);
  const bindings = useRef(new Map<string, PreviewGroupBinding>());
  const previewOwners = useRef(new Set<string>());
  const reviewOnly = useRef(new Set<string>());
  const fixedControls = useRef<ReturnType<typeof useSessionImportFixedAdoption> | null>(null);
  const acceptedDisplay = useRef<ImportGroupState | null>(null);
  const metadataAttempt = useRef<string | null>(null);
  const scan = useSessionImportScan(onError, { readCandidatePages: mode === "independent-candidates" });
  const invalidate = useCallback((candidateId: string) => {
    previewOwners.current.delete(candidateId);
    bindings.current.delete(candidateId);
    reviewOnly.current.delete(candidateId);
    options.onCandidateInvalidated?.(candidateId);
  }, [options.onCandidateInvalidated]);
  const quarantine = useCallback((candidateId: string) => {
    if (previewOwners.current.has(candidateId)) reviewOnly.current.add(candidateId);
    bindings.current.delete(candidateId);
  }, []);
  const onMemberChanged = useCallback((change: GroupMemberChange, prior: string) => {
    if (fixedControls.current?.preservesChange(change, prior)) quarantine(prior);
    else invalidate(prior);
  }, [invalidate, quarantine]);
  const onCleared = useCallback(() => {
    for (const id of [...reviewOnly.current]) invalidate(id);
    for (const [id, binding] of [...bindings.current]) {
      if (id !== binding.group.firstMember.candidateId) invalidate(id);
    }
  }, [invalidate]);
  const onChoicesAccepted = useCallback((next: ImportGroupState) => {
    acceptedDisplay.current = next;
    for (const [id, binding] of [...bindings.current]) {
      const choice = next.choices[binding.group.groupId];
      if (choice && choice.candidate.candidateId !== id) {
        if (fixedControls.current?.preservesChoice(id, binding.group.groupId, next)) quarantine(id);
        else invalidate(id);
      }
    }
  }, [invalidate, quarantine]);
  const groups = useSessionImportGroups(undefined, { onError, onMemberChanged, onCleared, onChoicesAccepted });
  const close = useCallback(() => {
    ++lifetime.current;
    fixedControls.current?.reset();
    prepareFlight.current = null;
    mutationFlight.current = false;
    setPreparing(false);
    metadataAttempt.current = null;
    acceptedDisplay.current = null;
    for (const id of [...previewOwners.current]) invalidate(id);
    groups.reset();
  }, [groups.reset, invalidate]);
  useEffect(() => () => {
    ++lifetime.current; prepareFlight.current = null;
    bindings.current.clear(); previewOwners.current.clear(); reviewOnly.current.clear();
  }, []);
  const clearSource = useCallback(() => { close(); scan.clearIntent(); }, [close, scan.clearIntent]);

  const getView = useCallback((): ImportSelectionView | null => {
    const live = scan.getState();
    if (!live.scan) return null;
    const scope = { scanId: live.scan.scanId, epoch: live.scan.epoch, generation: scan.scanGeneration.current };
    if (modeRef.current === "independent-candidates") {
      return { domain: "independent-candidates", scope, descriptor: live.selection, ready: !!live.selection };
    }
    const state = groups.getState();
    return { domain: "fixed-groups", scope, descriptor: state.selection,
      ready: state.ready && state.scope?.scanId === scope.scanId && state.scope.epoch === scope.epoch && state.scope.generation === scope.generation };
  }, [groups.getState, scan.getState, scan.scanGeneration]);
  const blocked = useCallback(() => prepareFlight.current !== null || mutationFlight.current ||
    fixedControls.current?.isBlocked() || groups.getState().loading || groups.getState().selectionBusy ||
    !!options.isLocked?.(), [groups.getState, options.isLocked]);
  const startScan = useCallback(async (sources?: ScanSource[]) => {
    if (blocked()) return false;
    close();
    modeRef.current = "fixed-groups";
    setMode("fixed-groups");
    const owner = lifetime.current;
    if (!(await scan.startScan(sources))) return false;
    if (owner !== lifetime.current) { scan.stopIncompleteScan(); return false; }
    const state = scan.getState().scan;
    if (!state) return false;
    await groups.start({ scanId: state.scanId, epoch: state.epoch, generation: scan.scanGeneration.current });
    // Accepted discovery remains visible even if grouped setup needs explicit retry.
    return owner === lifetime.current;
  }, [blocked, close, groups.start, scan.getState, scan.scanGeneration, scan.startScan, scan.stopIncompleteScan]);
  const reopen = useCallback(async () => {
    if (blocked()) return;
    const state = scan.getState().scan;
    if (!state || modeRef.current !== "fixed-groups" || groups.getState().scope) return;
    await groups.start({ scanId: state.scanId, epoch: state.epoch, generation: scan.scanGeneration.current });
  }, [blocked, groups.getState, groups.start, scan.getState, scan.scanGeneration]);
  const useIndependentCandidates = useCallback(async () => {
    if (blocked()) return false;
    close();
    modeRef.current = "independent-candidates";
    setMode("independent-candidates");
    return true;
  }, [blocked, close]);
  const useFixedGroups = useCallback(async () => {
    if (blocked()) return false;
    const state = scan.getState().scan;
    if (!state) return false;
    close();
    const owner = lifetime.current;
    modeRef.current = "fixed-groups";
    setMode("fixed-groups");
    const accepted = await groups.start({ scanId: state.scanId, epoch: state.epoch, generation: scan.scanGeneration.current });
    return accepted && owner === lifetime.current;
  }, [blocked, close, groups.start, scan.getState, scan.scanGeneration]);

  useEffect(() => {
    const state = scan.scanState;
    if (mode !== "fixed-groups" || !state || !groups.ready || groups.loading || groups.selectionBusy || blocked()) return;
    const key = `${state.scanId}:${state.epoch}:${scan.scanGeneration.current}:${state.stateVersion}`;
    if (state.stateVersion <= groups.metadataVersion || metadataAttempt.current === key) return;
    metadataAttempt.current = key;
    void groups.refreshMetadata();
  }, [blocked, mode, scan.scanState, scan.scanGeneration, groups.ready, groups.loading,
    groups.selectionBusy, groups.metadataVersion, groups.refreshMetadata]);

  const mutate = useCallback(async (groupAction: () => Promise<unknown>, candidateAction: () => Promise<unknown>) => {
    if (blocked()) return false;
    mutationFlight.current = true;
    const owner = lifetime.current;
    try { const result = await (modeRef.current === "fixed-groups" ? groupAction() : candidateAction()); return result !== false && owner === lifetime.current; }
    finally { if (owner === lifetime.current) mutationFlight.current = false; }
  }, [blocked]);
  const selectThese = useCallback((ids: string[]) => mutate(() => groups.setSelected(ids, true), () => scan.selectThese(ids)), [groups.setSelected, mutate, scan.selectThese]);
  const invertThese = useCallback((ids: string[]) => mutate(() => groups.invert(ids), () => scan.invertThese(ids)), [groups.invert, mutate, scan.invertThese]);
  const selectCurrent = useCallback(() => mutate(groups.freeze, scan.selectCurrent), [groups.freeze, mutate, scan.selectCurrent]);
  const clearSelected = useCallback(() => mutate(groups.clear, scan.clearSelected), [groups.clear, mutate, scan.clearSelected]);
  const toggle = useCallback((id: string) => mutate(() => {
    const choice = groups.getState().choices[id];
    return choice ? groups.setSelected([id], !choice.selected) : Promise.resolve(false);
  }, () => scan.toggle(id)), [groups.getState, groups.setSelected, mutate, scan.toggle]);
  const chooseGroupMember = useCallback((id: string, candidateId: string) =>
    modeRef.current !== "fixed-groups" ? Promise.resolve(false) : mutate(() => groups.choose(id, candidateId), async () => false), [groups.choose, mutate]);
  const changePage = useCallback(async (direction: -1 | 1) => {
    if (blocked()) return;
    if (modeRef.current === "fixed-groups") await (direction === 1 ? groups.nextPage() : groups.previousPage());
    else await scan.changePage(direction);
  }, [blocked, groups.nextPage, groups.previousPage, scan.changePage]);
  const refresh = useCallback(async () => {
    if (blocked()) return false;
    if (modeRef.current === "independent-candidates") return scan.refresh();
    const state = scan.getState().scan;
    if (!state) return false;
    const scope = groups.getState().scope;
    if (!scope || scope.scanId !== state.scanId || scope.epoch !== state.epoch ||
      scope.generation !== scan.scanGeneration.current) {
      return groups.start({ scanId: state.scanId, epoch: state.epoch, generation: scan.scanGeneration.current });
    }
    return groups.refresh();
  }, [blocked, groups.getState, groups.refresh, groups.start, scan.getState, scan.refresh, scan.scanGeneration]);
  const canRetain = useCallback((candidateId: string) =>
    previewOwners.current.has(candidateId) || previewOwners.current.size < IMPORT_OVERRIDE_LIMIT, []);
  const registerPreview = useCallback((candidateId: string): PreviewGroupBinding | null => {
    if (!canRetain(candidateId)) {
      onError("RESOURCE_LIMIT: retained content overrides exceed one bounded page"); return null;
    }
    const state = groups.getState();
    const group = state.ready && state.groups.find((item) => state.choices[item.groupId].candidate.candidateId === candidateId);
    if (!group || !state.scope) return null;
    const binding = { scope: { ...state.scope }, group };
    previewOwners.current.add(candidateId);
    reviewOnly.current.delete(candidateId);
    bindings.current.set(candidateId, binding);
    return binding;
  }, [canRetain, groups.getState, onError]);
  const rememberPreview = useCallback((candidateId: string) => {
    if (blocked()) return false;
    if (modeRef.current !== "independent-candidates") return !!registerPreview(candidateId);
    if (!canRetain(candidateId)) { onError("RESOURCE_LIMIT: retained content overrides exceed one bounded page"); return false; }
    previewOwners.current.add(candidateId);
    return true;
  }, [blocked, canRetain, onError, registerPreview]);
  const revokeFixedBinding = useCallback((candidateId: string, binding: PreviewGroupBinding) => {
    if (bindings.current.get(candidateId) === binding) quarantine(candidateId);
  }, [quarantine]);
  const fixedAdoption = useSessionImportFixedAdoption({ groups, getView, canRetain,
    register: registerPreview, quarantine, revoke: revokeFixedBinding });
  fixedControls.current = fixedAdoption;
  const getComparisonContext = useCallback((groupId: string): ComparisonPreviewContext | null => {
    const view = getView(), state = groups.getState();
    if (view?.domain !== "fixed-groups" || !view.ready || state.loading || state.selectionBusy) return null;
    const group = state.groups.find((item) => item.groupId === groupId), primary = state.choices[groupId]?.candidate;
    return group && primary ? copyComparisonContext({ scope: view.scope, group, primary }) : null;
  }, [getView, groups.getState]);
  const adoptFixedPreview = useCallback((request: FixedPreviewTransferRequest, transfer: FixedAdoptionTransfer) => {
    if (blocked() || modeRef.current !== "fixed-groups") return Promise.resolve(false);
    return fixedControls.current!.adopt(request, transfer);
  }, [blocked]);
  const reconcileFixedPreview = useCallback((transfer: FixedAdoptionTransfer) => {
    if (prepareFlight.current !== null || mutationFlight.current || options.isLocked?.() ||
      groups.getState().loading || groups.getState().selectionBusy) return Promise.resolve(false);
    // This is a dedicated ACK-only read capability, not a public busy bypass.
    return fixedControls.current!.reconcile(transfer);
  }, [groups.getState, options.isLocked]);

  const prepare = useCallback(async (slots: PrepareOverride[], run: (
    view: ImportSelectionView, overrides: PrepareOverride[],
  ) => Promise<PrepareBatchStatus | null>) => {
    if (blocked()) return null;
    const view = getView();
    if (!view) { onError("PLAN_STALE: selection is unavailable"); return null; }
    const owner = lifetime.current;
    const request = ++prepareSerial.current;
    prepareFlight.current = request;
    setPreparing(true);
    const owns = () => owner === lifetime.current && prepareFlight.current === request;
    try {
      // Review-only content remains visible, but is not an import override.
      // Still enforce the total retained budget before projecting eligibility.
      if (slots.length > IMPORT_OVERRIDE_LIMIT) throw new Error("RESOURCE_LIMIT: prepare overrides exceed one bounded page");
      if (new Set(slots.map((slot) => slot.candidateId)).size !== slots.length ||
        slots.some((slot) => !slot.candidateId || !slot.previewId || !slot.leafId)) groupStateError("invalid content override identities");
      const filtered = await selectedOverrides(view, slots.filter((slot) => !reviewOnly.current.has(slot.candidateId)),
        new Map(bindings.current), undefined, owns);
      if (!owns()) return null;
      const latest = getView();
      if (!latest?.ready || latest.domain !== view.domain || latest.scope.scanId !== view.scope.scanId ||
        latest.scope.epoch !== view.scope.epoch || latest.scope.generation !== view.scope.generation ||
        latest.descriptor?.selectionId !== view.descriptor?.selectionId || latest.descriptor?.stateVersion !== view.descriptor?.stateVersion) {
        groupStateError("selection changed during content verification");
      }
      const next = await run(view, filtered);
      return owns() ? next : null;
    } catch (cause) { if (owns()) onError(String(cause)); return null; }
    finally { if (owns()) { prepareFlight.current = null; setPreparing(false); } }
  }, [blocked, getView, onError]);

  const extras = useMemo(() => extrasNotOnPage(scan.candidates, scan.scanState?.extraCandidates ?? []), [scan.candidates, scan.scanState?.extraCandidates]);
  // Keep a coherent display snapshot during an ACK -> membership gap. Actions
  // remain disabled by readiness; never pair new counts with old checkboxes.
  const retained = acceptedDisplay.current;
  const display = !groups.ready && retained && groups.scope && retained.scope?.scanId === groups.scope.scanId &&
    retained?.scope?.epoch === groups.scope?.epoch && retained?.scope?.generation === groups.scope?.generation
    ? retained : groups;
  const selectionRows = useMemo(() => mode === "fixed-groups" ? groupRows(display)
    : candidateRows([...scan.candidates, ...extras], scan.selected), [mode, display, scan.candidates, scan.selected, extras]);
  const selectionView = getView();
  const selected = useMemo(() => new Set(selectionRows.filter((row) => row.selected).map((row) => row.candidate.candidateId)), [selectionRows]);
  return { scan, selectionView, selectionRows, selectionReady: selectionView?.ready ?? false,
    selected, selectedCount: (mode === "fixed-groups" ? display.selection?.selectedCount : selectionView?.descriptor?.selectedCount) ?? 0,
    candidates: mode === "fixed-groups" ? selectionRows.map((row) => row.candidate) : scan.candidates,
    nextCursor: mode === "fixed-groups" ? groups.nextCursor : scan.nextCursor,
    pageIndex: mode === "fixed-groups" ? groups.pageIndex : scan.pageIndex,
    busy: preparing || groups.loading || groups.selectionBusy || fixedAdoption.status.busy || fixedAdoption.isBlocked(),
    startScan, close, clearSource, reopen, useIndependentCandidates, useFixedGroups, selectThese, invertThese, selectCurrent,
    clearSelected, toggle, chooseGroupMember, changePage, refresh, rememberPreview, prepare,
    fixedAdoption: fixedAdoption.status, getFixedAdoptionStatus: fixedAdoption.getStatus, adoptFixedPreview,
    reconcileFixedPreview, cancelFixedAdoption: fixedAdoption.cancel, getComparisonContext,
    ownsFixedComparison: fixedAdoption.ownsComparison, dismissFixedAdoptionStatus: fixedAdoption.dismissStatus };
}
