import { useLayoutEffect, useRef, useState, type Dispatch, type RefObject, type SetStateAction } from "react";
import type { SourceCandidate } from "@/lib/api/sessionMigration";
import type { ImportSelectionRow } from "@/lib/sessionImport/selectionView";
import { isDialogFocusAllowed, isTypingTarget, listFocusable } from "@/lib/a11yFocus";
import { importErrorKey, importIssueKey } from "@/lib/sessionImport/errorCopy";
import { allowedSelectionOf } from "@/lib/sessionImport/contentStates";
import { activationOwningTarget, contentKeyCommand } from "@/lib/sessionImport/hostControls";
import { scanUiStatus } from "@/lib/sessionImport/scanStatus";
import type { ImportStep } from "@/hooks/useSessionImport";
import { useSessionImportUi } from "@/hooks/useSessionImport";
import { useSessionImportContentStates } from "@/hooks/useSessionImportContentStates";
import { SessionImportContentDetail } from "./SessionImportContentDetail";
import { SessionImportCandidateRow } from "./SessionImportCandidateRow";
import { SessionImportGroupMemberPicker } from "./SessionImportGroupMemberPicker";
import { copyComparisonContext, type ComparisonPreviewContext } from "@/lib/sessionImport/comparisonPreviewState";
import { Select } from "@/components/Select";
import { outcomeLabel, type SessionImportTFn } from "./sessionImportHostLabels";

type ImportUi = ReturnType<typeof useSessionImportUi>;
type Ui = Pick<ImportUi,
  "busy" | "cancelScan" | "candidates" | "continueScan" |
  "discoveredCount" | "extraCount" | "issueCount" | "nextCursor" |
  "pageIndex" | "previewSelected" | "scanCancelling" | "scanFailed" |
  "scanIssues" | "scanning" | "scanPaused" | "selected" |
  "selectedCount"> & {
    clearSelected: () => void | Promise<unknown>;
    selectCurrent: () => void | Promise<unknown>;
    selectThese: (ids: string[]) => void | Promise<unknown>;
    invertThese: (ids: string[]) => void | Promise<unknown>;
    refreshScan: () => void | Promise<unknown>;
    scanState: Pick<NonNullable<ImportUi["scanState"]>, "state"> | null;
    selectionRows?: ImportSelectionRow[];
    selectedRows?: Set<string>;
    selectionReady?: boolean;
    selectionView?: ImportUi["selectionView"];
    chooseGroupMember?: ImportUi["chooseGroupMember"];
    useIndependentCandidates?: ImportUi["useIndependentCandidates"];
    useFixedGroups?: ImportUi["useFixedGroups"];
  };
type ContentStates = ReturnType<typeof useSessionImportContentStates>;

