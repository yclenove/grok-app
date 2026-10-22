import { useId, useLayoutEffect, useRef, useState } from "react";
import type { MessageKey, Vars } from "@/i18n";
import { Select } from "@/components/Select";
import { GlassModal } from "@/components/GlassModal";
import { ImportedEventBody } from "./ImportedEventBody";
import { useImportedImages } from "@/hooks/useImportedImages";
import { isDialogFocusAllowed, listFocusable } from "@/lib/a11yFocus";
import type { ContentSelectionState, PreviewEventRow } from "@/lib/sessionImport/contentSelection";
import type { SelectionFields } from "@/lib/sessionImport/selection";

type TFn = (key: MessageKey, vars?: Vars) => string;

type PageFocusClaim = {
  previewId: string | null;
  generation: number;
  leaf: string | null;
  events: PreviewEventRow[];
  cursor: string | null;
  button: HTMLButtonElement;
  sawBusy: boolean;
};

function isVisibleDetail(root: HTMLElement): boolean {
  for (let node: HTMLElement | null = root; node; node = node.parentElement) {
    const style = node.ownerDocument.defaultView?.getComputedStyle(node);
    if (node.hidden || node.hasAttribute("inert") || node.getAttribute("aria-hidden") === "true" ||
        style?.display === "none" || style?.visibility === "hidden") return false;
  }
  return root.isConnected;
}

