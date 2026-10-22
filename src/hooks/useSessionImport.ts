import { useCallback, useEffect, useRef, useState } from "react";
import * as api from "@/lib/api";
import { useSessionImportJob, importJobActive } from "./useSessionImportJob";
import { useSessionImportPrepare } from "./useSessionImportPrepare";
import { useSessionImportAssociations } from "./useSessionImportAssociations";
import { useSessionImportSelection, type ImportSelectionOptions } from "./useSessionImportSelection";
import { useWorkspaceImport } from "./useWorkspaceImport";
import { useImportContinuation } from "./useImportContinuation";
import {
  OPEN_IMPORTED_EVENT,
  OPEN_IMPORT_EVENT,
  parseImportedDeepLink,
  type OpenImportedDetail,
  type OpenImportDetail,
} from "@/lib/sessionImport/open";
import type {
  CatalogEntry,
  DiscoveredRoot,
  HandoffDraft,
  ImportProvider,
  PrepareOverride,
  SourceRegistration,
  WorkspaceMapping,
} from "@/lib/api/sessionMigration";
import type { AllowedSelection } from "@/lib/sessionImport/selection";
import type { PendingDurablePlan } from "@/lib/sessionImport/durablePlan";

export type ContentPlanSlot = {
  candidateId: string;
  previewId: string;
  leafId: string;
  selection: AllowedSelection;
};
import {
  choiceFromCustomFile,
  choiceFromCustomFolder,
  choiceFromDefaultRoot,
  choiceFromRelocate,
  scanSourcesForChoice,
  type SourceChoice,
} from "@/lib/sessionImport/sourceChoice";

export type ImportStep = "source" | "content" | "prepare" | "mapping" | "import" | "result";
export type ImportCommitPhase = "planning" | "submitting" | null;

