import { useEffect, useMemo } from "react";
import { createT, type Locale } from "@/i18n";
import { useImportContinuation } from "@/hooks/useImportContinuation";
import type { CatalogEntry, ContinuationDraftView } from "@/lib/api/sessionMigration";
import { importErrorKey } from "@/lib/sessionImport/errorCopy";
import { SessionImportContinuationModal } from "./SessionImportContinuationModal";
import { createReviewContinuationClient } from "./sessionImportReviewContinuationClient";
import { catalogEntry, draft } from "./sessionImportReviewFixtures";

const activity = () => {}; // Review activity has no application session to update.

export function SessionImportReviewContinuation({ locale, theme, locked, failCreateOnce, onCreated, entry = catalogEntry }: {
  locale: Locale; theme: string; locked: boolean; failCreateOnce: boolean;
  onCreated: (view: ContinuationDraftView) => void;
  entry?: CatalogEntry;
}) {
  const client = useMemo(() => createReviewContinuationClient({ locked, failCreateOnce, entry }), [entry, locked, failCreateOnce]);
  const ui = useImportContinuation(null, activity, client.continuation);
  const tr = createT(locale);
  const { prepareContinue, setDraft, setContinuationError } = ui;
  useEffect(() => {
    let cancelled = false;
    // Fixture setup is mount-owned. StrictMode's probe must not freeze a
    // controller request that is normally started by a user action.
    void client.continuation.sessionImportContinuePrepare(entry.importId, {
      leaf: draft.leaf, revision: entry.currentRevision ?? draft.revisionId,
    }).then((next) => { if (!cancelled) setDraft(next); })
      .catch((cause) => { if (!cancelled) setContinuationError(String(cause)); });
    return () => { cancelled = true; };
  }, [client, entry, setContinuationError, setDraft]);
  return <>
    <SessionImportContinuationModal locale={locale} dataTheme={theme} className="session-import-review-frame"
      ui={{ ...ui, busy: ui.continuationBusy, projects: [{ id: "project-synthetic", name: "Synthetic project" },
        { id: "project-other", name: "Synthetic other project" }] }}
      client={client.modal} onOpenNativeSession={() => {
        const created = client.getCreated();
        if (created) onCreated(created);
      }} />
    {!ui.continuationVisible ? <div data-review-surface="continuation">
      {ui.continuationBusy ? <p role="status">{tr("sessionImport.continuationLoading")}</p> : <>
        {ui.continuationError ? <p role="alert">{tr(importErrorKey(ui.continuationError))}</p> : null}
        <button type="button" className="btn" onClick={() => void prepareContinue(entry.importId, draft.leaf, entry.currentRevision ?? draft.revisionId)}>
          {tr("sessionImport.handoffTitle")}
        </button>
      </>}
    </div> : null}
  </>;
}