export function SessionImportContentDetail(props: {
  tr: TFn;
  state: ContentSelectionState;
  busy: boolean;
  empty: boolean;
  onToggleEvent: (id: string) => void;
  onSelectPage: () => void;
  onInvertPage: () => void;
  onField: (field: keyof SelectionFields, value: boolean) => void;
  onSubagents: (value: boolean) => void;
  onAsset: (id: string) => void;
  onRequestLeaf: (leaf: string) => void;
  onConfirmLeaf: () => void;
  onCancelLeaf: () => void;
  onLoadMore: () => void;
}) {
  const { tr, state, busy } = props;
  const images = useImportedImages(state.previewId ? { kind: "preview", previewId: state.previewId } : null,
    state.events, JSON.stringify([state.previewId, state.leaf, state.generation]));
  const [optionsOpen, setOptionsOpen] = useState(false);
  const optionsId = useId();
  const detailRef = useRef<HTMLDivElement>(null);
  const pageFocusRef = useRef<PageFocusClaim | null>(null);

  useLayoutEffect(() => {
    const doc = detailRef.current?.ownerDocument;
    if (!doc) return;
    // Once the user moves elsewhere, a later lost focus must not revive this claim.
    const releaseOnMove = (event: Event) => {
      if (pageFocusRef.current && event.target !== pageFocusRef.current.button &&
          (event.type === "pointerdown" ||
            (event.target !== doc.body && event.target !== doc.documentElement))) {
        pageFocusRef.current = null;
      }
    };
    doc.addEventListener("focusin", releaseOnMove, true);
    doc.addEventListener("pointerdown", releaseOnMove, true);
    return () => {
      doc.removeEventListener("focusin", releaseOnMove, true);
      doc.removeEventListener("pointerdown", releaseOnMove, true);
    };
  }, []);

  useLayoutEffect(() => {
    const claim = pageFocusRef.current;
    const root = detailRef.current;
    if (!claim || !root) return;
    if (claim.previewId !== state.previewId || claim.generation !== state.generation ||
        claim.leaf !== state.leaf || state.pendingLeaf || !isVisibleDetail(root) ||
        !isDialogFocusAllowed(root)) {
      pageFocusRef.current = null;
      return;
    }
    if (busy) { claim.sawBusy = true; return; }
    if (!claim.sawBusy && claim.events === state.events && claim.cursor === state.nextCursor) return;
    pageFocusRef.current = null;
    const doc = root.ownerDocument;
    const active = doc.activeElement;
    if (active && active !== doc.body && active !== doc.documentElement) return;
    const oldIds = new Set(claim.events.map((event) => event.eventId));
    const rows = root.querySelectorAll<HTMLLIElement>(".session-import__events > li");
    const addedIndex = state.events.findIndex((event) => !oldIds.has(event.eventId));
    const row = rows[addedIndex < 0 ? rows.length - 1 : addedIndex];
    const target = claim.button.isConnected && !claim.button.disabled ? claim.button
      : (row && (listFocusable(row)[0] ?? row)) || listFocusable(root)[0];
    target?.focus({ preventScroll: true });
    const scrollTarget = target === claim.button ? target : row ?? target;
    scrollTarget?.scrollIntoView?.({ block: "nearest" });
  });

  const leafOptions = state.leaves.map((leaf) => ({ value: leaf, label: leaf }));
  const fieldToggle = (
    field: keyof SelectionFields,
    key: MessageKey,
  ) => (
    <label key={field}>
      <input
        type="checkbox"
        checked={state.fields[field]}
        disabled={busy}
        onChange={(e) => props.onField(field, e.target.checked)}
      />
      {tr(key)}
    </label>
  );
  return (
    <div ref={detailRef} className="session-import__detail" data-busy={busy ? "1" : "0"}>
      <p className="settings-row__desc">{tr("sessionImport.sessionDetail")}</p>
      <p className="settings-row__desc">{tr("sessionImport.defaultScope")}</p>
      {leafOptions.length > 0 ? (
        <Select
          value={state.leaf ?? leafOptions[0]?.value ?? ""}
          options={leafOptions}
          onChange={(value) => props.onRequestLeaf(value)}
          aria-label={tr("sessionImport.selectLeaf")}
          disabled={busy}
        />
      ) : null}
      <p role="status">
        {tr("sessionImport.archiveCount", { n: state.archiveCount })}
        {" · "}
        {tr("sessionImport.modelCount", { n: state.modelCount })}
      </p>
      <div className="session-import__toolbar">
        <button type="button" className="btn btn--sm btn--ghost" disabled={busy} onClick={props.onSelectPage}>
          {tr("sessionImport.selectPage")}
        </button>
        <button type="button" className="btn btn--sm btn--ghost" disabled={busy} onClick={props.onInvertPage}>
          {tr("sessionImport.invertPage")}
        </button>
      </div>
      <section className="session-import__content-options">
        <button type="button" className="session-import__options-toggle"
          aria-expanded={optionsOpen} aria-controls={optionsId}
          onClick={() => setOptionsOpen((value) => !value)}>
          <span aria-hidden="true">{optionsOpen ? "−" : "+"}</span>
          {tr("sessionImport.contentOptions")}
        </button>
        {optionsOpen ? <div className="session-import__fields" id={optionsId}>
          {fieldToggle("visibleReasoning", "sessionImport.includeReasoning")}
          <label>
            <input
              type="checkbox"
              checked={state.includeSubagents}
              disabled={busy}
              onChange={(e) => props.onSubagents(e.target.checked)}
            />
            {tr("sessionImport.includeSubagents")}
          </label>
          {fieldToggle("toolCallName", "sessionImport.fieldToolName")}
          {fieldToggle("toolCallInput", "sessionImport.fieldToolInput")}
          {fieldToggle("toolResultContent", "sessionImport.fieldToolResult")}
          {fieldToggle("compactionSummary", "sessionImport.fieldCompaction")}
        </div> : null}
      </section>
      {props.empty && !busy ? <p>{tr("sessionImport.peekEmpty")}</p> : null}
      <ol className="session-import__events">
        {state.events.map((ev: PreviewEventRow) => (
          <li key={ev.eventId} tabIndex={-1} data-kind={ev.kind} data-role={ev.role}>
            <label className="session-import__event-select">
              <input
                type="checkbox"
                aria-label={ev.role === "user"
                  ? tr("sessionImport.roleUser")
                  : ev.role === "assistant"
                    ? tr("sessionImport.roleAssistant")
                    : ev.role ?? ev.kind}
                checked={state.selectedEventIds.includes(ev.eventId)}
                disabled={busy || ev.selectable === false}
                onChange={() => props.onToggleEvent(ev.eventId)}
              />
              {ev.role !== "user" && ev.role !== "assistant" ? (
                <strong>{ev.role ?? ev.kind}</strong>
              ) : null}
              {ev.subagent ? ` · ${tr("sessionImport.includeSubagents")}` : ""}
            </label>
            <ImportedEventBody key={JSON.stringify([state.previewId, state.leaf, state.generation, ev.eventId])}
              event={ev} tr={tr} images={images} />
            {(ev.assetIds ?? []).map((id, index) => (
              <label key={id}>
                <input
                  type="checkbox"
                  checked={state.assetIds.includes(id)}
                  disabled={busy}
                  onChange={() => props.onAsset(id)}
                />
                {tr("sessionImport.attachmentsSelect")} {index + 1}
              </label>
            ))}
          </li>
        ))}
      </ol>
      {state.nextCursor ? (
        <button type="button" className="btn btn--sm btn--ghost" disabled={busy} onClick={(event) => {
          const button = event.currentTarget;
          pageFocusRef.current = button.ownerDocument.activeElement === button ? {
            previewId: state.previewId, generation: state.generation, leaf: state.leaf,
            events: state.events, cursor: state.nextCursor, button, sawBusy: false,
          } : null;
          props.onLoadMore();
        }}>
          {tr("sessionImport.loadMore")}
        </button>
      ) : null}
      <GlassModal
        open={!!state.pendingLeaf}
        onClose={props.onCancelLeaf}
        title={tr("sessionImport.confirmSwitchLeaf")}
        size="sm"
        closeLabel={tr("sessionImport.keepLeaf")}
      >
        <p>{tr("sessionImport.confirmSwitchLeafBody")}</p>
        <div className="session-import__toolbar">
          <button type="button" className="btn" onClick={props.onCancelLeaf}>
            {tr("sessionImport.keepLeaf")}
          </button>
          <button type="button" className="btn" onClick={props.onConfirmLeaf} disabled={busy}>
            {tr("sessionImport.applySwitch")}
          </button>
        </div>
      </GlassModal>
    </div>
  );
}
