import type { ReactNode } from "react";
import type { SessionImportTFn } from "./sessionImportHostLabels";

export type ReaderPane = "matches" | "events";

/**
 * Wide layouts keep matches beside the event body. Narrow layouts switch
 * panes without unmounting the search field that owns the query.
 * A null matches column is omitted so the event body uses the full width.
 */
export function SessionImportReaderSplit({
  tr,
  narrow,
  pane,
  onPane,
  matches,
  events,
}: {
  tr: SessionImportTFn;
  narrow: boolean;
  pane: ReaderPane;
  onPane: (pane: ReaderPane) => void;
  matches: ReactNode;
  events: ReactNode;
}) {
  const hasMatches = matches != null;
  return (
    <div
      className="session-import__reader-split"
      data-layout={narrow ? "narrow" : "wide"}
      data-pane={hasMatches ? pane : "events"}
      data-matches={hasMatches ? "1" : "0"}
    >
      {narrow && hasMatches ? (
        <div className="session-import__reader-panes" role="group" aria-label={tr("sessionImport.reader.panes")}>
          <button
            type="button"
            className="btn btn--sm"
            aria-pressed={pane === "matches"}
            onClick={() => onPane("matches")}
          >
            {tr("sessionImport.reader.showMatches")}
          </button>
          <button
            type="button"
            className="btn btn--sm"
            aria-pressed={pane === "events"}
            onClick={() => onPane("events")}
          >
            {tr("sessionImport.reader.showEvents")}
          </button>
        </div>
      ) : null}
      {hasMatches ? (
        <div className="session-import__reader-matches" hidden={narrow && pane !== "matches"}>
          {matches}
        </div>
      ) : null}
      <div className="session-import__reader-events" hidden={hasMatches && narrow && pane !== "events"}>
        {events}
      </div>
    </div>
  );
}
