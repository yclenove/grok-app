import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { GlassModal } from "@/components/GlassModal";
import { createT, type Locale } from "@/i18n";
import { useImportedReader } from "@/hooks/useImportedReader";
import type { CatalogEntry } from "@/lib/api/sessionMigration";
import { isDialogFocusAllowed } from "@/lib/a11yFocus";
import { eventHasIssue, importedEventDisplay } from "@/lib/sessionImport/eventView";
import { nextReaderHit, readerShortcutDirection } from "@/lib/sessionImport/readerNavigation";
import { windowReaderEvents } from "@/lib/sessionImport/readerWindow";
import { ImportedReaderActions } from "./ImportedReaderActions";
import { ImportedReaderError } from "./ImportedReaderError";
import { ImportedRevisionNotice } from "./ImportedRevisionNotice";
import { SessionImportReviewAssociation } from "./SessionImportReviewAssociation";
import { SessionImportReaderContent } from "./SessionImportReaderContent";
import type { ReaderPane } from "./SessionImportReaderSplit";
import { useContainerNarrow } from "./useContainerNarrow";
import { catalogEntry, draft } from "./sessionImportReviewFixtures";
import { reviewReaderClient, type ReviewReaderState } from "./sessionImportReviewReaderClient";

export function SessionImportReviewReader({ locale, theme, state, onClose, onContinue, entry = catalogEntry }: {
  locale: Locale;
  theme: string;
  state: ReviewReaderState;
  onClose: () => void;
  onContinue: () => void;
  entry?: CatalogEntry;
}) {
  const tr = useMemo(() => createT(locale), [locale]);
  const client = useMemo(() => reviewReaderClient(state, entry), [entry, state]);
  const reportError = useCallback(() => {}, []); // The real Reader stores and renders errors.
  const reader = useImportedReader(entry.importId, reportError, {
    revision: entry.currentRevision ?? draft.revisionId, leaf: draft.leaf,
  }, client);
  const measured = useContainerNarrow(true);
  const [roleFilter, setRoleFilter] = useState<"all" | "user" | "assistant">("all");
  const [issuesOnly, setIssuesOnly] = useState(false);
  const [pane, setPane] = useState<ReaderPane>("events");
  const [highlight, setHighlight] = useState<string | null>(null);
  const [copyFailed, setCopyFailed] = useState(false);
  const bootstrapped = useRef(false);
  const searched = useRef(false);
  const visible = windowReaderEvents(reader.events.filter((raw) => {
    const event = importedEventDisplay(raw);
    return (roleFilter === "all" || event.role === roleFilter) && (!issuesOnly || eventHasIssue(event));
  }), highlight);

  useEffect(() => {
    if (state === "idle" || !reader.scope || bootstrapped.current) return;
    bootstrapped.current = true;
    reader.setQuery("锚点");
  }, [reader.scope, reader.setQuery, state]);
  useEffect(() => {
    if (!bootstrapped.current || searched.current || reader.query !== "锚点") return;
    searched.current = true;
    void reader.search();
  }, [reader.query, reader.search]);
  useEffect(() => {
    const node = highlight ? document.getElementById(`import-ev-${highlight}`) : null;
    if (node && isDialogFocusAllowed(node)) {
      node.focus({ preventScroll: true });
      node.scrollIntoView({ block: "nearest" });
    }
  }, [highlight, reader.events]);

  const reveal = async (id: string) => {
    setRoleFilter("all"); setIssuesOnly(false); setPane("events");
    if (await reader.ensureEvent(id)) setHighlight(id);
  };
  const jump = async (direction: -1 | 1) => {
    const id = nextReaderHit(reader.hits.map((hit) => hit.eventId), highlight, direction);
    if (id) await reveal(id);
  };
  return (
    <GlassModal open onClose={onClose} title={entry.title} size="lg" wrapBody
      closeLabel={tr("sessionImport.close")} dataTheme={theme}
      className="session-import-modal session-import-reader session-import-review-frame"
      footer={<ImportedReaderActions tr={tr} busy={reader.busy} continueBusy={!reader.scope || reader.busy}
        hasPrevious={!!reader.previousCursor} hasNext={!!reader.nextCursor} hasText={visible.items.length > 0}
        hasIssue={visible.items.some((raw) => eventHasIssue(importedEventDisplay(raw)))}
        copyStatus={copyFailed ? "failed" : "idle"} exportNotice={reader.notice} exportError={reader.exportError}
        onPage={(direction) => { void (direction === "next" ? reader.loadMore() : reader.loadPrevious()); }}
        onCopy={() => setCopyFailed(true)} onExport={() => void reader.exportMarkdown()}
        onIssue={() => { const issue = visible.items.find((raw) => eventHasIssue(importedEventDisplay(raw)));
          const id = importedEventDisplay(issue).id; if (id) void reveal(id); }}
        onContinue={onContinue} onKeyDown={(event) => {
          if (event.defaultPrevented || !reader.hits.length || !isDialogFocusAllowed(event.currentTarget)) return;
          const direction = readerShortcutDirection(event.nativeEvent);
          if (direction != null) { event.preventDefault(); void jump(direction); }
        }} />}>
      <div ref={measured.ref} data-review-surface="reader" data-layout={measured.narrow ? "narrow" : "wide"}
        data-measured-width={measured.width}>
        <p className="session-import__read-only">{tr("sessionImport.readOnly")}</p>
        <ImportedRevisionNotice health={reader.health} tr={tr} />
        <SessionImportReviewAssociation tr={tr} importId={entry.importId} />
        <ImportedReaderError tr={tr} error={reader.error} busy={reader.busy} copyBusy={false}
          onReload={reader.reload} onCopy={() => setCopyFailed(true)} />
        <SessionImportReaderContent tr={tr} reader={reader} readerIdentity="synthetic-reader"
          readerEvents={visible.items} eventsWindowed={visible.windowed}
          roleFilter={roleFilter} setRoleFilter={setRoleFilter} issuesOnly={issuesOnly} setIssuesOnly={setIssuesOnly}
          highlightId={highlight} readerPane={pane} setReaderPane={setPane} narrow={measured.narrow}
          jumpHit={jump} revealReaderHit={reveal} onContinueFrom={onContinue} />
      </div>
    </GlassModal>
  );
}
