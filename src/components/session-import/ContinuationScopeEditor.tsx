import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { MessageKey, Vars } from "@/i18n";
import { sessionImportEventContext, type HandoffDraft } from "@/lib/api/sessionMigration";
import type { AllowedSelection, SelectionFields } from "@/lib/sessionImport/selection";
import { importedEventDisplay, importedEventId } from "@/lib/sessionImport/eventView";
import { acceptReaderScope } from "@/lib/sessionImport/readerScope";
import { importErrorKey } from "@/lib/sessionImport/errorCopy";

type Tr = (key: MessageKey, vars?: Vars) => string;
const fields: Array<[keyof SelectionFields, MessageKey]> = [
  ["messageText", "sessionImport.scopeMessages"], ["toolCallName", "sessionImport.fieldToolName"],
  ["toolCallInput", "sessionImport.fieldToolInput"], ["toolResultContent", "sessionImport.fieldToolResult"],
  ["compactionSummary", "sessionImport.fieldCompaction"], ["visibleReasoning", "sessionImport.includeReasoning"],
];

function ScopeIds({ title, ids, selected, disabled, tr, onToggle, onPreview }: {
  title: string; ids: string[]; selected: string[]; disabled: boolean; tr: Tr;
  onToggle: (id: string) => void; onPreview?: (id: string) => void;
}) {
  const [query, setQuery] = useState("");
  const [page, setPage] = useState(0);
  const matches = ids.filter((id) => id.toLowerCase().includes(query.toLowerCase()));
  const start = Math.min(page, Math.max(0, Math.ceil(matches.length / 40) - 1)) * 40;
  const chosen = new Set(selected);
  return <section aria-label={title}>
    <h4 className="session-import__pane-title">{title}</h4>
    <input className="input" aria-label={`${title}: ${tr("sessionImport.scopeFilter")}`}
      value={query} onChange={(event) => { setQuery(event.target.value); setPage(0); }} />
    <ul className="session-import__events">{matches.slice(start, start + 40).map((id) => <li key={id}>
      <label className="session-import__event-select" style={{ overflowWrap: "anywhere" }}>
        <input type="checkbox" checked={chosen.has(id)} disabled={disabled} onChange={() => onToggle(id)} />
        <code>{id}</code>
      </label>
      {onPreview ? <button type="button" className="btn btn--ghost" onClick={() => onPreview(id)}>
        {tr("sessionImport.preview")}
      </button> : null}
    </li>)}</ul>
    <div className="session-import__pager">
      <button type="button" className="btn btn--ghost" disabled={start === 0}
        onClick={() => setPage((value) => Math.max(0, value - 1))}>{tr("sessionImport.previousPage")}</button>
      <button type="button" className="btn btn--ghost" disabled={start + 40 >= matches.length}
        onClick={() => setPage((value) => value + 1)}>{tr("sessionImport.loadMore")}</button>
    </div>
  </section>;
}

