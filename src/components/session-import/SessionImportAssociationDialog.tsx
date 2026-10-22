import { useCallback, useEffect, useRef, useState } from "react";
import { GlassModal } from "@/components/GlassModal";
import { Select } from "@/components/Select";
import { useImportedReader } from "@/hooks/useImportedReader";
import { useImportedImages } from "@/hooks/useImportedImages";
import type { useSessionImportAssociations } from "@/hooks/useSessionImportAssociations";
import { sessionImportPreviewEvents } from "@/lib/api/sessionMigration";
import type { PreparedAssociationReview } from "@/lib/api/sessionMigrationIdentityAssociation";
import { importErrorKey } from "@/lib/sessionImport/errorCopy";
import { ImportedEventList } from "./ImportedEventList";
import type { SessionImportTFn } from "./sessionImportHostLabels";

type AssociationUi = ReturnType<typeof useSessionImportAssociations>;
const ignoreReaderError = () => {};

function AssociationContent({ inspected, tr, markReady }: {
  inspected: PreparedAssociationReview; tr: SessionImportTFn; markReady: AssociationUi["markReady"];
}) {
  const { review } = inspected;
  const reader = useImportedReader(review.importId, ignoreReaderError, { revision: review.expectedRevision });
  const [source, setSource] = useState<Awaited<ReturnType<typeof sessionImportPreviewEvents>> | null>(null);
  const targetImages = useImportedImages(reader.scope ? {
    kind: "published", importId: review.importId, revisionId: reader.scope.revision,
  } : null, reader.events, JSON.stringify([review.reviewDigest, reader.scope?.leaf]));
  const sourceImages = useImportedImages({ kind: "preview", previewId: review.previewId },
    source?.events ?? [], JSON.stringify([review.reviewDigest, inspected.leafId]));
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const flight = useRef(false);
  const lifetime = useRef(0);
  const cursor = useRef<string | null>(null);
  const load = useCallback(async (after: string | null = null) => {
    if (flight.current) return;
    flight.current = true;
    const token = lifetime.current;
    setBusy(true); setError(null); cursor.current = after;
    try {
      const page = await sessionImportPreviewEvents(review.previewId, after, 20, inspected.leafId);
      if (lifetime.current === token) setSource(page);
    } catch (cause) {
      if (lifetime.current === token) setError(String(cause));
    } finally {
      if (lifetime.current === token) { flight.current = false; setBusy(false); }
    }
  }, [review.previewId, inspected.leafId]);
  useEffect(() => {
    ++lifetime.current; flight.current = false;
    void load();
    return () => { ++lifetime.current; flight.current = false; };
  }, [load]);
  const ready = !!source && !!reader.scope && !reader.error && !error && !busy && !reader.busy;
  useEffect(() => { markReady(review.reviewDigest, ready); }, [markReady, ready, review.reviewDigest]);
  const failure = (cause: string | null, retry: () => void) => cause ? <div role="alert">
    <p>{tr(importErrorKey(cause))}</p>
    <button type="button" className="btn btn--sm" onClick={retry}>{tr("sessionImport.comparison.retryRead")}</button>
  </div> : null;
  return <>
    <p className="settings-row__desc">{tr("sessionImport.historyLink.scopeHint")}</p>
    <div className="session-import__comparison-pair">
      <section className="session-import__comparison-source" aria-label={tr("sessionImport.historyLink.target")}>
        <h3>{tr("sessionImport.historyLink.target")}</h3>
        <p className="session-import__locator"><code>{review.importId}@{review.expectedRevision}</code></p>
        {reader.busy ? <p role="status">{tr("sessionImport.peekLoading")}</p> : null}
        {failure(reader.error, () => void reader.reload())}
        <ImportedEventList events={reader.events} tr={tr} idPrefix="association-target" images={targetImages} />
        <div className="session-import__toolbar">
          <button type="button" className="btn btn--sm" disabled={reader.busy || !reader.previousCursor}
            onClick={() => void reader.loadPrevious()}>{tr("sessionImport.previousPage")}</button>
          <button type="button" className="btn btn--sm" disabled={reader.busy || !reader.nextCursor}
            onClick={() => void reader.loadMore()}>{tr("sessionImport.nextPage")}</button>
        </div>
      </section>
      <section className="session-import__comparison-source" aria-label={tr("sessionImport.historyLink.source")}>
        <h3>{tr("sessionImport.historyLink.source")}</h3>
        <p className="session-import__locator"><code>{review.previewId} · {inspected.leafId}</code></p>
        {busy ? <p role="status">{tr("sessionImport.peekLoading")}</p> : null}
        {failure(error, () => void load(cursor.current))}
        <ImportedEventList events={source?.events ?? []} tr={tr} idPrefix="association-source" images={sourceImages} />
        <div className="session-import__toolbar">
          <button type="button" className="btn btn--sm" disabled={busy}
            onClick={() => void load()}>{tr("sessionImport.refresh")}</button>
          <button type="button" className="btn btn--sm" disabled={busy || !!error || !source?.nextCursor}
            onClick={() => void load(source?.nextCursor)}>{tr("sessionImport.nextPage")}</button>
        </div>
      </section>
    </div>
    <details><summary>{tr("sessionImport.comparison.references")}</summary>
      <p className="session-import__locator"><code>{review.sourceId}</code></p>
      <p className="session-import__locator"><code>{review.oldNativeSessionId} → {review.currentNativeSessionId}</code></p>
      <p className="session-import__locator"><code>{review.selectionHash}</code></p>
    </details>
  </>;
}

