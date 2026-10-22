import { type KeyboardEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createT, type Locale } from "@/i18n";
import { GlassModal } from "@/components/GlassModal";
import { SessionImportMappingStep } from "./SessionImportMappingStep";
import { useSessionImportUi, type ImportStep } from "@/hooks/useSessionImport";
import { useImportClipboard } from "@/hooks/useImportClipboard";
import { ImportedReaderActions } from "./ImportedReaderActions";
import { SessionImportReaderContent } from "./SessionImportReaderContent";
import { ImportedReaderError } from "./ImportedReaderError";
import { useImportedReader } from "@/hooks/useImportedReader";
import { useImportedImages } from "@/hooks/useImportedImages";
import { useImportedCatalog } from "@/hooks/useImportedCatalog";
import { useImportedEntry } from "@/hooks/useImportedEntry";
import { useSessionImportContentStates } from "@/hooks/useSessionImportContentStates";
import { useSessionImportComparisonDialog } from "@/hooks/useSessionImportComparisonDialog";
import { SessionImportSourceComparisonDialog } from "./SessionImportSourceComparisonDialog";
import { importErrorKey } from "@/lib/sessionImport/errorCopy";
import { isDialogFocusAllowed } from "@/lib/a11yFocus";
import { eventHasIssue, eventPlainText, importedEventDisplay } from "@/lib/sessionImport/eventView";
import { nextReaderHit, readerShortcutDirection } from "@/lib/sessionImport/readerNavigation";
import {
  openImportedHistory,
  rememberImportedHistoryAnchor,
} from "@/lib/sessionImport/open";
import type { SourceCandidate } from "@/lib/api/sessionMigration";
import { extrasNotOnPage } from "@/lib/sessionImport/scanList";
import {
  windowReaderEvents,
} from "@/lib/sessionImport/readerWindow";
import {
  sessionImportExportMarkdown,
} from "@/lib/api/sessionMigration";
import { ImportedRevisionNotice } from "./ImportedRevisionNotice";
import { ImportedCompactionHistory } from "./ImportedCompactionHistory";
import { ImportedRevisionPicker } from "./ImportedRevisionPicker";
import { ImportedWorkspaceAssociation } from "./ImportedWorkspaceAssociation";
import { SessionImportSourceActions, SessionImportSourceStep } from "./SessionImportSourceStep";
import { SessionImportWorkspacePanel } from "./SessionImportWorkspacePanel";
import { SessionImportContentActions, SessionImportContentStep } from "./SessionImportContentStep";
import { SessionImportPrepareStep } from "./SessionImportPrepareStep";
import { SessionImportContinuationModal } from "./SessionImportContinuationModal";
import { SessionImportResults } from "./SessionImportResults";
import { SessionImportPageStatus } from "./SessionImportPageStatus";
import type { ReaderPane } from "./SessionImportReaderSplit";
import { SessionImportStepNav } from "./SessionImportStepNav";
import { SessionImportJobHistory } from "./SessionImportJobHistory";
import { useContainerNarrow } from "./useContainerNarrow";
import {
  visibleImportErrorText,
} from "@/lib/sessionImport/hostControls";

