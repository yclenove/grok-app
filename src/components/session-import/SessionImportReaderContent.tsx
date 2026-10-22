import type { Dispatch, SetStateAction } from "react";
import type { useImportedReader } from "@/hooks/useImportedReader";
import type { ImportedImages } from "@/hooks/useImportedImages";
import { importErrorKey } from "@/lib/sessionImport/errorCopy";
import { MAX_READER_HITS, MAX_RENDERED_EVENTS } from "@/lib/sessionImport/readerWindow";
import { ImportedEventList } from "./ImportedEventList";
import { ImportedReaderSearch } from "./ImportedReaderSearch";
import { SessionImportReaderSplit, type ReaderPane } from "./SessionImportReaderSplit";
import type { SessionImportTFn } from "./sessionImportHostLabels";

type ReaderRole = "all" | "user" | "assistant";

/** Shared reader body; navigation, storage and clipboard stay with its owner. */
export function SessionImportReaderContent({
  tr, reader, readerIdentity, readerEvents, eventsWindowed, roleFilter, setRoleFilter,
  issuesOnly, setIssuesOnly, highlightId, readerPane, setReaderPane, narrow,
  jumpHit, revealReaderHit, onContinueFrom, images,
}: {
  tr: SessionImportTFn;
  reader: ReturnType<typeof useImportedReader>;
  readerIdentity: string;
  readerEvents: unknown[];
  images?: ImportedImages;
  eventsWindowed: boolean;
  roleFilter: ReaderRole;
  setRoleFilter: (role: ReaderRole) => void;
  issuesOnly: boolean;
  setIssuesOnly: Dispatch<SetStateAction<boolean>>;
  highlightId: string | null;
  readerPane: ReaderPane;
  setReaderPane: (pane: ReaderPane) => void;
  narrow: boolean;
  jumpHit: (direction: -1 | 1) => Promise<void>;
  revealReaderHit: (eventId: string) => Promise<void>;
  onContinueFrom: (eventId: string) => void;
}) {
  return (
    <>
    <ImportedReaderSearch
      key={`search:${readerIdentity}`}
      tr={tr}
      busy={reader.busy || !reader.scope}
      query={reader.query}
      hasHits={reader.hits.length > 0}
      onQueryChange={reader.setQuery}
      onSearch={reader.search}
      onClear={reader.clearSearch}
    />
    {reader.searchError ? (
      <p role="alert" className="settings-row__desc" data-import-search-error="1" data-import-search-state="error">
        {tr(reader.searchError === "queryTooLong"
          ? "sessionImport.searchQueryTooLong"
          : importErrorKey(reader.searchError))}
      </p>
    ) : null}
    {reader.busy && reader.query.trim() && !reader.error && !reader.searchError && reader.notice !== "empty" ? (
      <p role="status" data-import-search-state="searching">{tr("sessionImport.searchBusy")}</p>
    ) : null}
    <div className="session-import__providers" role="toolbar" aria-label={tr("sessionImport.importedGroup")}>
      {(["all", "user", "assistant"] as const).map((role) => (
        <button
          key={role}
          type="button"
          className="btn btn--sm btn--ghost"
          data-selected={roleFilter === role ? "1" : "0"}
          aria-pressed={roleFilter === role}
          onClick={() => setRoleFilter(role)}
        >
          {role === "all"
            ? tr("sessionImport.roleAll")
            : role === "user"
              ? tr("sessionImport.roleUser")
              : tr("sessionImport.roleAssistant")}
        </button>
      ))}
      <button
        type="button"
        className="btn btn--sm btn--ghost"
        data-selected={issuesOnly ? "1" : "0"}
        aria-pressed={issuesOnly}
        onClick={() => setIssuesOnly((v) => !v)}
      >
        {tr("sessionImport.issuesOnly")}
      </button>
    </div>
    {reader.error ? null : (
      <p role="status">{tr("sessionImport.showingEvents", { n: readerEvents.length })}</p>
    )}
    {reader.error
      ? null
      : readerEvents.length === 0 && (issuesOnly || roleFilter !== "all") ? (
        <p role="status">{tr("sessionImport.filterEmptyEvents")}</p>
      ) : null}
    {reader.notice === "empty" ? (
      <p role="status" data-import-search-state="empty">{tr("sessionImport.searchNoHits")}</p>
    ) : null}
    {reader.hitsWindowed ? (
      <p role="status">{tr("sessionImport.searchWindowed", { n: MAX_READER_HITS })}</p>
    ) : null}
    {reader.eventsWindowed || eventsWindowed ? (
      <p role="status">
        {tr("sessionImport.eventsWindowed", { n: MAX_RENDERED_EVENTS })}
      </p>
    ) : null}
    <SessionImportReaderSplit
      tr={tr}
      narrow={narrow}
      pane={readerPane}
      onPane={setReaderPane}
      matches={reader.hits.length > 0 ? (
      <div className="session-import__hits">
        <p role="status">{tr("sessionImport.searchHits", { n: reader.hits.length })}</p>
        {highlightId && reader.hits.some((h) => h.eventId === highlightId) ? (
          <p role="status">
            {tr("sessionImport.matchIndex", {
              i: reader.hits.findIndex((h) => h.eventId === highlightId) + 1,
              n: reader.hits.length,
            })}
          </p>
        ) : null}
        <div className="session-import__toolbar">
          <button type="button" className="btn btn--sm btn--ghost" aria-keyshortcuts="p" onClick={() => void jumpHit(-1)}>
            {tr("sessionImport.prevHit")}
          </button>
          <button type="button" className="btn btn--sm btn--ghost" aria-keyshortcuts="n" onClick={() => void jumpHit(1)}>
            {tr("sessionImport.nextHit")}
          </button>
        </div>
        <ul className="session-import__list">
          {reader.hits.map((h) => (
            <li key={h.eventId}>
              <button
                type="button"
                className="btn btn--ghost"
                data-selected={highlightId === h.eventId ? "1" : "0"}
                onClick={() => void revealReaderHit(h.eventId)}
              >
                <strong className="session-import__hit-kind">{h.kind === "message" ? tr("sessionImport.scopeMessages") : h.kind}</strong>{" "}
                <span className="session-import__hit-snippet">{h.snippet}</span>
              </button>
            </li>
          ))}
        </ul>
        <div className="session-import__toolbar">
          {reader.hitsPrevious ? (
            <button
              type="button"
              className="btn btn--sm btn--ghost"
              disabled={reader.busy}
              onClick={() => void reader.loadPreviousHits()}
            >
              {tr("sessionImport.prevHitPage")}
            </button>
          ) : null}
          {reader.hitsNext ? (
            <button
              type="button"
              className="btn btn--sm btn--ghost"
              disabled={reader.busy}
              onClick={() => void reader.loadMoreHits()}
            >
              {tr("sessionImport.nextHitPage")}
            </button>
          ) : null}
        </div>
      </div>
    ) : null}
      events={reader.error ? null : (
    <ImportedEventList
      key={readerIdentity}
      events={readerEvents}
      images={images}
      highlightId={highlightId}
      tr={tr}
      onContinueFrom={onContinueFrom}
    />
    )}
    />
    </>
  );
}