export function ContinuationScopeEditor({ draft, tr, busy, pending, onApply, onCancel, loadContext = sessionImportEventContext }: {
  draft: HandoffDraft; tr: Tr; busy: boolean; pending: boolean;
  onApply: (selection: AllowedSelection) => Promise<boolean>; onCancel: () => void;
  loadContext?: typeof sessionImportEventContext;
}) {
  const heading = useRef<HTMLHeadingElement>(null);
  useLayoutEffect(() => { heading.current?.focus(); }, []);
  const ceiling = draft.selection!;
  const [selection, setSelection] = useState<AllowedSelection>(() => structuredClone(ceiling));
  const [preview, setPreview] = useState<{ id: string; text: string } | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [previewBusy, setPreviewBusy] = useState(false);
  const request = useRef(0);
  useEffect(() => () => { request.current += 1; }, []);
  const disabled = busy || pending;
  const toggleId = (key: "eventIds" | "pinnedEventIds" | "assetIds", id: string) => {
    if (disabled) return;
    setSelection((before) => {
      const ids = before[key].includes(id) ? before[key].filter((value) => value !== id) : [...before[key], id];
      if (key === "pinnedEventIds" && !before.eventIds.includes(id)) return before;
      return { ...before, [key]: ids, ...(key === "eventIds"
        ? { pinnedEventIds: before.pinnedEventIds.filter((value) => ids.includes(value)) } : {}) };
    });
  };
  const showPreview = async (id: string) => {
    const token = ++request.current; setPreviewBusy(true); setPreviewError(null); setPreview(null);
    try {
      const context = await loadContext(draft.importId, id, draft.revisionId, draft.leaf, { before: 0, after: 0 });
      if (token !== request.current) return;
      acceptReaderScope({ revision: draft.revisionId, leaf: draft.leaf }, null, context);
      const event = context.events.find((value) => importedEventId(value) === id);
      if (!event) throw "PLAN_STALE: selected preview event is missing";
      const display = importedEventDisplay(event);
      setPreview({ id, text: display.parts.filter((part) => part.kind === "text" || part.kind === "system")
        .map((part) => "text" in part ? part.text : "").join("\n").slice(0, 4000) });
    } catch (cause) { if (token === request.current) setPreviewError(String(cause)); }
    finally { if (token === request.current) setPreviewBusy(false); }
  };
  return <section className="session-import__content-options" aria-label={tr("sessionImport.scopeReview")}>
    <h3 ref={heading} tabIndex={-1} className="session-import__pane-title">{tr("sessionImport.scopeReview")}</h3>
    <p className="settings-row__desc">{tr("sessionImport.scopeHint")}</p>
    <p>{tr("sessionImport.modelCount", { n: selection.eventIds.length })}</p>
    <div className="session-import__fields">{fields.map(([key, label]) => <label key={key}>
      <input type="checkbox" checked={selection.fields[key]} disabled={disabled || !ceiling.fields[key]}
        onChange={(event) => setSelection((before) => ({ ...before, fields: { ...before.fields, [key]: event.target.checked } }))} />
      <span style={{ minWidth: 0, overflowWrap: "anywhere" }}>{tr(label)}</span>
    </label>)}
      <label><input type="checkbox" checked={selection.includeSubagents} disabled={disabled || !ceiling.includeSubagents}
        onChange={(event) => setSelection((before) => ({ ...before, includeSubagents: event.target.checked }))} />
        {tr("sessionImport.includeSubagents")}</label>
    </div>
    <ScopeIds title={tr("sessionImport.scopeEvents")} ids={ceiling.eventIds} selected={selection.eventIds}
      disabled={disabled} tr={tr} onToggle={(id) => toggleId("eventIds", id)} onPreview={(id) => { void showPreview(id); }} />
    {ceiling.pinnedEventIds.length ? <ScopeIds title={tr("sessionImport.scopePinned")} ids={ceiling.pinnedEventIds}
      selected={selection.pinnedEventIds} disabled={disabled} tr={tr} onToggle={(id) => toggleId("pinnedEventIds", id)} /> : null}
    {ceiling.assetIds.length ? <ScopeIds title={tr("sessionImport.attachmentsSelect")} ids={ceiling.assetIds}
      selected={selection.assetIds} disabled={disabled} tr={tr} onToggle={(id) => toggleId("assetIds", id)} /> : null}
    {previewBusy ? <p role="status">{tr("sessionImport.continuationLoading")}</p> : null}
    {previewError ? <p role="alert">{tr(importErrorKey(previewError))}</p> : null}
    {preview ? <div><code>{preview.id}</code><pre className="session-import__user-body"
      style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{preview.text}</pre></div> : null}
    {pending ? <p role="status">{tr("sessionImport.scopePending")}</p> : null}
    <div className="session-import__pager">
      <button type="button" className="btn btn--ghost" disabled={disabled} onClick={onCancel}>{tr("sessionImport.scopeCancel")}</button>
      <button type="button" className="btn btn--primary" aria-disabled={busy}
        onClick={() => { if (!busy) void onApply(selection); }}>{tr("sessionImport.scopeApply")}</button>
    </div>
  </section>;
}
