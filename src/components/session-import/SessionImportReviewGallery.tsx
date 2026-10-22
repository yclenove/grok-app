import { useEffect, useMemo, useRef, useState, type ComponentProps } from "react";
import { GlassModal } from "@/components/GlassModal";
import { Select } from "@/components/Select";
import { createT, isLocale, LOCALES, type Locale } from "@/i18n";
import type { ImportStep } from "@/hooks/useSessionImport";
import { useSessionImportContentStates } from "@/hooks/useSessionImportContentStates";
import { useWorkspaceImport } from "@/hooks/useWorkspaceImport";
import type { SourceChoice } from "@/lib/sessionImport/sourceChoice";
import { ContinuationDraftBanner, type ContinuationDraftClient } from "./ContinuationDraftBanner";
import { SessionImportReviewCatalog } from "./SessionImportReviewCatalog";
import { SessionImportReviewReader } from "./SessionImportReviewReader";
import { SessionImportReviewAssociation } from "./SessionImportReviewAssociation";
import { SessionImportReviewContinuation } from "./SessionImportReviewContinuation";
import {
  SessionImportContentActions,
  SessionImportContentStep,
} from "./SessionImportContentStep";
import { SessionImportMappingStep } from "./SessionImportMappingStep";
import { useSessionImportAssociations, type AssociationClient } from "@/hooks/useSessionImportAssociations";
import { SessionImportPageStatus } from "./SessionImportPageStatus";
import { SessionImportPrepareStep } from "./SessionImportPrepareStep";
import { SessionImportResults } from "./SessionImportResults";
import { SessionImportSourceActions, SessionImportSourceStep } from "./SessionImportSourceStep";
import { SessionImportStepNav } from "./SessionImportStepNav";
import { SessionImportWorkspacePanel } from "./SessionImportWorkspacePanel";
import { createReviewWorkspaceClient, REVIEW_WORKSPACE_FILE, reviewWorkspaceProjects } from "./sessionImportReviewWorkspaceClient";
import { useContainerNarrow } from "./useContainerNarrow";
import { useReviewLocaleCatalog } from "./useReviewLocaleCatalog";
import { useReviewMetrics } from "./useReviewMetrics";
import {
  candidate, catalogEntry, draft, jobFor, LONG_PATH, prepareItems, prepareStatus,
  registration, reviewCandidates, reviewCatalogEntries, reviewPreviewClient, root, SYNTHETIC_PROJECT,
} from "./sessionImportReviewFixtures";

const SURFACES = ["source", "content", "prepare", "mapping", "import", "result", "catalog", "reader", "association", "continuation", "saved-context"] as const;
type Surface = (typeof SURFACES)[number];
type ReaderState = "idle" | "searching" | "empty" | "error" | "hits";
type Theme = "dark" | "light";

const WIZARD: ImportStep[] = ["source", "content", "prepare", "mapping", "import", "result"];

// This isolated gallery never contacts the real history catalog.
const galleryHistoryClient: AssociationClient = {
  entries: async () => ({ items: [], nextCursor: null, total: 0 }),
  entry: async () => null,
  inspect: async () => { throw new Error("FORMAT_UNSUPPORTED: synthetic gallery has no historical identity pair"); },
};

function isSurface(value: string | null): value is Surface {
  return !!value && (SURFACES as readonly string[]).includes(value);
}

function readReview() {
  const params = new URLSearchParams(typeof window === "undefined" ? "" : window.location.search);
  const surface = isSurface(params.get("surface")) ? params.get("surface") as Surface : "content";
  const localeRaw = params.get("locale");
  const locale: Locale = localeRaw && isLocale(localeRaw) ? localeRaw : "en";
  const theme: Theme = params.get("theme") === "light" ? "light" : "dark";
  const readerRaw = params.get("reader");
  const reader: ReaderState = readerRaw === "searching" || readerRaw === "empty" || readerRaw === "error" || readerRaw === "hits"
    ? readerRaw : "idle";
  return {
    surface,
    locale,
    theme,
    reader,
    review: params.get("review") === "1",
    conflict: params.get("conflict") === "1",
    locked: params.get("locked") === "1",
    controls: params.get("controls") === "1",
    failCreateOnce: params.get("creation") === "retry",
    workspaceConflict: params.get("workspace") === "conflict",
    relocationRetry: params.get("relocation") === "retry",
  };
}