function localImportErrorText(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

export function useSessionImportUi(onProjectsChanged?: () => void, options: ImportSelectionOptions = {}) {
  const [open, setOpen] = useState(false);
  const [readerId, setReaderId] = useState<string | null>(null);
  const [readerRevision, setReaderRevision] = useState<string | null>(null);
  const [readerLeaf, setReaderLeaf] = useState<string | null>(null);
  const [readerAnchor, setReaderAnchor] = useState<string | null>(null);
  const [projectId, setProjectId] = useState<string | null>(null);
  const [step, setStep] = useState<ImportStep>("source");
  const [roots, setRoots] = useState<DiscoveredRoot[]>([]);
  const [registrations, setRegistrations] = useState<SourceRegistration[]>([]);
  const [choice, setSourceChoice] = useState<SourceChoice | null>(null);
  const sourceGeneration = useRef(0);
  const rootsGeneration = useRef(0);
  const registrationGeneration = useRef(0);
  // React busy state updates after the current event. Own mapping mutations
  // synchronously so duplicate submits and Back cannot race one another.
  const mappingCommand = useRef<"commit" | "abandon" | null>(null);
  const [commitPhase, setCommitPhase] = useState<ImportCommitPhase>(null);
  const [pickerBusy, setPickerBusy] = useState(false);
  const [filePickerFlight, setFilePickerFlight] = useState<Promise<boolean> | null>(null);
  const [rootsBusy, setRootsBusy] = useState(false);
  const workspaceUi = useWorkspaceImport(onProjectsChanged);
  const [error, setError] = useState<string | null>(null);
  const prepareUi = useSessionImportPrepare(setError);
  const selectionLocked = useCallback(() => prepareUi.hasBatch() || !!options.isLocked?.(), [prepareUi.hasBatch, options.isLocked]);
  const selectionUi = useSessionImportSelection(setError, { ...options, isLocked: selectionLocked });
  const scanUi = selectionUi.scan;
  const setChoice = useCallback((next: SourceChoice | null) => {
    sourceGeneration.current += 1;
    setPickerBusy(false);
    workspaceUi.reset();
    selectionUi.clearSource();
    setSourceChoice(next);
  }, [workspaceUi.reset, selectionUi.clearSource]);
  useEffect(() => () => {
    sourceGeneration.current += 1;
    rootsGeneration.current += 1;
  }, []);
  const [busy, setBusy] = useState(false);
  const { job, setJob, jobActive, jobBusy, jobItemsStale, jobHasPrevious,
    refreshJobItems, loadMoreJobItems, previousJobItems, mutateJob } = useSessionImportJob(setError);
  const [mappings, setMappings] = useState<WorkspaceMapping[]>([]);
  const [projects, setProjects] = useState<Array<{ id: string; name: string }>>(
    [],
  );
  const [entries, setEntries] = useState<CatalogEntry[]>([]);
  const [pendingPlan, setPendingPlan] = useState<PendingDurablePlan | null>(null);
  // Undo leaves a tombstone so bulk imports never resurrect a removed session;
  // only this explicit, default-off choice re-imports such sessions.
  const [explicitReimport, setExplicitReimport] = useState(false);
  const submittedPrepareBatch = useRef<string | null>(null);
  const associations = useSessionImportAssociations(
    prepareUi.batch, open && step === "mapping" && !pendingPlan,
    () => !!mappingCommand.current || busy || prepareUi.busy,
  );
  const [restoreStatus, setRestoreStatus] = useState<
    null | "plan" | "job" | "prepare" | "draft" | "expired" | "failed"
  >(null);
  const recoveryGeneration = useRef(0);
  const historyGeneration = useRef(0);
  const continuationActivity = useCallback(() => { recoveryGeneration.current += 1; }, []);
  const continuation = useImportContinuation(projectId, continuationActivity);
  const { draft, setDraft, continuationError, setContinuationError, invalidateContinuation } = continuation;
  const { scanGeneration } = scanUi;
  const pendingReaderContinue = useRef<OpenImportedDetail | null>(null);
  const [readerContinueRequested, setReaderContinueRequested] = useState(false);
  const pickCustomFileRef = useRef<() => Promise<boolean>>(async () => false);

  useEffect(() => {
    let previousReader = "";
    const onOpen = (ev: Event) => {
      historyGeneration.current += 1;
      const d = (ev as CustomEvent<OpenImportDetail>).detail ?? {};
      if (mappingCommand.current) {
        setOpen(true);
        return;
      }
      setProjectId(d.projectId ?? null);
      setOpen(true);
      void selectionUi.reopen();
      setError(null);
      // Opening the wizard must preserve the recovered/current step. An explicit
      // file import starts a different intent, so it cannot reuse the old plan.
      if (d.mode === "file") {
        recoveryGeneration.current += 1;
        setStep("source");
        setPendingPlan(null);
        setRestoreStatus(null);
        // Settings and sidebar call this directly. The covered entry is not a
        // restore target; SourceStep adopts this promise on its own file button.
        const flight = pickCustomFileRef.current();
        setFilePickerFlight(flight);
        void flight.finally(() => {
          setFilePickerFlight((current) => (current === flight ? null : current));
        });
      }
    };
    const applyImported = (d: OpenImportedDetail | null, continueDraft?: boolean) => {
      historyGeneration.current += 1;
      const nextReader = JSON.stringify([d?.importId, d?.revision, d?.leaf]);
      if (nextReader !== previousReader) invalidateContinuation();
      previousReader = nextReader;
      setContinuationError(null);
      pendingReaderContinue.current = d?.importId && (continueDraft || d.continue) ? d : null;
      setReaderContinueRequested(pendingReaderContinue.current != null);
      if (pendingReaderContinue.current) recoveryGeneration.current += 1;
      if (!d?.importId) {
        setReaderId(null);
        setReaderRevision(null);
        setReaderLeaf(null);
        setReaderAnchor(null);
        return;
      }
      setReaderId(d.importId);
      setReaderRevision(d.revision ?? null);
      setReaderLeaf(d.leaf ?? null);
      setReaderAnchor(d.anchorEventId ?? null);
      // The Host resolves one reader snapshot before preparing its continuation.
    };
    const onReader = (ev: Event) => {
      applyImported((ev as CustomEvent<OpenImportedDetail>).detail);
    };
    const onHash = () => {
      applyImported(parseImportedDeepLink(window.location.hash || ""));
    };
    window.addEventListener(OPEN_IMPORT_EVENT, onOpen);
    window.addEventListener(OPEN_IMPORTED_EVENT, onReader);
    window.addEventListener("hashchange", onHash);
    window.addEventListener("popstate", onHash);
    applyImported(parseImportedDeepLink(window.location.hash || ""));
    return () => {
      window.removeEventListener(OPEN_IMPORT_EVENT, onOpen);
      window.removeEventListener(OPEN_IMPORTED_EVENT, onReader);
      window.removeEventListener("hashchange", onHash);
      window.removeEventListener("popstate", onHash);
    };
  }, [invalidateContinuation, selectionUi.reopen, setContinuationError]);

  const loadRoots = useCallback(async () => {
    const token = ++rootsGeneration.current;
    const registrationToken = registrationGeneration.current;
    setRootsBusy(true);
    setError(null);
    try {
      const [nextRoots, nextRegs] = await Promise.all([
        api.sessionImportListRoots(),
        api.sessionImportSources().catch(() => [] as SourceRegistration[]),
      ]);
      if (rootsGeneration.current !== token) return;
      setRoots(nextRoots);
      if (registrationGeneration.current === registrationToken) setRegistrations(nextRegs);
    } catch (e) {
      if (rootsGeneration.current === token) setError(String(e));
    } finally {
      if (rootsGeneration.current === token) setRootsBusy(false);
    }
  }, []);

  const scan = useCallback(async () => {
    // Workspace preview/list has its own confirmation flow. An empty workspace
    // must never become scan(undefined), which discovers all provider histories.
    if (workspaceUi.mode || pickerBusy || mappingCommand.current) return;
    const intent = sourceGeneration.current;
    recoveryGeneration.current += 1;
    setError(null);
    if (!(await prepareUi.discard())) return;
    if (intent !== sourceGeneration.current) return;
    setMappings([]);
    if (await selectionUi.startScan(scanSourcesForChoice(choice))) {
      setPendingPlan(null);
      setRestoreStatus(null);
      setStep("content");
    }
  }, [choice, pickerBusy, workspaceUi.mode, prepareUi.discard, selectionUi.startScan]);

  const pickCustomRoot = useCallback(async (provider: ImportProvider): Promise<boolean> => {
    const token = ++sourceGeneration.current;
    setPickerBusy(true);
    try {
      const dir = await api.pickDirectory();
      if (sourceGeneration.current !== token) return false;
      if (dir) setChoice(choiceFromCustomFolder(provider, dir));
      return true;
    } catch (e) {
      if (sourceGeneration.current !== token) return false;
      setError(String(e));
      return true;
    } finally {
      if (sourceGeneration.current === token) setPickerBusy(false);
    }
  }, [setChoice]);

  const pickCustomFile = useCallback(async (): Promise<boolean> => {
    const token = ++sourceGeneration.current;
    setPickerBusy(true);
    setError(null);
    try {
      const files = await api.pickAttachFiles();
      if (sourceGeneration.current !== token) return false;
      const file = files[0];
      if (!file) return true;
      if (/\.code-workspace$/i.test(file)) {
        recoveryGeneration.current += 1;
        selectionUi.clearSource();
        setSourceChoice(null);
        setStep("source");
        setPickerBusy(false);
        await workspaceUi.load({ kind: "file", path: file });
        return sourceGeneration.current === token;
      }
      setChoice(choiceFromCustomFile(file));
      return true;
    } catch (e) {
      if (sourceGeneration.current !== token) return false;
      setError(String(e));
      return true;
    } finally {
      if (sourceGeneration.current === token) setPickerBusy(false);
    }
  }, [setChoice, selectionUi.clearSource, workspaceUi.load]);
  pickCustomFileRef.current = pickCustomFile;

  const pickWorkspaceFolder = useCallback(async (): Promise<boolean> => {
    const token = ++sourceGeneration.current;
    setPickerBusy(true);
    setError(null);
    try {
      const path = await api.pickDirectory();
      if (sourceGeneration.current !== token) return false;
      if (!path) return true;
      recoveryGeneration.current += 1;
      selectionUi.clearSource();
      setSourceChoice(null);
      setPickerBusy(false);
      await workspaceUi.load({ kind: "folder", path });
      return sourceGeneration.current === token;
    } catch (cause) {
      if (sourceGeneration.current !== token) return false;
      setError(String(cause));
      return true;
    } finally {
      if (sourceGeneration.current === token) setPickerBusy(false);
    }
  }, [selectionUi.clearSource, workspaceUi.load]);

  const chooseListedRoot = useCallback((provider: string, path: string) => {
    setChoice(choiceFromDefaultRoot(provider, path));
  }, [setChoice]);

  const chooseRegistered = useCallback((reg: SourceRegistration) => {
    setChoice({
      kind: "registered",
      provider: reg.provider,
      path: reg.rootPath,
      sourceId: reg.sourceId,
    });
  }, [setChoice]);

  const relocateRegistered = useCallback(async (reg: SourceRegistration): Promise<boolean> => {
    const token = ++sourceGeneration.current;
    setPickerBusy(true);
    setError(null);
    try {
      const dir = await api.pickDirectory();
      if (sourceGeneration.current !== token) return false;
      if (!dir) return true;
      registrationGeneration.current += 1;
      const next = await api.sessionImportSourceRegister({
        provider: reg.provider,
        path: dir,
        sourceId: reg.sourceId,
      });
      if (sourceGeneration.current !== token) return false;
      const nextRegistrations = await api.sessionImportSources().catch(() => [next]);
      if (sourceGeneration.current !== token) return false;
      setRegistrations(nextRegistrations);
      setChoice(choiceFromRelocate(next, next.rootPath));
      return true;
    } catch (e) {
      if (sourceGeneration.current !== token) return false;
      setError(String(e));
      return true;
    } finally {
      if (sourceGeneration.current === token) setPickerBusy(false);
    }
  }, [setChoice]);

  const previewSelected = useCallback(async (slots: ContentPlanSlot[]) => {
    const generation = scanGeneration.current;
    setError(null);
    const overrides: PrepareOverride[] = slots.map((slot) => ({
      candidateId: slot.candidateId,
      previewId: slot.previewId,
      leafId: slot.leafId,
      selection: slot.selection,
    }));
    const next = await selectionUi.prepare(overrides, (view, filtered) => prepareUi.start(
      view.scope.scanId, view.scope.epoch, view.descriptor!.selectionId, filtered, view.descriptor!.stateVersion,
    ));
    if (next && generation === scanGeneration.current) {
      setStep("prepare");
    }
  }, [prepareUi.start, scanGeneration, selectionUi.prepare]);

  const finishPrepare = useCallback(async () => {
    const batch = prepareUi.batch;
    if (!batch || !["completed", "completed-with-issues"].includes(batch.state)) return;
    const batchId = batch.batchId;
    setBusy(true);
    setError(null);
    try {
      const nextMappings = await api.sessionImportPrepareMappings(batchId, projectId);
      if (!prepareUi.isCurrent(batchId)) return;
      setMappings(nextMappings);
      setStep("mapping");
    } catch (error) {
      setError(localImportErrorText(error));
    } finally {
      setBusy(false);
    }
  }, [prepareUi.batch, prepareUi.isCurrent, projectId]);

  const abandonPrepare = useCallback(async (nextStep: "source" | "content") => {
    if (mappingCommand.current || associations.isReviewing()) return false;
    mappingCommand.current = "abandon";
    const request = ++recoveryGeneration.current;
    const intent = sourceGeneration.current;
    const owns = () => request === recoveryGeneration.current && intent === sourceGeneration.current;
    setBusy(true);
    setError(null);
    try {
      if (!(await prepareUi.discard())) return false;
      if (!owns()) return false;
      setMappings([]);
      setPendingPlan(null);
      setRestoreStatus(null);
      // Close invalidates grouped selection while a retained batch locks its
      // reopen. Release the batch first, then read the same scan without grants.
      const target = nextStep === "content" && !scanUi.scanState ? "source" : nextStep;
      setStep(target);
      if (target === "content") await selectionUi.reopen();
      if (!owns()) return false;
      return true;
    } finally {
      mappingCommand.current = null;
      setBusy(false);
    }
  }, [prepareUi.discard, scanUi.scanState, selectionUi.reopen, associations]);

  const commit = useCallback(async () => {
    if (mappingCommand.current || associations.isReviewing()) return;
    mappingCommand.current = "commit";
    recoveryGeneration.current += 1;
    setBusy(true);
    setCommitPhase(pendingPlan ? "submitting" : "planning");
    setError(null);
    try {
      if (pendingPlan) {
        const requestId = pendingPlan.requestId ?? crypto.randomUUID();
        // Recovery supplies the immutable plan; retain a newly chosen request
        // identity before sending so an uncertain reply can be retried exactly.
        if (!pendingPlan.requestId) setPendingPlan({ ...pendingPlan, requestId });
        const j = await api.sessionImportCommit(
          pendingPlan.planId,
          pendingPlan.planHash,
          requestId,
          pendingPlan.explicitReimport ?? false,
        );
        submittedPrepareBatch.current = prepareUi.batch?.batchId ?? null;
        setPendingPlan(null);
        setExplicitReimport(false);
        setJob(j);
        setStep(importJobActive(j) ? "import" : "result");
        setCommitPhase(null);
        setEntries((await api.sessionImportEntries()).items ?? []);
        return;
      }
      const finalMappings = mappings.length
        ? mappings
        : [
            {
              rootId: "unassigned",
              targetProjectId: projectId,
              unassigned: !projectId,
              userChoiceVersion: 1,
            },
          ];
      if (!prepareUi.batch) {
        throw new Error("PLAN_STALE: prepare batch is unavailable");
      }
      const confirmations = associations.snapshot();
      const plan = confirmations.length
        ? await api.sessionImportPreparedAssociationPlan({
          version: 1, batchId: prepareUi.batch.batchId,
          expectedStateVersion: prepareUi.batch.stateVersion,
          mappings: finalMappings, identityAssociations: confirmations,
        })
        : await api.sessionImportPreparePlan(prepareUi.batch.batchId, finalMappings);
      const requestId = crypto.randomUUID();
      const reimport = explicitReimport && !confirmations.length;
      setPendingPlan({
        ...plan,
        requestId,
        explicitReimport: reimport,
      });
      setCommitPhase("submitting");
      const j = await api.sessionImportCommit(plan.planId, plan.planHash, requestId, reimport);
      submittedPrepareBatch.current = prepareUi.batch.batchId;
      setExplicitReimport(false);
      setPendingPlan(null);
      setJob(j);
      setStep(importJobActive(j) ? "import" : "result");
      setCommitPhase(null);
      setEntries((await api.sessionImportEntries()).items ?? []);
    } catch (e) {
      setError(localImportErrorText(e));
    } finally {
      mappingCommand.current = null;
      setCommitPhase(null);
      setBusy(false);
    }
  }, [
    pendingPlan,
    prepareUi.batch,
    projectId,
    mappings,
    associations,
    explicitReimport,
  ]);

  const prepareContinue = useCallback(async (importId: string, leaf?: string | null, revision?: string | null) => {
    pendingReaderContinue.current = null;
    setReaderContinueRequested(false);
    await continuation.prepareContinue(
      importId,
      leaf === undefined ? (importId === readerId ? readerLeaf : null) : leaf,
      revision === undefined ? (importId === readerId ? readerRevision : null) : revision,
    );
  }, [continuation.prepareContinue, readerId, readerLeaf, readerRevision]);
  const prepareReaderContinue = useCallback(async (importId: string, leaf: string, revision: string) => {
    const intent = pendingReaderContinue.current;
    if (!intent || intent.importId !== importId || !leaf || !revision ||
      (intent.revision != null && intent.revision !== revision) ||
      (intent.leaf != null && intent.leaf !== leaf)) return;
    // Consume before awaiting: repeated effects cannot create duplicate drafts.
    pendingReaderContinue.current = null;
    setReaderContinueRequested(false);
    await continuation.prepareContinue(importId, leaf, revision);
  }, [continuation.prepareContinue]);

  const createContinue = useCallback(async () => {
    const id = await continuation.createContinue();
    if (id) {
      pendingReaderContinue.current = null;
      setReaderContinueRequested(false);
      setReaderId(null);
      setReaderRevision(null);
      setReaderLeaf(null);
      setReaderAnchor(null);
      setOpen(false);
    }
    return id;
  }, [continuation.createContinue]);

  const undoJob = useCallback(() => mutateJob(api.sessionImportUndo), [mutateJob]);
  const cancelJob = useCallback(() => mutateJob(api.sessionImportCancel), [mutateJob]);
  const retryFailed = useCallback(() => mutateJob(api.sessionImportRetryFailed), [mutateJob]);

  const setMappingUnassigned = useCallback((rootId: string, unassigned: boolean) => {
    setMappings((prev) =>
      prev.map((m) =>
        m.rootId === rootId
          ? {
              ...m,
              unassigned,
              targetProjectId: unassigned ? null : projectId,
              userChoiceVersion: m.userChoiceVersion + 1,
            }
          : m,
      ),
    );
  }, [projectId]);

  const setMappingProject = useCallback((rootId: string, targetProjectId: string | null) => {
    setMappings((prev) =>
      prev.map((m) =>
        m.rootId === rootId
          ? {
              ...m,
              unassigned: !targetProjectId,
              targetProjectId,
              userChoiceVersion: m.userChoiceVersion + 1,
            }
          : m,
      ),
    );
  }, []);

  const loadEntries = useCallback(async () => {
    try {
      setEntries((await api.sessionImportEntries()).items ?? []);
    } catch {
      /* catalog may be empty */
    }
  }, []);

  useEffect(() => {
    if (open) void loadRoots();
    return () => { rootsGeneration.current += 1; };
  }, [open, loadRoots]);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    void api.projectsList().then((list) => {
      if (!cancelled) setProjects(list.map((p) => ({ id: p.id, name: p.name })));
    }).catch(() => {
      if (!cancelled) setProjects([]);
    });
    return () => { cancelled = true; };
  }, [open]);

  useEffect(() => {
    void loadEntries();
  }, [loadEntries]);

  useEffect(() => {
    let cancelled = false;
    const generation = recoveryGeneration.current;
    void api.sessionImportRecovery().then((rec) => {
      if (cancelled || generation !== recoveryGeneration.current) return;
      if (rec.job) {
        setJob(rec.job);
        setRestoreStatus("job");
        setStep("import");
      } else if (rec.plan) {
        setPendingPlan({
          ...rec.plan,
        });
        setRestoreStatus("plan");
        setStep("mapping");
      } else if (rec.prepare) {
        prepareUi.adopt(rec.prepare);
        setRestoreStatus("prepare");
        setStep("prepare");
      } else if (rec.draft) {
        setDraft(rec.draft as HandoffDraft);
        setRestoreStatus("draft");
      } else if (rec.expired) {
        setRestoreStatus("expired");
        setError("PLAN_STALE: durable plan expired");
      } else if (rec.failed) {
        setRestoreStatus("failed");
        setError(rec.failed);
      }
    }).catch((cause) => {
      if (cancelled || generation !== recoveryGeneration.current) return;
      setRestoreStatus("failed");
      setError(String(cause));
    });
    return () => {
      cancelled = true;
    };
  }, [prepareUi.adopt, setJob, setDraft]);

  useEffect(() => {
    if (!job) return;
    if (jobActive) setStep("import");
    else setStep((current) => current === "import" ? "result" : current);
    void loadEntries();
  }, [job?.jobId, job?.stateVersion, jobActive, loadEntries]);

  const setWizardOpen = useCallback((next: boolean) => {
    setOpen(next);
    if (next) void selectionUi.reopen();
    if (!next) {
      historyGeneration.current += 1;
      sourceGeneration.current += 1;
      rootsGeneration.current += 1;
      setPickerBusy(false);
      setRootsBusy(false);
      workspaceUi.reset();
      selectionUi.close();
      scanUi.stopIncompleteScan();
      // Closing also covers a start/retry whose first acknowledgement has not
      // arrived yet; button cancellation intentionally does not interrupt it.
      void prepareUi.stop();
    }
  }, [prepareUi.stop, scanUi.stopIncompleteScan, workspaceUi.reset, selectionUi.close, selectionUi.reopen]);

  useEffect(() => {
    if (!open || busy || prepareUi.batch) return;
    // A close/reopen can overlap Back. Reinitialize only the newly visible
    // lifecycle after its batch lock releases; failed scoped reads need Refresh.
    if (step === "prepare") {
      setStep(scanUi.scanState ? "content" : "source");
    } else if (step === "content") {
      void selectionUi.reopen();
    }
  }, [open, busy, step, prepareUi.batch, scanUi.scanState, selectionUi.reopen]);

  const setReader = useCallback((id: string | null) => {
    pendingReaderContinue.current = null;
    setReaderContinueRequested(false);
    invalidateContinuation();
    setReaderId(id);
    setContinuationError(null);
    if (!id) {
      setReaderRevision(null);
      setReaderLeaf(null);
      setReaderAnchor(null);
    }
  }, [invalidateContinuation, setContinuationError]);

  const anyBusy = busy || jobBusy || rootsBusy || pickerBusy || workspaceUi.busy || scanUi.scanBusy
    || selectionUi.busy || prepareUi.busy || continuation.continuationBusy || !!associations.dialog;
  const hasUnsubmittedBatch = !!prepareUi.batch && prepareUi.batch.batchId !== submittedPrepareBatch.current;
  const canOpenJobHistory = open && !anyBusy && !jobActive && !pendingPlan && !hasUnsubmittedBatch
    && !workspaceUi.mode && !draft && !readerId && !options.isLocked?.()
    && ["source", "import", "result"].includes(step);
  const historyAccess = useRef(canOpenJobHistory);
  historyAccess.current = canOpenJobHistory;
  useEffect(() => () => { historyGeneration.current += 1; }, []);
  const openExistingJob = useCallback(async (id: string, isCurrent: () => boolean): Promise<boolean> => {
    if (!historyAccess.current || mappingCommand.current || !isCurrent()) return false;
    // Only an explicit selection supersedes startup recovery. Merely browsing
    // must preserve recovered plans and any newer source/continuation intent.
    const request = ++recoveryGeneration.current;
    const historyRequest = ++historyGeneration.current;
    const source = sourceGeneration.current;
    const next = await api.sessionImportJobGet(id);
    if (!isCurrent() || !historyAccess.current || mappingCommand.current
      || request !== recoveryGeneration.current || historyRequest !== historyGeneration.current
      || source !== sourceGeneration.current) return false;
    if (next.jobId !== id) throw new Error("PLAN_STALE: mismatched import job");
    setJob(next);
    setRestoreStatus(null);
    setError(null);
    setStep(importJobActive(next) ? "import" : "result");
    return true;
  }, [setJob]);

  return {
    open,
    setOpen: setWizardOpen,
    readerId,
    readerRevision,
    readerLeaf,
    readerAnchor,
    readerContinueRequested,
    prepareReaderContinue,
    explicitReimport,
    setExplicitReimport,
    setReaderId: setReader,
    projectId,
    step,
    setStep,
    roots,
    registrations,
    choice,
    setChoice,
    pickCustomRoot,
    pickCustomFile,
    pickWorkspaceFolder,
    filePickerFlight,
    workspaceUi,
    chooseListedRoot,
    chooseRegistered,
    relocateRegistered,
    ...scanUi,
    candidates: selectionUi.candidates,
    selectionRows: selectionUi.selectionRows,
    selectionView: selectionUi.selectionView,
    selectionReady: selectionUi.selectionReady,
    selected: selectionUi.selected,
    selectedRows: new Set(selectionUi.selectionRows.filter((row) => row.selected).map((row) => row.rowId)),
    selectedCount: selectionUi.selectedCount,
    nextCursor: selectionUi.nextCursor,
    pageIndex: selectionUi.pageIndex,
    toggle: selectionUi.toggle,
    selectThese: selectionUi.selectThese,
    invertThese: selectionUi.invertThese,
    selectCurrent: selectionUi.selectCurrent,
    clearSelected: selectionUi.clearSelected,
    chooseGroupMember: selectionUi.chooseGroupMember,
    useIndependentCandidates: selectionUi.useIndependentCandidates,
    useFixedGroups: selectionUi.useFixedGroups,
    rememberPreview: selectionUi.rememberPreview,
    fixedAdoption: selectionUi.fixedAdoption,
    getFixedAdoptionStatus: selectionUi.getFixedAdoptionStatus,
    adoptFixedPreview: selectionUi.adoptFixedPreview,
    reconcileFixedPreview: selectionUi.reconcileFixedPreview,
    cancelFixedAdoption: selectionUi.cancelFixedAdoption,
    ownsFixedComparison: selectionUi.ownsFixedComparison,
    dismissFixedAdoptionStatus: selectionUi.dismissFixedAdoptionStatus,
    getComparisonContext: selectionUi.getComparisonContext,
    busy: anyBusy,
    canOpenJobHistory,
    openExistingJob,
    associations,
    error,
    continuationError,
    job,
    jobActive,
    jobItemsStale,
    jobHasPrevious,
    entries,
    draft,
    setDraft,
    pendingPlan,
    restoreStatus,
    commitPhase,
    scan,
    loadMore: () => selectionUi.changePage(1),
    previousPage: () => selectionUi.changePage(-1),
    refreshScan: selectionUi.refresh,
    cancelScan: () => scanUi.controlScan("cancel"),
    continueScan: () => scanUi.controlScan("continue"),
    mappings,
    projects,
    setMappingUnassigned,
    setMappingProject,
    previewSelected,
    prepareBatch: prepareUi.batch,
    prepareItems: prepareUi.items,
    prepareItemsError: prepareUi.itemsError,
    prepareNextCursor: prepareUi.nextCursor,
    prepareActive: prepareUi.active,
    cancelPrepare: prepareUi.cancel,
    retryPrepare: prepareUi.retry,
    loadMorePrepare: prepareUi.loadMore,
    retryPrepareItems: prepareUi.retryItems,
    finishPrepare,
    abandonPrepare,
    loadMoreJobItems,
    previousJobItems,
    refreshJobItems,
    commit,
    undoJob,
    cancelJob,
    retryFailed,
    prepareContinue,
    createContinue,
    continuationBusy: continuation.continuationBusy,
    continuationLocked: continuation.continuationLocked,
    selectContinuationWorkspace: continuation.selectContinuationWorkspace,
    continuationReviewingScope: continuation.continuationReviewingScope,
    continuationScopePending: continuation.continuationScopePending,
    beginContinuationScopeReview: continuation.beginContinuationScopeReview,
    cancelContinuationScopeReview: continuation.cancelContinuationScopeReview,
    selectContinuationScope: continuation.selectContinuationScope,
    discardContinuation: continuation.discardContinuation,
    hideContinuation: continuation.hideContinuation,
    continuationVisible: continuation.continuationVisible,
    loadEntries,
  };
}
