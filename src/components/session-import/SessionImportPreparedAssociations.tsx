import { useEffect, useRef, useState } from "react";
import type { useSessionImportUi } from "@/hooks/useSessionImport";
import type { SessionImportTFn } from "./sessionImportHostLabels";

type ImportUi = ReturnType<typeof useSessionImportUi>;
type PreparedAssociationUi = Pick<ImportUi,
  "busy" | "prepareItems" | "prepareNextCursor" | "prepareItemsError" |
  "loadMorePrepare" | "retryPrepareItems"> & {
  associations: Pick<ImportUi["associations"], "confirmed" | "open" | "clear">;
};

const pageSize = 50;

/** Keyed by prepared batch/version by the mapping step; confirmations live above it. */
export function SessionImportPreparedAssociations({ tr, ui }: {
  tr: SessionImportTFn; ui: PreparedAssociationUi;
}) {
  const [page, setPage] = useState(0);
  const [loading, setLoading] = useState(false);
  const mounted = useRef(true);
  const flight = useRef(false);
  const failedDestination = useRef<number | null>(null);
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);
  const index = Math.min(page, Math.max(0, Math.ceil(ui.prepareItems.length / pageSize) - 1));
  // Slice metadata BEFORE filtering: sparse non-Cursor pages remain bounded too.
  const visible = ui.prepareItems.slice(index * pageSize, (index + 1) * pageSize)
    .filter((item) => item.state === "prepared" && item.provider === "cursor" && item.previewId);
  const visibleIds = new Set(visible.map((item) => item.candidateId));
  const offPage = ui.associations.confirmed.filter((link) => !visibleIds.has(link.candidateId));
  const cachedNext = (index + 1) * pageSize < ui.prepareItems.length;
  const locked = ui.busy || loading;
  const fetchPage = async (load: () => Promise<boolean>, destination: number) => {
    if (ui.busy || flight.current) return;
    flight.current = true; setLoading(true); failedDestination.current = destination;
    try {
      if (await load() && mounted.current) {
        setPage(destination); failedDestination.current = null;
      }
    } finally {
      if (mounted.current) { flight.current = false; setLoading(false); }
    }
  };
  const next = () => {
    if (locked || flight.current) return;
    if (cachedNext) { failedDestination.current = null; setPage(index + 1); }
    else if (ui.prepareNextCursor && !ui.prepareItemsError) void fetchPage(ui.loadMorePrepare, index + 1);
  };
  return <details className="session-import__history-links" aria-busy={loading}>
    <summary>{tr("sessionImport.historyLink.title")}</summary>
    <p className="settings-row__desc">{tr("sessionImport.historyLink.notice")}</p>
    <ul className="session-import__list" aria-label={tr("sessionImport.historyLink.source")}>
      {visible.map((item) => {
        const link = ui.associations.confirmed.find((entry) => entry.candidateId === item.candidateId);
        return <li key={item.candidateId}>
          <span>{link ? tr("sessionImport.historyLink.confirmed", { source: item.title, target: link.targetTitle }) : item.title}</span>
          <div className="session-import__toolbar">
            <button type="button" className="btn btn--sm" disabled={locked}
              onClick={() => ui.associations.open(item)}>{tr("sessionImport.historyLink.title")}</button>
            {link ? <button type="button" className="btn btn--sm btn--ghost" disabled={locked}
              onClick={() => ui.associations.clear(item.candidateId)}>{tr("sessionImport.association.clear")}</button> : null}
          </div>
        </li>;
      })}
    </ul>
    {!visible.length && !loading ? <p role="status">{tr("sessionImport.historyLink.empty")}</p> : null}
    {offPage.length ? <ul className="session-import__list">{offPage.map((link) =>
      <li key={link.candidateId}><span>{tr("sessionImport.historyLink.confirmed", { source: link.title, target: link.targetTitle })}</span>
        <button type="button" className="btn btn--sm" disabled={locked}
          onClick={() => ui.associations.clear(link.candidateId)}>{tr("sessionImport.association.clear")}</button></li>)}
    </ul> : null}
    {ui.prepareItemsError ? <p role="alert">{tr("sessionImport.prepare.resultsError")}</p> : null}
    {loading ? <p role="status">{tr("sessionImport.peekLoading")}</p> : null}
    <div className="session-import__toolbar">
      <button type="button" className="btn btn--sm" disabled={locked || index === 0}
        onClick={() => { failedDestination.current = null; setPage(index - 1); }}>{tr("sessionImport.previousPage")}</button>
      <span role="status">{tr("sessionImport.page", { n: index + 1 })}</span>
      <button type="button" className="btn btn--sm" disabled={locked || (!cachedNext && (!ui.prepareNextCursor || ui.prepareItemsError))}
        onClick={next}>{tr("sessionImport.nextPage")}</button>
      {ui.prepareItemsError ? <button type="button" className="btn btn--sm" disabled={locked}
        onClick={() => void fetchPage(ui.retryPrepareItems, failedDestination.current ?? index)}>{tr("sessionImport.prepare.retryResults")}</button> : null}
    </div>
  </details>;
}