export function SessionImportContentStep(props: {
  tr: SessionImportTFn;
  ui: Ui;
  contentStates: ContentStates;
  filteredCandidates: SourceCandidate[];
  filteredExtras: SourceCandidate[];
  extraCandidates: SourceCandidate[];
  visibleIds: string[];
  listFilter: string;
  setListFilter: Dispatch<SetStateAction<string>>;
  onlyIncomplete: boolean;
  setOnlyIncomplete: Dispatch<SetStateAction<boolean>>;
  onlyArchived: boolean;
  setOnlyArchived: Dispatch<SetStateAction<boolean>>;
  filterRef: RefObject<HTMLInputElement | null>;
  loadMoreScan: () => void;
  previousScanPage: () => void;
  toggleCandidate: (id: string) => void;
  peekCandidate: (c: SourceCandidate) => void;
  copyVisibleImportError: (error: string | null | undefined) => void;
  layout?: "wide" | "narrow";
  onCompare?: (context: ComparisonPreviewContext) => void;
  revealSerial?: number;
}) {
  const {
    tr,
    ui,
    contentStates,
    filteredCandidates,
    filteredExtras,
    extraCandidates,
    visibleIds,
    listFilter,
    setListFilter,
    onlyIncomplete,
    setOnlyIncomplete,
    onlyArchived,
    setOnlyArchived,
    filterRef,
    loadMoreScan,
    previousScanPage,
    toggleCandidate,
    peekCandidate,
    copyVisibleImportError,
    layout = "wide",
  } = props;
  const [contentPane, setContentPane] = useState<"list" | "preview">("list");
  const [memberMenuId, setMemberMenuId] = useState<string | null>(null);
  const lastReveal = useRef(props.revealSerial ?? 0);
  useLayoutEffect(() => {
    const next = props.revealSerial ?? 0;
    if (next !== lastReveal.current) {
      lastReveal.current = next;
      setContentPane("preview");
    }
  }, [props.revealSerial]);
  const showPreview = (candidate: SourceCandidate) => {
    if (layout === "narrow") setContentPane("preview");
    peekCandidate(candidate);
  };
  const peekSlot = contentStates.active;
  const contentRef = useRef<HTMLDivElement>(null);
  const listPaneRef = useRef<HTMLElement>(null);
  const previewPaneRef = useRef<HTMLElement>(null);
  const layoutWas = useRef(layout);
  const listHidden = layout === "narrow" && contentPane !== "list";
  const previewHidden = layout === "narrow" && contentPane !== "preview";
  const candidateFocusRef = useRef<HTMLElement | null>(null);
  const paginationRef = useRef<HTMLDivElement>(null);
  const paginationFocusRef = useRef<HTMLElement | null>(null);
  const [focusedId, setFocusedId] = useState<string | null>(null);
  const candidates = [...filteredCandidates, ...filteredExtras];
  const rowIdOf = (c: SourceCandidate) => ui.selectionRows?.find((row) => row.candidate.candidateId === c.candidateId)?.rowId ?? c.candidateId;
  const focusIndex = Math.max(0, candidates.findIndex((c) => rowIdOf(c) === focusedId));
  const focused = candidates[focusIndex];
  const focusCandidate = (index: number) => {
    if (listHidden || !listPaneRef.current || !isDialogFocusAllowed(listPaneRef.current)) return;
    const row = listPaneRef.current?.querySelectorAll<HTMLLIElement>("[data-import-candidate]")[index];
    row?.focus({ preventScroll: true });
    row?.scrollIntoView({ block: "nearest" });
  };

  useLayoutEffect(() => {
    // Paging and moving between the main/extras lists remove focused DOM nodes.
    // Repair that loss only; never pull focus back from another active control.
    const previous = candidateFocusRef.current;
    if (!previous || previous.isConnected || document.activeElement !== document.body) return;
    candidateFocusRef.current = null;
    if (!contentRef.current || !isDialogFocusAllowed(contentRef.current)) return;
    if (listHidden) {
      const visible = previewPaneRef.current;
      const next = (visible && listFocusable(visible)[0]) || visible;
      next?.focus({ preventScroll: true });
      return;
    }
    if (focused) focusCandidate(focusIndex);
    else filterRef.current?.focus({ preventScroll: true });
  });

  useLayoutEffect(() => {
    const layoutChanged = layoutWas.current !== layout;
    layoutWas.current = layout;
    if (layout !== "narrow" || !contentRef.current || !isDialogFocusAllowed(contentRef.current)) return;
    const active = document.activeElement;
    if (!(active instanceof HTMLElement)) return;
    const inList = !!listPaneRef.current?.contains(active);
    const inPreview = !!previewPaneRef.current?.contains(active);
    if (layoutChanged) {
      if (inPreview && contentPane !== "preview") setContentPane("preview");
      else if (inList && contentPane !== "list") setContentPane("list");
      return;
    }
    const hiddenPane = inList && listHidden ? listPaneRef.current
      : inPreview && previewHidden ? previewPaneRef.current : null;
    if (!hiddenPane) return;
    const visiblePane = listHidden ? previewPaneRef.current : listPaneRef.current;
    const next = (visiblePane && listFocusable(visiblePane)[0]) || visiblePane;
    next?.focus({ preventScroll: true });
  }, [contentPane, layout, listHidden, previewHidden]);

  useLayoutEffect(() => {
    const previous = paginationFocusRef.current;
    if (!previous || ui.busy || listHidden) return;
    const doc = previous.ownerDocument;
    const active = doc.activeElement;
    if (active && active !== doc.body && active !== doc.documentElement) return;
    paginationFocusRef.current = null;
    // Boundary pages remove the focused Next/Previous button. Busy transitions
    // may also blur a disabled button; recover only the focus we actually lost.
    const target = previous.isConnected && !previous.hasAttribute("disabled") ? previous
      : paginationRef.current?.querySelector<HTMLButtonElement>("button:not(:disabled)") ?? filterRef.current;
    if (target && isDialogFocusAllowed(target)) {
      target.focus({ preventScroll: true });
      target.scrollIntoView({ block: "nearest" });
    }
  });

  return (
    <div
      ref={contentRef}
      data-import-content="1"
      data-layout={layout}
      onFocusCapture={(e) => {
        candidateFocusRef.current = e.target.closest("[data-import-candidate]") ? e.target : null;
        paginationFocusRef.current = e.target.closest(".session-import__pagination") ? e.target : null;
      }}
      onBlurCapture={(e) => {
        if (e.target === candidateFocusRef.current && e.target.isConnected) {
          candidateFocusRef.current = null;
        }
        const doc = e.target.ownerDocument;
        const movedToControl = e.relatedTarget && e.relatedTarget !== doc.body && e.relatedTarget !== doc.documentElement;
        if (e.target === paginationFocusRef.current && e.target.isConnected &&
            (!e.target.hasAttribute("disabled") || movedToControl)) {
          paginationFocusRef.current = null;
        }
      }}
      onKeyDown={(e) => {
        if (e.defaultPrevented || ui.busy || e.nativeEvent.isComposing ||
            e.nativeEvent.keyCode === 229 || e.ctrlKey || e.altKey || e.metaKey ||
            !isDialogFocusAllowed(e.currentTarget)) return;
        const target = e.target instanceof Element ? e.target : null;
        const inPreview = !!target && !!previewPaneRef.current?.contains(target);
        // The preview owns its text and controls. A hidden list is not a
        // shortcut target, including when jsdom still accepts focus there.
        if (inPreview || listHidden) return;
        const onControl = activationOwningTarget(e.target);
        const inTextField = isTypingTarget(e.target);
        if (!onControl && !inTextField && candidates.length > 0) {
          const targetIndex = e.key === "ArrowDown" ? Math.min(focusIndex + 1, candidates.length - 1)
            : e.key === "ArrowUp" ? Math.max(focusIndex - 1, 0)
            : e.key === "Home" ? 0
            : e.key === "End" ? candidates.length - 1 : null;
          if (targetIndex !== null) {
            e.preventDefault();
            focusCandidate(targetIndex);
            return;
          }
        }
        const cmd = contentKeyCommand(e.key, {
          busy: ui.busy,
          composing: e.nativeEvent.isComposing,
          inTextField,
          onControl,
          hasNextPage: !!ui.nextCursor,
          canPreviousPage: ui.pageIndex > 0,
          hasFocusedCandidate: !!focused,
        });
        if (cmd === "none") return;
        e.preventDefault();
        if (cmd === "loadMore") loadMoreScan();
        if (cmd === "previousPage") previousScanPage();
        if (cmd === "toggleFocused" && focused) toggleCandidate(rowIdOf(focused));
        if (cmd === "peekFocused" && focused) showPreview(focused);
        if (cmd === "focusFilter") filterRef.current?.focus();
      }}
    >
      <div className="session-import__scan-summary">
      {ui.selectionView && ui.useIndependentCandidates && ui.useFixedGroups ? <Select
        value={ui.selectionView.domain} disabled={ui.busy} aria-label={tr("sessionImport.selectionMode")}
        options={[{ value: "fixed-groups", label: tr("sessionImport.groupedSources") },
          { value: "independent-candidates", label: tr("sessionImport.independentCandidates") }]}
        onChange={(domain) => { setMemberMenuId(null); void (domain === "fixed-groups" ? ui.useFixedGroups?.() : ui.useIndependentCandidates?.()); }} /> : null}
      {ui.scanIssues.length > 0 ? (
        <ul>
          {ui.scanIssues.map((iss, i) => (
            <li key={`${iss.path}-${i}`}>
              {tr(importErrorKey(`${iss.code}: ${iss.message}`))} — {iss.path}
            </li>
          ))}
        </ul>
      ) : null}
      <p role="status">
        {tr("sessionImport.scanCounts", {
          n: ui.discoveredCount,
          selected: ui.selectedCount,
          issues: ui.issueCount,
        })}
      </p>
      {visibleIds.length > 0 ? (
        <p role="status">
          {tr("sessionImport.visibleSelected", {
            n: visibleIds.filter((id) => (ui.selectedRows ?? ui.selected).has(id)).length,
            shown: visibleIds.length,
          })}
        </p>
      ) : null}
      <div
        className="session-import__progress"
        data-state={scanUiStatus({
          state: ui.scanState?.state,
          candidateCount: ui.candidates.length,
        })}
        role="progressbar"
        aria-valuetext={
          ui.scanCancelling
            ? tr("sessionImport.cancellingScan")
            : ui.scanning
              ? tr("sessionImport.scanning")
              : ui.scanPaused
                ? tr("sessionImport.scanPaused")
                : ui.scanFailed
                  ? tr("sessionImport.scanFailed")
                  : undefined
        }
      >
        <span />
      </div>
      {ui.scanCancelling ? <p role="status">{tr("sessionImport.cancellingScan")}</p> : null}
      {ui.scanning && !ui.scanCancelling ? <p role="status">{tr("sessionImport.scanning")}</p> : null}
      {ui.scanPaused ? <p role="status">{tr("sessionImport.scanPaused")}</p> : null}
      {ui.scanFailed ? <p role="alert">{tr("sessionImport.scanFailed")}</p> : null}
      {scanUiStatus({
        state: ui.scanState?.state,
        candidateCount: ui.candidates.length,
      }) === "empty" ? (
        <p>{tr("sessionImport.empty")}</p>
      ) : null}
      </div>
      {layout === "narrow" ? (
        <div className="session-import__content-switch" role="group" aria-label={tr("sessionImport.content.panes")}>
          <button type="button" className="btn btn--sm" aria-pressed={contentPane === "list"} onClick={() => setContentPane("list")}>
            {tr("sessionImport.content.showList")}
          </button>
          <button type="button" className="btn btn--sm" aria-pressed={contentPane === "preview"} onClick={() => setContentPane("preview")}>
            {tr("sessionImport.content.showPreview")}
          </button>
        </div>
      ) : null}
      <div className="session-import__workbench">
        <section
          ref={listPaneRef}
          className="session-import__candidates-pane"
          aria-label={tr("sessionImport.step.content")}
          hidden={listHidden}
          inert={listHidden ? true : undefined}
          tabIndex={-1}
        >
      <div className="session-import__toolbar session-import__bulk-actions">
        {ui.scanning || ui.scanPaused ? (
          <button type="button" className="btn btn--sm" onClick={() => void ui.cancelScan()}>
            {tr("sessionImport.cancelScan")}
          </button>
        ) : null}
        {ui.scanPaused ? (
          <button type="button" className="btn btn--sm" onClick={() => void ui.continueScan()}>
            {tr("sessionImport.continueScan")}
          </button>
        ) : null}
        <button type="button" className="btn btn--sm" disabled={ui.busy || ui.selectionReady === false} onClick={() => void ui.selectCurrent()}>
          {tr("sessionImport.selectAllCurrent")}
        </button>
        <button
          type="button"
          className="btn btn--sm"
          disabled={ui.busy || ui.selectionReady === false || ui.selectedCount === 0}
          onClick={() => void ui.clearSelected()}
        >
          {tr("sessionImport.clearSelection")}
        </button>
        <button
          type="button"
          className="btn btn--sm"
          disabled={ui.busy || ui.selectionReady === false || visibleIds.length === 0}
          onClick={() => void ui.selectThese(visibleIds)}
        >
          {tr("sessionImport.selectVisible")}
        </button>
        <button
          type="button"
          className="btn btn--sm"
          disabled={ui.busy || ui.selectionReady === false || visibleIds.length === 0}
          onClick={() => void ui.invertThese(visibleIds)}
        >
          {tr("sessionImport.invertVisible")}
        </button>
        <button type="button" className="btn btn--sm" disabled={ui.busy} onClick={() => void ui.refreshScan()}>
          {tr("sessionImport.refresh")}
        </button>
      </div>
          <div className="session-import__filters">
      <input
        ref={filterRef}
        type="search"
        className="input"
        placeholder={tr("sessionImport.filterSessions")}
        value={listFilter}
        onChange={(e) => setListFilter(e.target.value)}
        aria-label={tr("sessionImport.filterSessions")}
      />
      <label>
        <input
          type="checkbox"
          checked={onlyIncomplete}
          onChange={() => setOnlyIncomplete((v) => !v)}
        />
        {tr("sessionImport.onlyIncomplete")}
      </label>
      <label>
        <input type="checkbox" checked={onlyArchived} onChange={() => setOnlyArchived((v) => !v)} />
        {tr("sessionImport.onlyArchived")}
      </label>
          </div>
          <div className="session-import__candidate-scroll">
      {(listFilter.trim() || onlyIncomplete || onlyArchived) && filteredCandidates.length === 0 && filteredExtras.length === 0 ? (
        <p role="status">{tr("sessionImport.filterEmpty")}</p>
      ) : null}
      <ul className="session-import__list">
        {filteredCandidates.map((c) => (
          <SessionImportCandidateRow
            key={rowIdOf(c)}
            rowId={rowIdOf(c)}
            candidate={c}
            selected={ui.selected.has(c.candidateId)}
            focused={rowIdOf(c) === (focused && rowIdOf(focused))}
            peekBusy={!!contentStates.byCandidate[c.candidateId]?.busy}
            disabled={ui.busy || ui.selectionReady === false}
            versionPicker={(() => {
              const group = ui.selectionRows?.find((row) => row.candidate.candidateId === c.candidateId)?.group;
              return group && ui.selectionView?.domain === "fixed-groups" && ui.chooseGroupMember ?
                <><SessionImportGroupMemberPicker scope={ui.selectionView.scope} group={group} candidateId={c.candidateId}
                  open={memberMenuId === group.groupId} disabled={ui.busy || ui.selectionReady === false} busy={ui.busy}
                  tr={tr} onOpen={() => setMemberMenuId(group.groupId)} onClose={() => setMemberMenuId(null)}
                  onChoose={ui.chooseGroupMember} onRefresh={ui.refreshScan} />
                  {group.memberCount > 1 && props.onCompare ? <button type="button" className="btn btn--sm btn--ghost"
                    disabled={ui.busy || ui.selectionReady === false} aria-haspopup="dialog"
                    onKeyDown={(event) => {
                      if ((event.nativeEvent.isComposing || event.nativeEvent.keyCode === 229) &&
                        (event.key === "Enter" || event.key === " ")) event.preventDefault();
                    }}
                    onClick={(event) => {
                      event.currentTarget.focus(); setMemberMenuId(null);
                      if (ui.selectionView?.domain === "fixed-groups") {
                        props.onCompare?.(copyComparisonContext({ scope: ui.selectionView.scope, group, primary: c }));
                      }
                    }}>{tr("sessionImport.comparison.title")}</button> : null}
                </> : null;
            })()}
            tr={tr}
            onFocus={() => setFocusedId(rowIdOf(c))}
            onToggle={() => toggleCandidate(rowIdOf(c))}
            onPeek={() => showPreview(c)}
          />
        ))}
      </ul>
      {extraCandidates.length > 0 ? (
        <>
          <p className="settings-row__desc">{tr("sessionImport.newDiscoveries")}</p>
          <ul className="session-import__list">
            {filteredExtras.map((c) => (
              <SessionImportCandidateRow
                key={rowIdOf(c)}
                rowId={rowIdOf(c)}
                candidate={c}
                selected={ui.selected.has(c.candidateId)}
                focused={rowIdOf(c) === (focused && rowIdOf(focused))}
                peekBusy={!!contentStates.byCandidate[c.candidateId]?.busy}
                disabled={ui.busy || ui.selectionReady === false}
                tr={tr}
                onFocus={() => setFocusedId(rowIdOf(c))}
                onToggle={() => toggleCandidate(rowIdOf(c))}
                onPeek={() => showPreview(c)}
              />
            ))}
          </ul>
        </>
      ) : ui.extraCount > 0 ? (
        <p className="settings-row__desc">{tr("sessionImport.newDiscoveries")}</p>
      ) : null}
          </div>
          <div ref={paginationRef} className="session-import__pagination">
      <p role="status">{tr("sessionImport.page", { n: ui.pageIndex + 1 })}</p>
      <div className="session-import__toolbar session-import__page-actions">
        {ui.pageIndex > 0 ? (
          <button type="button" className="btn" disabled={ui.busy} onClick={previousScanPage}>
            {tr("sessionImport.previousPage")}
          </button>
        ) : null}
        {ui.nextCursor ? (
          <button
            type="button"
            className="btn"
            data-import-load-more="1"
            disabled={ui.busy}
            onClick={loadMoreScan}
          >
            {tr("sessionImport.nextPage")}
          </button>
        ) : null}
            </div>
          </div>
        </section>
        <section
          ref={previewPaneRef}
          className="session-import__preview-pane"
          aria-label={tr("sessionImport.preview")}
          hidden={previewHidden}
          inert={previewHidden ? true : undefined}
          tabIndex={-1}
        >
          <h3 className="session-import__pane-title">{tr("sessionImport.preview")}</h3>
          {!peekSlot ? (
            <p className="session-import__empty-preview">{tr("sessionImport.previewHint")}</p>
          ) : null}
      {peekSlot?.busy ? <p role="status">{tr("sessionImport.peekLoading")}</p> : null}
      {peekSlot?.title ? (
        <div className="session-import__peek">
          <p>
            {tr("sessionImport.preview")}: {peekSlot.title}
          </p>
          {peekSlot.summary ? (
            <p className="settings-row__desc">
              {peekSlot.summary.outcome ? `${outcomeLabel(peekSlot.summary.outcome, tr)} · ` : ""}
              {tr("sessionImport.eventCount", { n: peekSlot.summary.eventCount })}
              {" · "}
              {tr("sessionImport.inventorySummary", {
                messages: peekSlot.summary.messages,
                tools: peekSlot.summary.tools,
                attachments: peekSlot.summary.attachments,
              })}
              {peekSlot.summary.missing > 0
                ? ` · ${tr("sessionImport.missingAsset", { id: peekSlot.summary.missing })}`
                : ""}
              {peekSlot.selection.leaf
                ? ` · ${tr("sessionImport.branch")}: ${peekSlot.selection.leaf}`
                : ""}
            </p>
          ) : null}
          {peekSlot.summary?.issues.length ? (
            <ul>
              {peekSlot.summary.issues.map((iss, i) => (
                <li key={`${iss.code}-${i}`}>
                  {tr("sessionImport.issues")}: {importIssueKey(iss.code) ? tr(importIssueKey(iss.code)!) : iss.message || iss.code}
                </li>
              ))}
            </ul>
          ) : null}
          {peekSlot.error ? (
            <div role="alert">
              <p>{tr(importErrorKey(peekSlot.error))}</p>
              <p>{tr("error.details")}</p>
              <pre>{peekSlot.error}</pre>
              <button
                type="button"
                className="btn btn--sm"
                data-import-error-copy="1"
                onClick={() => void copyVisibleImportError(peekSlot.error)}
              >
                {tr("sessionImport.copyText")}
              </button>
            </div>
          ) : null}
          <SessionImportContentDetail
            tr={tr}
            state={peekSlot.selection}
            busy={peekSlot.busy || ui.busy}
            empty={!peekSlot.error && peekSlot.selection.events.length === 0}
            onToggleEvent={(id) =>
              contentStates.dispatchSelection(peekSlot.candidateId, {
                type: "toggleEvent",
                eventId: id,
              })
            }
            onSelectPage={() =>
              contentStates.dispatchSelection(peekSlot.candidateId, { type: "selectPage" })
            }
            onInvertPage={() =>
              contentStates.dispatchSelection(peekSlot.candidateId, { type: "invertPage" })
            }
            onField={(field, value) =>
              contentStates.dispatchSelection(peekSlot.candidateId, {
                type: "setField",
                field,
                value,
              })
            }
            onSubagents={(value) =>
              contentStates.dispatchSelection(peekSlot.candidateId, {
                type: "setSubagents",
                value,
              })
            }
            onAsset={(id) =>
              contentStates.dispatchSelection(peekSlot.candidateId, {
                type: "toggleAsset",
                assetId: id,
              })
            }
            onRequestLeaf={(leaf) =>
              contentStates.dispatchSelection(peekSlot.candidateId, {
                type: "requestLeaf",
                leaf,
              })
            }
            onCancelLeaf={() =>
              contentStates.dispatchSelection(peekSlot.candidateId, { type: "cancelLeaf" })
            }
            onConfirmLeaf={() => {
              void contentStates.confirmLeaf(peekSlot.candidateId);
            }}
            onLoadMore={() => {
              void contentStates.loadMore(peekSlot.candidateId);
            }}
          />
        </div>
      ) : null}
        </section>
      </div>
    </div>
  );
}

/** Keep navigation outside the scrolling body, including short/narrow windows. */
export function SessionImportContentActions({ tr, ui, contentStates, goStep }: {
  tr: SessionImportTFn;
  ui: Ui;
  contentStates: ContentStates;
  goStep: (step: ImportStep) => void;
}) {
  return (
    <div className="session-import__toolbar session-import__footer">
      <button
        type="button"
        className="btn"
        data-import-back="content"
        onClick={() => goStep("source")}
      >
        {tr("sessionImport.back")}
      </button>
      <button
        type="button"
        className="btn btn--primary"
        disabled={ui.selectedCount === 0 || ui.busy || ui.selectionReady === false}
        onClick={() => {
          const slots = Object.values(contentStates.byCandidate)
            .filter((slot) => slot.previewId && slot.selection.leaf)
            .map((slot) => ({
              candidateId: slot.candidateId,
              previewId: slot.previewId as string,
              leafId: slot.selection.leaf as string,
              selection: allowedSelectionOf(slot),
            }));
          void ui.previewSelected(slots);
        }}
      >
        {tr("sessionImport.next")}
      </button>
    </div>
  );
}
