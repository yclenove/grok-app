import { useEffect, useMemo, useRef, useState } from "react";
import { createT, type Locale } from "@/i18n";
import { GlassModal } from "@/components/GlassModal";
import {
  sessionImportContinuationGet, sessionImportContinuationSave,
  type ContinuationDraftView,
} from "@/lib/api/sessionMigration";
import { importErrorKey } from "@/lib/sessionImport/errorCopy";
import { openImportedHistory } from "@/lib/sessionImport/open";

export type ContinuationDraftClient = {
  get: (appSessionId: string) => Promise<ContinuationDraftView | null>;
  save: (appSessionId: string, expectedTextHash: string, text: string) => Promise<ContinuationDraftView>;
};

type Props = {
  sessionId: string;
  locale: Locale;
  empty: boolean;
  sessionState: string;
  /** Defaults to the live continuation API. Gallery and tests pass a fail-closed client. */
  client?: ContinuationDraftClient;
  /** Open the review editor after the first successful read. Production leaves this unset. */
  startOpen?: boolean;
  /** Theme for the portaled review editor. Production inherits the document theme. */
  dataTheme?: string;
  onOpenHistory?: typeof openImportedHistory;
};
type Editor = { base: ContinuationDraftView; text: string };

/** Durable handoff context stays separate from the new instruction composer.
 * Never expands, sends, or copies the context into that composer (the Host
 * attaches it once). Keying the inner view isolates session-switch races. */
export function ContinuationDraftBanner(props: Props) {
  return <DraftBanner key={props.sessionId} {...props} />;
}

function DraftBanner({ sessionId, locale, empty, sessionState, client, startOpen = false, dataTheme,
  onOpenHistory = openImportedHistory }: Props) {
  const get = client?.get ?? sessionImportContinuationGet;
  const save = client?.save ?? sessionImportContinuationSave;
  const tr = useMemo(() => createT(locale), [locale]);
  const [view, setView] = useState<ContinuationDraftView | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [refresh, setRefresh] = useState(0);
  const [open, setOpen] = useState(false);
  const [editor, setEditor] = useState<Editor | null>(null);
  const [editError, setEditError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const generation = useRef(0);
  const working = useRef(false);
  const active = sessionState === "streaming" || sessionState === "connecting";
  const editable = !!view?.textHash && view.text !== null;
  useEffect(() => {
    const token = ++generation.current;
    setLoading(true);
    setError(null);
    void get(sessionId).then((next) => {
      if (token !== generation.current) return;
      setView(next);
      if (startOpen && next?.textHash && next.text !== null) {
        setEditor({ base: next, text: next.text });
        setOpen(true);
      }
    }).catch((cause) => {
      if (token === generation.current) setError(String(cause));
    }).finally(() => {
      if (token === generation.current) setLoading(false);
    });
    return () => { ++generation.current; };
  }, [sessionId, sessionState, refresh, get, startOpen]);

  async function updateEditor(reload: boolean) {
    if (!editor || working.current || active || (!reload && !editor.text.trim())) return;
    working.current = true;
    setBusy(true);
    setEditError(null);
    // Invalidate any older read; a save response is newer than that read.
    const token = ++generation.current;
    try {
      const next = reload
        ? await get(sessionId)
        : await save(sessionId, editor.base.textHash!, editor.text);
      if (token !== generation.current) return;
      setView(next);
      setError(null);
      setEditor(next?.textHash && next.text !== null ? { base: next, text: next.text } : null);
      if (!reload) { setOpen(false); setEditor(null); }
    } catch (cause) {
      if (token === generation.current) setEditError(String(cause));
    } finally {
      working.current = false;
      setBusy(false);
      // The edit operation can finish after a session-state refresh started.
      // Only its current owner may clear that read's loading gate.
      if (token === generation.current) setLoading(false);
    }
  }

  // Native chats without a continuation retain the original empty-chat copy.
  if (!view && !error) {
    if (!empty) return null;
    return <div className="conn-bar" role="status"><span className="session-import__continuation-hint">
      {tr(loading ? "sessionImport.continuationLoading" : "automations.emptySession")}
    </span></div>;
  }
  return <>
    <section className="conn-bar session-import__continuation-banner" aria-label={tr("sessionImport.handoffTitle")}>
      <p className="session-import__continuation-hint" role={error ? "alert" : "status"}>
        {error ? tr(importErrorKey(error)) : tr(editable
          ? "sessionImport.continuationSaved" : "sessionImport.continuationLocked")}
      </p>
      {error ? <button type="button" className="btn btn--ghost" disabled={loading || busy}
        onClick={() => setRefresh((value) => value + 1)}>{tr("sessionImport.refresh")}</button> : null}
      {editable ? <button type="button" className="btn btn--primary" disabled={loading || busy || active || !!error}
        onClick={() => {
          if (!editor) { setEditor({ base: view!, text: view!.text! }); setEditError(null); }
          setOpen(true);
        }}>{tr("sessionImport.continuationReview")}</button> : null}
      {view ? <button type="button" className="btn btn--ghost" disabled={busy} onClick={() => {
        onOpenHistory({ importId: view.importId, revision: view.revisionId, leaf: view.leaf });
      }}>{tr("sessionImport.continuationHistory")}</button> : null}
    </section>
    <GlassModal open={open} onClose={() => { if (!working.current) setOpen(false); }}
      title={tr("sessionImport.handoffTitle")} size="lg" wrapBody
      className="session-import-modal session-import-continuation" closeLabel={tr("sessionImport.close")}
      dataTheme={dataTheme}
      closeOnOverlay={false} showClose={!busy} footer={<>
        <button type="button" className="btn btn--ghost" disabled={busy} onClick={() => {
          setOpen(false); setEditor(null); setEditError(null);
        }}>{tr("common.cancel")}</button>
        <button type="button" className="btn btn--primary" disabled={busy || loading || active || !editable || !editor?.text.trim()}
          onClick={() => { void updateEditor(false); }}>{tr("common.save")}</button>
      </>}>
      <p className="session-import__read-only">{tr(editable
        ? "sessionImport.continuationSaved" : "sessionImport.continuationLocked")}</p>
      {editError ? <p role="alert">{tr(importErrorKey(editError))}</p> : null}
      {editor ? <textarea className="session-import__draft" rows={12}
        aria-label={tr("sessionImport.continuationReview")} value={editor.text}
        disabled={busy || active || !editable}
        onChange={(event) => setEditor({ ...editor, text: event.target.value })} /> : null}
      <button type="button" className="btn btn--ghost" disabled={busy || active || !editor}
        onClick={() => { void updateEditor(true); }}>{tr("sessionImport.continuationReload")}</button>
    </GlassModal>
  </>;
}