export function SessionImportHost(props: {
  locale: Locale;
  onOpenNativeSession?: (id: string) => void;
  onProjectsChanged?: () => void;
}) {
  const tr = useMemo(() => createT(props.locale), [props.locale]);
  const onReaderError = useCallback((_msg: string) => {}, []);
  const contentStates = useSessionImportContentStates(onReaderError);
  const consentLocked = useRef(false);
  const isConsentLocked = useCallback(() => consentLocked.current, []);
  const setConsentLock = useCallback((locked: boolean) => { consentLocked.current = locked; }, []);
  const ui = useSessionImportUi(props.onProjectsChanged, {
    onCandidateInvalidated: contentStates.invalidateCandidate, isLocked: isConsentLocked,
  });
  const comparison = useSessionImportComparisonDialog(ui, contentStates, setConsentLock);
  const contentUi = { ...ui, busy: ui.busy || comparison.busy };
  const wizardError = ui.error ?? ui.continuationError;
  const catalog = useImportedCatalog();
  const metadata = useImportedEntry(
    ui.readerId,
    catalog.entries.find((entry) => entry.importId === ui.readerId),
    catalog.refreshVersion,
  );
  const readerEntry = metadata.entry;
  const reader = useImportedReader(ui.readerId, onReaderError, {
    revision: ui.readerRevision,
    leaf: ui.readerLeaf,
  });
  useEffect(() => {
    if (ui.readerContinueRequested && ui.readerId && reader.scope) {
      void ui.prepareReaderContinue(ui.readerId, reader.scope.leaf, reader.scope.revision);
    }
  }, [ui.readerContinueRequested, ui.readerId, ui.prepareReaderContinue, reader.scope]);
  const extraCandidates = ui.selectionView?.domain === "fixed-groups" ? [] : extrasNotOnPage(
    ui.candidates,
    ui.scanState?.extraCandidates ?? [],
  );
  const scanGen = ui.scanGeneration?.current;
  const closeWizard = contentStates.closeWizard;
  useEffect(() => () => closeWizard?.(), [closeWizard]);
  useEffect(() => {
    if (!ui.open) closeWizard?.();
  }, [closeWizard, ui.open]);
  const [highlightId, setHighlightId] = useState<string | null>(null);
  const [focusEventId, setFocusEventId] = useState<string | null>(null);
  const focusReaderEvent = useCallback((id: string | null) => {
    setHighlightId(id);
    setFocusEventId(id);
  }, []);
  const [listFilter, setListFilter] = useState("");
  const [onlyIncomplete, setOnlyIncomplete] = useState(false);
  const [onlyArchived, setOnlyArchived] = useState(false);
  const [issuesOnly, setIssuesOnly] = useState(false);
  const readerIdentity = JSON.stringify([ui.readerId, ui.readerRevision ?? null, ui.readerLeaf ?? null]);
  const readerImages = useImportedImages(ui.readerId && reader.scope ? {
    kind: "published", importId: ui.readerId, revisionId: reader.scope.revision,
  } : null, reader.events, JSON.stringify([readerIdentity, reader.scope?.leaf]));
  const clipboard = useImportClipboard(ui.readerId ? readerIdentity : null);
  const errorClipboard = useImportClipboard(JSON.stringify([
    readerIdentity, ui.open, ui.step,
    visibleImportErrorText(wizardError), visibleImportErrorText(reader.error),
  ]));
  const errorCopyNotice = errorClipboard.status === "failed"
    ? <p role="alert">{tr("sessionImport.clipboardFailed")}</p>
    : errorClipboard.status === "copied"
      ? <p role="status">{tr("sessionImport.clipboardCopied")}</p> : null;
  const [roleFilter, setRoleFilter] = useState<"all" | "user" | "assistant">("all");
  const [readerPane, setReaderPane] = useState<ReaderPane>("events");
  const wizardBox = useContainerNarrow(ui.open);
  const [wizardFooter, setWizardFooter] = useState<HTMLDivElement | null>(null);
  const readerBox = useContainerNarrow(!!ui.readerId);
  const filterRef = useRef<HTMLInputElement>(null);
  const matchCandidate = (c: SourceCandidate) => {
    if (onlyIncomplete && c.completeness !== "prefix") return false;
    if (onlyArchived && !c.archived) return false;
    const q = listFilter.trim().toLowerCase();
    if (!q) return true;
    return `${c.title} ${c.provider} ${c.cwd ?? ""} ${c.locator}`.toLowerCase().includes(q);
  };
  const filteredCandidates = useMemo(
    () => ui.candidates.filter(matchCandidate),
    [ui.candidates, listFilter, onlyIncomplete, onlyArchived],
  );
  const filteredExtras = useMemo(
    () => extraCandidates.filter(matchCandidate),
    [extraCandidates, listFilter, onlyIncomplete, onlyArchived],
  );
  const filteredReaderEvents = useMemo(() => {
    return reader.events.filter((raw) => {
      const view = importedEventDisplay(raw);
      if (roleFilter !== "all" && view.role !== roleFilter) return false;
      if (issuesOnly && !eventHasIssue(view, readerImages.states)) return false;
      return true;
    });
  }, [reader.events, roleFilter, issuesOnly, readerImages.states]);
  const readerWindow = useMemo(
    () => windowReaderEvents(filteredReaderEvents, highlightId),
    [filteredReaderEvents, highlightId],
  );
  const readerEvents = readerWindow.items;
  const moveReaderWindow = useCallback(
    async (direction: "previous" | "next") => {
      const anchor = direction === "previous"
        ? await reader.loadPrevious()
        : await reader.loadMore();
      if (anchor) focusReaderEvent(anchor);
    },
    [reader.loadMore, reader.loadPrevious, focusReaderEvent],
  );
  const visibleIds = useMemo(
    () => [...filteredCandidates, ...filteredExtras].map((c) =>
      ui.selectionRows?.find((row) => row.candidate.candidateId === c.candidateId)?.rowId ?? c.candidateId),
    [filteredCandidates, filteredExtras, ui.selectionRows],
  );
  const titleByCandidate = useMemo(() => {
    const m = new Map<string, string>();
    for (const c of [...ui.candidates, ...extraCandidates]) m.set(c.candidateId, c.title);
    return m;
  }, [ui.candidates, extraCandidates]);
  const mappingSessions = useMemo(
    () => ui.prepareBatch
      ? ui.prepareItems.filter((item) => item.state === "prepared").slice(0, 20)
        .map((item) => ({ id: item.candidateId, title: item.title || titleByCandidate.get(item.candidateId) || item.candidateId }))
      : [...ui.selected].slice(0, 20).map((id) => ({ id, title: titleByCandidate.get(id) ?? id })),
    [ui.prepareBatch, ui.prepareItems, ui.selected, titleByCandidate],
  );
  const mappingSessionCount = ui.prepareBatch?.prepared ?? ui.selectedCount;

  useEffect(() => {
    focusReaderEvent(null);
    setRoleFilter("all");
    setIssuesOnly(false);
  }, [ui.readerId, focusReaderEvent]);

  useEffect(() => {
    const anchor = ui.readerAnchor;
    if (!anchor || !ui.readerId) {
      if (!anchor) focusReaderEvent(null);
      return;
    }
    setRoleFilter("all");
    setIssuesOnly(false);
    focusReaderEvent(anchor);
    void reader.ensureEvent(anchor);
  }, [reader.ensureEvent, ui.readerAnchor, ui.readerId, focusReaderEvent]);

  useEffect(() => {
    if (!focusEventId) return;
    const target = document.getElementById(`import-ev-${focusEventId}`);
    if (!target) return;
    const frame = window.requestAnimationFrame(() => {
      if (!target.isConnected || !isDialogFocusAllowed(target)) return;
      target.scrollIntoView({ block: "nearest" });
      target.focus({ preventScroll: true });
    });
    return () => window.cancelAnimationFrame(frame);
  }, [focusEventId, reader.events, roleFilter, issuesOnly]);

  useEffect(() => {
    const known = [...ui.candidates, ...extraCandidates];
    const seeds = [...ui.selected]
      .map((id) => known.find((c) => c.candidateId === id))
      .filter((c): c is SourceCandidate => !!c)
      .map((c) => ({
        candidateId: c.candidateId,
        sourceRevision: c.sourceRevision,
        title: c.title,
      }));
    contentStates.ensureSelected(seeds, scanGen, ui.selectionView?.scope ?? null);
  }, [
    ui.selected,
    ui.candidates,
    extraCandidates,
    contentStates.ensureSelected,
    scanGen,
    ui.selectionView?.scope.scanId,
    ui.selectionView?.scope.epoch,
    ui.selectionView?.scope.generation,
  ]);

  const revealReaderHit = async (eventId: string) => {
    // Search spans the archive, not the current display filters. Reveal the
    // selected body now; a later explicit filter change still takes precedence.
    setReaderPane("events");
    setRoleFilter("all");
    setIssuesOnly(false);
    focusReaderEvent(eventId);
    if (await reader.ensureEvent(eventId)) rememberReaderAnchor(eventId);
  };

  const jumpHit = async (dir: -1 | 1) => {
    const next = nextReaderHit(reader.hits.map((h) => h.eventId), highlightId, dir);
    if (next) await revealReaderHit(next);
  };

  const rememberReaderAnchor = (eventId: string) => {
    const scope = reader.getScope();
    if (!ui.readerId || !scope) return;
    rememberImportedHistoryAnchor({
      importId: ui.readerId,
      revision: scope.revision,
      leaf: scope.leaf,
      anchorEventId: eventId,
    });
  };

  const onReaderKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.defaultPrevented || !reader.hits.length) return;
    const direction = readerShortcutDirection(e.nativeEvent);
    if (direction == null) return;
    e.preventDefault();
    void jumpHit(direction);
  };

  const copyMarkdown = async (importId: string) => {
    const md = await sessionImportExportMarkdown(importId);
    await navigator.clipboard.writeText(md);
  };

  const copyVisibleImportError = async (error: string | null | undefined) => {
    const text = visibleImportErrorText(error);
    if (!text) return;
    await errorClipboard.copy(text);
  };

  const loadMoreScan = () => {
    if (contentUi.busy || !ui.nextCursor) return;
    void ui.loadMore();
  };

  const previousScanPage = () => {
    if (contentUi.busy || ui.pageIndex <= 0) return;
    void ui.previousPage();
  };

  const toggleCandidate = (id: string) => {
    if (contentUi.busy) return;
    void ui.toggle(id);
  };

  const peekCandidate = (c: SourceCandidate) => {
    if (contentUi.busy) return;
    if (ui.rememberPreview && !ui.rememberPreview(c.candidateId)) return;
    void contentStates.open({
      candidateId: c.candidateId,
      sourceRevision: c.sourceRevision,
      title: c.title,
    });
  };

  const commitImport = () => {
    if (ui.busy) return;
    void ui.commit();
  };

  const goStep = (step: ImportStep) => {
    if (!stepOpenable(step)) return;
    if (step !== ui.step) comparison.close();
    if ((step === "source" || step === "content") && (ui.prepareBatch || ui.pendingPlan)) {
      void ui.abandonPrepare(step);
      return;
    }
    ui.setStep(step);
  };

  const stepOpenable = (step: ImportStep) => {
    if (ui.workspaceUi?.mode && step !== "source") return false;
    if (ui.prepareActive && step !== "prepare") return false;
    if (step === "source") return true;
    if (step === "content") return !!ui.scanState || ui.candidates.length > 0;
    if (step === "prepare") return !!ui.prepareBatch;
    if (step === "mapping") return !!ui.pendingPlan || ui.mappings.length > 0;
    return !!ui.job;
  };

  return (
    <>
      <GlassModal
        open={ui.open}
        onClose={() => { comparison.close(); ui.setOpen(false); }}
        title={tr("sessionImport.title")}
        size="lg"
        wrapBody
        className={`session-import-modal session-import-wizard session-import-wizard--${ui.step}`}
        closeLabel={tr("sessionImport.close")}
        footer={ui.step === "content" ? (
          <SessionImportContentActions tr={tr} ui={contentUi} contentStates={contentStates} goStep={goStep} />
        ) : ui.step === "source" && !ui.workspaceUi?.mode ? (
          <SessionImportSourceActions
            tr={tr}
            choice={ui.choice}
            busy={ui.busy}
            scanStarting={ui.scanStarting}
            onChoose={ui.setChoice}
            onScan={() => void ui.scan()}
          />
        ) : <div ref={setWizardFooter} className="session-import__action-host" />}
      >
        <div ref={wizardBox.ref} className="session-import__wizard-measure">
        <SessionImportStepNav
          tr={tr}
          step={ui.step}
          busy={contentUi.busy}
          layout={wizardBox.narrow ? "narrow" : "wide"}
          stepOpenable={stepOpenable}
          onGo={goStep}
        />
        <SessionImportPageStatus tr={tr} step={ui.step} />
        <SessionImportJobHistory locale={props.locale} enabled={ui.canOpenJobHistory && !comparison.busy}
          onSelect={ui.openExistingJob} />
        {ui.restoreStatus ? (
          <p role="status" data-import-restore={ui.restoreStatus}>
            {tr(
              ui.restoreStatus === "plan"
                ? "sessionImport.restore.plan"
                : ui.restoreStatus === "job"
                  ? "sessionImport.restore.job"
                  : ui.restoreStatus === "prepare"
                    ? "sessionImport.restore.prepare"
                  : ui.restoreStatus === "draft"
                    ? "sessionImport.restore.draft"
                    : ui.restoreStatus === "expired"
                      ? "sessionImport.restore.expired"
                      : "sessionImport.restore.failed",
            )}
          </p>
        ) : null}
        {wizardError ? (
          <div role="alert" className="settings-row__desc">
            {tr(importErrorKey(wizardError))}
            <button
              type="button"
              className="btn btn--sm"
              data-import-error-copy="1"
              disabled={errorClipboard.status === "pending"}
              onClick={() => void copyVisibleImportError(wizardError)}
            >
              {tr("sessionImport.copyText")}
            </button>
            <details>
              <summary>{tr("error.details")}</summary>
              <pre>{wizardError}</pre>
            </details>
          </div>
        ) : null}
        {errorCopyNotice}
        {ui.step === "source" && ui.workspaceUi?.mode ? (
          <SessionImportWorkspacePanel ui={ui.workspaceUi} tr={tr} projects={ui.projects} registrations={ui.registrations} footerTarget={wizardFooter} />
        ) : null}
        {ui.step === "source" && !ui.workspaceUi?.mode ? (
          <SessionImportSourceStep
            tr={tr}
            roots={ui.roots}
            registrations={ui.registrations}
            choice={ui.choice}
            busy={ui.busy}
            onChoose={ui.setChoice}
            onPickFolder={(provider) => ui.pickCustomRoot(provider)}
            onPickFile={() => ui.pickCustomFile()}
            fileFlight={ui.filePickerFlight}
            onPickWorkspaceFolder={() => ui.pickWorkspaceFolder()}
            onWorkspaces={() => void ui.workspaceUi.list()}
            onRelocate={(reg) => ui.relocateRegistered(reg)}
            onScan={() => void ui.scan()}
          />
        ) : null}
        {ui.step === "content" ? (
          <SessionImportContentStep
            tr={tr}
            ui={contentUi}
            contentStates={contentStates}
            filteredCandidates={filteredCandidates}
            filteredExtras={filteredExtras}
            extraCandidates={extraCandidates}
            visibleIds={visibleIds}
            listFilter={listFilter}
            setListFilter={setListFilter}
            onlyIncomplete={onlyIncomplete}
            setOnlyIncomplete={setOnlyIncomplete}
            onlyArchived={onlyArchived}
            setOnlyArchived={setOnlyArchived}
            filterRef={filterRef}
            loadMoreScan={loadMoreScan}
            previousScanPage={previousScanPage}
            toggleCandidate={toggleCandidate}
            peekCandidate={peekCandidate}
            copyVisibleImportError={copyVisibleImportError}
            layout={wizardBox.narrow ? "narrow" : "wide"}
            onCompare={comparison.open}
            revealSerial={comparison.revealSerial}
          />
        ) : null}
        {ui.step === "prepare" && ui.prepareBatch ? (
          <SessionImportPrepareStep
            footerTarget={wizardFooter}
            tr={tr}
            status={ui.prepareBatch}
            items={ui.prepareItems}
            itemsError={ui.prepareItemsError}
            nextCursor={ui.prepareNextCursor}
            busy={ui.busy}
            active={ui.prepareActive}
            onCancel={() => void ui.cancelPrepare()}
            onRetry={() => void ui.retryPrepare()}
            onContinue={() => void ui.finishPrepare()}
            onBack={() => void ui.abandonPrepare("content")}
            onLoadMore={() => void ui.loadMorePrepare()}
            onRetryItems={() => void ui.retryPrepareItems()}
          />
        ) : null}
        {ui.step === "mapping" ? (
          <SessionImportMappingStep
            footerTarget={wizardFooter}
            tr={tr} ui={ui} mappingSessions={mappingSessions} mappingSessionCount={mappingSessionCount}
            commitImport={commitImport}
            onBack={() => void ui.abandonPrepare(ui.scanState ? "content" : "source")}
          />
        ) : null}
        {(ui.step === "result" || ui.step === "import") && ui.job ? (
          <SessionImportResults
            footerTarget={wizardFooter}
            key={ui.job.jobId}
            tr={tr}
            job={ui.job}
            titleByCandidate={titleByCandidate}
            active={ui.jobActive}
            busy={ui.busy}
            itemsStale={ui.jobItemsStale}
            hasPrevious={ui.jobHasPrevious}
            onPrevious={() => void ui.previousJobItems()}
            onNext={() => void ui.loadMoreJobItems()}
            onRefresh={() => void ui.refreshJobItems()}
            onCancel={() => void ui.cancelJob()}
            onRetry={() => void ui.retryFailed()}
            onUndo={() => void ui.undoJob()}
            onDone={() => ui.setOpen(false)}
            onOpen={(importId) => { ui.setOpen(false); openImportedHistory({ importId }); }}
            onContinue={(importId) => { ui.setOpen(false); openImportedHistory({ importId, continue: true }); }}
            onExport={copyMarkdown}
          />
        ) : null}
        </div>
      </GlassModal>

      {comparison.dialog ? <SessionImportSourceComparisonDialog key={comparison.dialog.key}
        context={comparison.dialog.context} tr={tr} onClose={comparison.close} adoption={comparison} /> : null}
      <GlassModal open={!!comparison.consent} onClose={comparison.cancelConsent}
        title={tr("sessionImport.comparison.dirtyTitle")} closeLabel={tr("sessionImport.close")}
        size="sm" wrapBody className="session-import-adoption-consent"
        footer={<>
          <button type="button" className="btn btn--ghost" onClick={comparison.cancelConsent}>{tr("common.cancel")}</button>
          <button type="button" className="btn btn--primary" onClick={comparison.confirmConsent}>{tr("sessionImport.comparison.confirmAdopt")}</button>
        </>}>
        <p>{tr("sessionImport.comparison.dirtyNotice")}</p>
      </GlassModal>

      <GlassModal
        open={!!ui.readerId}
        onClose={() => ui.setReaderId(null)}
        title={readerEntry?.alias || readerEntry?.title || tr("sessionImport.importedGroup")}
        size="lg"
        wrapBody
        className="session-import-modal session-import-reader"
        footer={
          <ImportedReaderActions
            tr={tr}
            busy={!ui.readerId || !reader.scope || reader.busy}
            continueBusy={!ui.readerId || !reader.scope || reader.busy || ui.busy}
            hasPrevious={reader.previousCursor != null}
            hasNext={reader.nextCursor != null}
            hasText={!reader.error && readerEvents.length > 0}
            hasIssue={!reader.error && readerEvents.some((raw) => eventHasIssue(importedEventDisplay(raw), readerImages.states))}
            copyStatus={clipboard.status}
            exportNotice={reader.notice}
            exportError={reader.exportError ?? null}
            onPage={(direction) => void moveReaderWindow(direction)}
            onCopy={() => void clipboard.copy(readerEvents.map((raw) => eventPlainText(importedEventDisplay(raw))).filter(Boolean).join("\n\n"))}
            onIssue={() => {
              const first = readerEvents.find((raw) => eventHasIssue(importedEventDisplay(raw), readerImages.states));
              const id = first ? importedEventDisplay(first).id : null;
              if (id) focusReaderEvent(id);
            }}
            onExport={() => void reader.exportMarkdown()}
            onContinue={() => {
              const scope = reader.getScope();
              if (ui.readerId && scope) void ui.prepareContinue(ui.readerId, scope.leaf, scope.revision);
            }}
            onKeyDown={onReaderKeyDown}
          />
        }
        closeLabel={tr("sessionImport.close")}
      >
        <div
          ref={readerBox.ref}
          onKeyDown={onReaderKeyDown}
        >
        <p className="session-import__read-only">{tr("sessionImport.readOnly")}</p>
        {ui.readerId ? <ImportedRevisionPicker key={`revisions:${ui.readerId}`} importId={ui.readerId}
          revision={ui.readerRevision ?? reader.scope?.revision ?? null} locale={props.locale} /> : null}
        {!reader.error ? <ImportedRevisionNotice key={`health:${readerIdentity}`} health={reader.health} tr={tr} /> : null}
        {ui.readerId ? <ImportedWorkspaceAssociation key={ui.readerId} importId={ui.readerId} locale={props.locale} /> : null}
        {readerEntry ? (
          <p className="settings-row__desc">
            {tr("sessionImport.readerMeta", {
              provider: readerEntry.provider,
              id: readerEntry.nativeSessionId,
              when: (readerEntry.createdAt ?? "").slice(0, 10),
            })}
          </p>
        ) : null}
        {metadata.error && !reader.error ? (
          <p role="alert" className="settings-row__desc">
            {tr("sessionImport.catalogError")}
            <button type="button" className="btn btn--sm btn--ghost" disabled={metadata.busy} onClick={metadata.reload}>
              {tr("sessionImport.refresh")}
            </button>
          </p>
        ) : null}
        {ui.continuationError ? (
          <p role="alert" className="settings-row__desc">
            {tr(importErrorKey(ui.continuationError))}
            <button
              type="button"
              className="btn btn--sm btn--ghost"
              data-import-error-copy="1"
              disabled={errorClipboard.status === "pending"}
              onClick={() => void copyVisibleImportError(ui.continuationError)}
            >
              {tr("sessionImport.copyText")}
            </button>
          </p>
        ) : null}
        <ImportedReaderError
          key={`recovery:${readerIdentity}`}
          tr={tr}
          error={reader.error}
          busy={!ui.readerId || reader.busy}
          copyBusy={errorClipboard.status === "pending"}
          onReload={() => {
            if (metadata.error) metadata.reload();
            reader.reload();
          }}
          onCopy={() => copyVisibleImportError(reader.error)}
        />
        {errorCopyNotice}
        {!reader.error ? <ImportedCompactionHistory importId={ui.readerId} revision={reader.scope?.revision ?? null}
          leaf={ui.readerLeaf ?? null} events={reader.events} busy={reader.busy || ui.busy} tr={tr} /> : null}
        <SessionImportReaderContent
          tr={tr} reader={reader} readerIdentity={readerIdentity} readerEvents={readerEvents} images={readerImages}
          eventsWindowed={readerWindow.windowed} roleFilter={roleFilter} setRoleFilter={setRoleFilter}
          issuesOnly={issuesOnly} setIssuesOnly={setIssuesOnly} highlightId={highlightId}
          readerPane={readerPane} setReaderPane={setReaderPane} narrow={readerBox.narrow}
          jumpHit={jumpHit} revealReaderHit={revealReaderHit}
          onContinueFrom={(eventId) => {
            const scope = reader.getScope();
            if (!ui.readerId || !scope) return;
            // Keep the real trigger focused while preparation is asynchronous.
            // Highlighting a source is not a request to navigate keyboard focus.
            setFocusEventId(null);
            setHighlightId(eventId);
            void ui.prepareContinue(ui.readerId, eventId, scope.revision);
          }}
        />

        </div>
      </GlassModal>

      <SessionImportContinuationModal ui={ui} locale={props.locale} onOpenNativeSession={props.onOpenNativeSession} />
    </>
  );
}