/**
 * Isolated review entry. It renders production import components with synthetic
 * adapters and does not start a scan, import, or provider call. Step
 * availability here is not the product goStep/stepOpenable gate.
 */
export function SessionImportReviewGallery() {
  const initial = readReview();
  const [surface, setSurface] = useState<Surface>(initial.surface);
  const [locale, setLocale] = useState<Locale>(initial.locale);
  const [theme, setTheme] = useState<Theme>(initial.theme);
  const [reader, setReader] = useState<ReaderState>(initial.reader);
  const [open, setOpen] = useState(true);
  const [wizardFooter, setWizardFooter] = useState<HTMLDivElement | null>(null);
  const [filePicker, setFilePicker] = useState(false);
  const [listFilter, setListFilter] = useState("");
  const [onlyIncomplete, setOnlyIncomplete] = useState(false);
  const [onlyArchived, setOnlyArchived] = useState(false);
  const [project, setProject] = useState("");
  const [choice, setChoice] = useState<SourceChoice | null>(null);
  const [selected, setSelected] = useState(() => new Set(reviewCandidates.map((item) => item.candidateId)));
  const [preparing, setPreparing] = useState(prepareStatus);
  const [preparedItems, setPreparedItems] = useState(prepareItems);
  const associations = useSessionImportAssociations(preparing, open && surface === "mapping", () => false, galleryHistoryClient);
  const [job, setJob] = useState(() => jobFor(initial.surface));
  const [runImport, setRunImport] = useState(false);
  const [openedEntry, setOpenedEntry] = useState(catalogEntry);
  const savedDraft = useRef(draft);
  const draftVersion = useRef(0);
  const contentStates = useSessionImportContentStates(undefined, reviewPreviewClient);
  const workspaceClient = useMemo(() => createReviewWorkspaceClient({
    conflictOnce: initial.workspaceConflict, relocationRetry: initial.relocationRetry,
  }), [initial.workspaceConflict, initial.relocationRetry]);
  const workspaceUi = useWorkspaceImport(undefined, workspaceClient.client);
  const filterRef = useRef<HTMLInputElement>(null);
  const tr = useMemo(() => createT(locale), [locale]);
  const catalog = useReviewLocaleCatalog(locale);
  const measured = useContainerNarrow(open && catalog.ready);
  const wizard = (WIZARD as string[]).includes(surface);
  const layout = measured.narrow ? "narrow" : "wide";
  const metrics = useReviewMetrics({
    open, surface, locale, theme, reader, layout,
    measureWidth: measured.width, localeReady: catalog.ready,
  });
  const client = useMemo<ContinuationDraftClient>(() => ({
    async get() {
      if (initial.conflict) throw new Error("HANDOFF_CHANGED: synthetic");
      if (initial.locked) return { ...draft, text: null, textHash: null };
      return savedDraft.current;
    },
    async save(_id, expected, text) {
      if (initial.conflict || initial.locked || expected !== savedDraft.current.textHash) {
        throw new Error("HANDOFF_CHANGED: synthetic");
      }
      savedDraft.current = { ...savedDraft.current, text, textHash: `synthetic-${++draftVersion.current}` };
      return savedDraft.current;
    },
  }), [initial.conflict, initial.locked]);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);

  useEffect(() => {
    if (!runImport || job.state !== "running" || surface !== "import") return;
    const timer = window.setTimeout(() => {
      setJob(jobFor("result")); setSurface("result"); setRunImport(false);
    }, 1000);
    return () => window.clearTimeout(timer);
  }, [job.state, runImport, surface]);

  useEffect(() => {
    contentStates.ensureSelected(reviewCandidates.filter((item) => selected.has(item.candidateId)));
  }, [contentStates.ensureSelected, selected]);

  const toggleCandidate = (id: string) => setSelected((current) => {
    const next = new Set(current);
    if (next.has(id)) next.delete(id); else next.add(id);
    return next;
  });
  const filteredCandidates = reviewCandidates.filter((item) =>
    (!onlyIncomplete || item.completeness === "prefix") && (!onlyArchived || item.archived) &&
    `${item.title} ${item.provider} ${item.cwd} ${item.locator}`.toLowerCase().includes(listFilter.trim().toLowerCase()),
  );
  const unavailable = () => { throw new Error("Synthetic review action has no live adapter"); };
  const openReviewEntry = (id: string, next: "reader" | "continuation") => {
    const entry = reviewCatalogEntries.find((item) => item.importId === id);
    if (!entry) throw new Error("EVENT_MISSING: synthetic catalog entry");
    setOpenedEntry(entry); setSurface(next);
  };
  const retryPrepare = () => {
    setPreparing((current) => ({ ...current, state: "completed", prepared: current.total, failed: 0, skipped: 0 }));
    setPreparedItems((items) => items.map((item) => ({ ...item, state: "prepared", error: null })));
  };

  type ContentProps = ComponentProps<typeof SessionImportContentStep>;
  const contentUi = {
    scanIssues: [],
    discoveredCount: reviewCandidates.length,
    selectedCount: selected.size,
    issueCount: 0,
    selected,
    scanCancelling: false,
    scanning: false,
    scanPaused: false,
    scanFailed: false,
    scanState: { state: "completed" },
    candidates: reviewCandidates,
    busy: false,
    nextCursor: null,
    pageIndex: 0,
    extraCount: 0,
    cancelScan: unavailable,
    continueScan: unavailable,
    selectCurrent: async () => setSelected(new Set(reviewCandidates.map((item) => item.candidateId))),
    clearSelected: async () => setSelected(new Set()),
    selectThese: async (ids: string[]) => setSelected((current) => new Set([...current, ...ids])),
    invertThese: async (ids: string[]) => setSelected((current) => {
      const next = new Set(current);
      for (const id of ids) { if (next.has(id)) next.delete(id); else next.add(id); }
      return next;
    }),
    refreshScan: async () => contentStates.closeWizard(),
    previewSelected: async () => {
      const items = prepareItems.filter((item) => selected.has(item.candidateId));
      const prepared = items.filter((item) => item.state === "prepared").length;
      setPreparing({ ...prepareStatus, total: items.length, prepared, failed: items.length - prepared });
      setPreparedItems(items);
      setSurface("prepare");
    },
  } satisfies ContentProps["ui"];

  const modalClass = surface === "reader"
    ? "session-import-modal session-import-reader"
    : surface === "catalog"
      ? "session-import-modal session-import-catalog-modal"
      : surface === "saved-context"
        ? "session-import-modal session-import-continuation"
        : `session-import-modal session-import-wizard session-import-wizard--${surface}`;

  const footer = surface === "source" && !workspaceUi.mode ? (
    <SessionImportSourceActions
      tr={tr}
      choice={choice}
      busy={false}
      scanStarting={false}
      onChoose={setChoice}
      onScan={() => setSurface("content")}
    />
  ) : surface === "content" ? (
    <SessionImportContentActions
      tr={tr}
      ui={contentUi}
      contentStates={contentStates}
      goStep={(step) => setSurface(step)}
    />
  ) : wizard ? <div ref={setWizardFooter} className="session-import__action-host" /> : undefined;

  return (
    <div data-design-gallery="review" data-review-gate="synthetic">
      <aside className="session-import-review-controls" hidden={!initial.controls}>
        <p className="session-import-gallery__mock" role="note">{tr("sessionImport.gallery.synthetic")}</p>
        <fieldset className="session-import__filters">
          <label>
            Surface
            <Select aria-label="Review surface" value={surface}
              options={SURFACES.map((value) => ({ value, label: value }))}
              onChange={(value) => { if (isSurface(value)) setSurface(value); }} />
          </label>
          <label>
            Locale
            <Select aria-label="Review locale" value={locale}
              options={LOCALES.map((value) => ({ value, label: value }))}
              onChange={(value) => { if (isLocale(value)) setLocale(value); }} />
          </label>
          <label>
            Theme
            <Select aria-label="Review theme" value={theme}
              options={["dark", "light"].map((value) => ({ value, label: value }))}
              onChange={(value) => setTheme(value as Theme)} />
          </label>
          {surface === "reader" ? (
            <label>
              Reader
              <Select aria-label="Review reader state" value={reader}
                options={["idle", "searching", "empty", "error", "hits"].map((value) => ({ value, label: value }))}
                onChange={(value) => setReader(value as ReaderState)} />
            </label>
          ) : null}
        </fieldset>
      </aside>
      <pre hidden id="session-import-review-metrics">{metrics}</pre>
      {!catalog.ready ? (
        <div role="status" aria-busy={!catalog.error} data-review-state={catalog.error ? "locale-error" : "locale-loading"}>
          {catalog.error ? <button type="button" className="btn" onClick={catalog.retry}>{tr("ui.errorBoundary.retry")}</button> : "…"}
        </div>
      ) : open && surface === "reader" ? (
        <SessionImportReviewReader key={`${openedEntry.importId}:${locale}:${reader}`} entry={openedEntry} locale={locale} theme={theme} state={reader}
          onClose={() => setOpen(false)} onContinue={() => setSurface("continuation")} />
      ) : open && surface === "continuation" ? (
        <SessionImportReviewContinuation key={`${openedEntry.importId}:${initial.locked}:${initial.failCreateOnce}`} entry={openedEntry} locale={locale} theme={theme}
          locked={initial.locked} failCreateOnce={initial.failCreateOnce} onCreated={(view) => {
            savedDraft.current = view; setSurface("saved-context");
          }} />
      ) : open ? (
        <GlassModal
          open
          onClose={() => setOpen(false)}
          title={tr("sessionImport.title")}
          size="lg"
          wrapBody
          dataTheme={theme}
          className={`${modalClass} session-import-review-frame`}
          closeLabel={tr("sessionImport.close")}
          footer={footer}
        >
          <div
            ref={measured.ref}
            className="session-import__wizard-measure"
            data-review-surface={surface}
            data-layout={layout}
            data-measured-width={measured.width}
          >
            {wizard ? (
              <>
                <SessionImportStepNav
                  tr={tr}
                  step={surface as ImportStep}
                  busy={workspaceUi.busy}
                  layout={layout}
                  portalTheme={theme}
                  stepOpenable={(step) => !workspaceUi.mode || step === "source"}
                  onGo={(step) => { if (!workspaceUi.mode || step === "source") setSurface(step); }}
                />
                <SessionImportPageStatus tr={tr} step={surface as ImportStep} />
              </>
            ) : null}
            {surface === "source" && workspaceUi.mode ? (
              <SessionImportWorkspacePanel ui={workspaceUi} tr={tr} projects={reviewWorkspaceProjects}
                registrations={[registration]} footerTarget={wizardFooter} relocationClient={workspaceClient.relocation} />
            ) : null}
            {surface === "source" && !workspaceUi.mode ? (
              <SessionImportSourceStep
                tr={tr}
                roots={[root]}
                registrations={[registration]}
                choice={choice}
                busy={false}
                onChoose={setChoice}
                onPickFolder={async (provider) => setChoice({ kind: "custom-folder", provider, path: root.path })}
                onPickFile={async () => setFilePicker(true)}
                onPickWorkspaceFolder={() => workspaceUi.load({ kind: "folder", path: SYNTHETIC_PROJECT })}
                onWorkspaces={() => void workspaceUi.list()}
                onRelocate={async (reg) => setChoice({ kind: "relocate", provider: reg.provider, sourceId: reg.sourceId, path: root.path })}
                onScan={() => setSurface("content")}
              />
            ) : null}
            {surface === "content" ? (
              <SessionImportContentStep
                tr={tr}
                ui={contentUi}
                contentStates={contentStates}
                filteredCandidates={filteredCandidates}
                filteredExtras={[]}
                extraCandidates={[]}
                visibleIds={filteredCandidates.map((item) => item.candidateId)}
                listFilter={listFilter}
                setListFilter={setListFilter}
                onlyIncomplete={onlyIncomplete}
                setOnlyIncomplete={setOnlyIncomplete}
                onlyArchived={onlyArchived}
                setOnlyArchived={setOnlyArchived}
                filterRef={filterRef}
                loadMoreScan={unavailable}
                previousScanPage={unavailable}
                toggleCandidate={toggleCandidate}
                peekCandidate={(item) => void contentStates.open(item)}
                copyVisibleImportError={unavailable}
                layout={layout}
              />
            ) : null}
            {surface === "prepare" ? (
              <SessionImportPrepareStep
                footerTarget={wizardFooter}
                tr={tr}
                status={preparing}
                items={preparedItems}
                itemsError={false}
                nextCursor={null}
                busy={false}
                active={false}
                onCancel={() => setPreparing((current) => ({ ...current, state: "cancelled" }))}
                onRetry={retryPrepare}
                onContinue={() => setSurface("mapping")}
                onBack={() => setSurface("content")}
                onLoadMore={unavailable}
                onRetryItems={retryPrepare}
              />
            ) : null}
            {surface === "mapping" ? (
              <SessionImportMappingStep
                footerTarget={wizardFooter}
                tr={tr}
                ui={{
                  busy: false, prepareBatch: preparing, pendingPlan: null, commitPhase: null,
                  associations, prepareItems: preparedItems, prepareNextCursor: null, prepareItemsError: false,
                  loadMorePrepare: async () => false, retryPrepareItems: async () => false, explicitReimport: false, setExplicitReimport: () => {},
                  projectId: null, projects: [{ id: "proj-synthetic", name: "Synthetic project" }],
                  registrations: [registration],
                  mappings: [{ rootId: root.path, targetProjectId: project || null, unassigned: !project, userChoiceVersion: 1 }],
                  setMappingProject: (_rootId, id) => setProject(id ?? ""),
                }}
                mappingSessions={preparedItems.filter((item) => item.state === "prepared")
                  .map((item) => ({ id: item.candidateId, title: item.title ?? item.candidateId }))}
                mappingSessionCount={preparing.prepared}
                commitImport={() => { setJob(jobFor("import")); setSurface("import"); setRunImport(true); }}
                onBack={() => setSurface("prepare")}
              />
            ) : null}
            {surface === "import" || surface === "result" ? (
              <SessionImportResults
                footerTarget={wizardFooter}
                tr={tr}
                job={job}
                titleByCandidate={new Map([["synthetic-1", candidate.title], ["synthetic-2", "Unreadable archive"]])}
                active={job.state === "running"}
                busy={false}
                itemsStale={false}
                hasPrevious={false}
                onPrevious={unavailable}
                onNext={unavailable}
                onRefresh={() => setJob((current) => ({ ...current, stateVersion: current.stateVersion + 1 }))}
                onCancel={() => {
                  setJob((current) => ({ ...current, state: "cancelled", stateVersion: current.stateVersion + 1,
                    items: current.items.map((item) => item.phase === "running" ? { ...item, phase: "cancelled" } : item) }));
                  setSurface("result");
                }}
                onRetry={() => {
                  setJob((current) => ({ ...jobFor("result"), state: "completed", stateVersion: current.stateVersion + 1,
                    progress: { done: current.items.length, retryable: 0 },
                    items: current.items.map((item) => item.phase === "failed" ? {
                      ...item, phase: "committed", error: null, outcome: "complete",
                      importId: "imp:00000000-0000-4000-8000-000000000002",
                    } : item) }));
                }}
                onUndo={() => setJob((current) => ({ ...current, state: "undone", stateVersion: current.stateVersion + 1,
                  items: current.items.map((item) => item.phase === "committed" ? { ...item, phase: "undone" } : item) }))}
                onDone={() => setOpen(false)}
                onOpen={(id) => openReviewEntry(id, "reader")}
                onContinue={(id) => openReviewEntry(id, "continuation")}
                onExport={async () => unavailable()}
              />
            ) : null}
            {surface === "catalog" ? (
              <SessionImportReviewCatalog tr={tr} onOpen={(target) => openReviewEntry(target.importId, "reader")}
                onContinue={(id) => openReviewEntry(id, "continuation")} />
            ) : null}
            {surface === "association" ? <SessionImportReviewAssociation tr={tr} /> : null}
            {surface === "saved-context" ? (
              <ContinuationDraftBanner
                sessionId="synthetic-session"
                locale={locale}
                empty
                sessionState="idle"
                client={client}
                startOpen={initial.review}
                dataTheme={theme}
                onOpenHistory={(target) => openReviewEntry(target.importId, "reader")}
              />
            ) : null}
          </div>
        </GlassModal>
      ) : (
        <button type="button" onClick={() => setOpen(true)}>{tr("sessionImport.title")}</button>
      )}
      <GlassModal open={filePicker} onClose={() => setFilePicker(false)} title={tr("sessionImport.fileMode")}
        size="lg" wrapBody dataTheme={theme} closeLabel={tr("common.close")} className="session-import-modal">
        <p className="settings-row__desc">{tr("sessionImport.gallery.synthetic")}</p>
        <ul className="session-import__roots">
          <li><button type="button" className="btn" onClick={() => {
            setFilePicker(false); void workspaceUi.load({ kind: "file", path: REVIEW_WORKSPACE_FILE });
          }}><span className="session-import__locator">{REVIEW_WORKSPACE_FILE}</span></button></li>
          <li><button type="button" className="btn" onClick={() => {
            setFilePicker(false); setChoice({ kind: "custom-file", provider: "generic-file", path: LONG_PATH });
          }}><span className="session-import__locator">{LONG_PATH}</span></button></li>
        </ul>
      </GlassModal>
    </div>
  );
}