export function SessionImportAssociationDialog({ tr, ui }: { tr: SessionImportTFn; ui: AssociationUi }) {
  if (!ui.dialog) return null;
  const { target, review, member } = ui.dialog;
  const choices = (ui.targets?.items ?? []).filter((entry) => !entry.hidden && entry.provider === "cursor" && entry.currentRevision);
  if (target && !choices.some((entry) => entry.importId === target.importId)) choices.unshift(target);
  return <div onKeyDown={(event) => {
    if ((event.nativeEvent.isComposing || event.nativeEvent.keyCode === 229) &&
        (event.key === "Enter" || event.key === " ")) event.preventDefault();
    if (event.key !== "Tab" && event.key !== "Escape") event.stopPropagation();
  }}><GlassModal open onClose={ui.close} title={tr("sessionImport.historyLink.title")}
    closeLabel={tr("sessionImport.close")} wrapBody size="lg"
    className="session-import-modal session-import-comparison"
    footer={<>
      <button type="button" className="btn btn--ghost" onClick={ui.close}>{tr("common.cancel")}</button>
      <button type="button" className="btn btn--primary" disabled={ui.busy || !ui.dialog.ready || !review}
        onClick={() => ui.confirm()}>{tr("sessionImport.historyLink.confirm")}</button>
    </>}>
    <p>{member.title}</p>
    <p className="settings-row__desc">{tr("sessionImport.historyLink.notice")}</p>
    <Select value={target?.importId ?? ""} disabled={ui.busy} aria-label={tr("sessionImport.historyLink.target")}
      options={[{ value: "", label: tr("sessionImport.historyLink.choose") },
        ...choices.map((entry) => ({ value: entry.importId, label: `${entry.alias || entry.title} · ${entry.importId}` }))]}
      onChange={(id) => void ui.choose(id)} />
    <div className="session-import__toolbar">
      <button type="button" className="btn btn--sm" disabled={ui.busy} onClick={() => void ui.firstTargets()}>{tr("sessionImport.refresh")}</button>
      <button type="button" className="btn btn--sm" disabled={ui.busy || !ui.targets?.nextCursor}
        onClick={() => void ui.nextTargets()}>{tr("sessionImport.nextPage")}</button>
    </div>
    {ui.busy ? <p role="status">{tr("sessionImport.peekLoading")}</p> : null}
    {ui.error ? <p role="alert">{tr(importErrorKey(ui.error))}</p> : null}
    {ui.error && target ? <button type="button" className="btn btn--sm" disabled={ui.busy}
      onClick={() => void ui.choose(target.importId)}>{tr("sessionImport.comparison.retryRead")}</button> : null}
    {ui.targets && !choices.length ? <p role="status">{tr("sessionImport.historyLink.empty")}</p> : null}
    {review ? <AssociationContent key={review.review.reviewDigest} inspected={review} tr={tr} markReady={ui.markReady} /> : null}
  </GlassModal></div>;
}
