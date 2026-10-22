import { useMemo, useState } from "react";
import type { MessageKey, Vars } from "@/i18n";
import { GlassModal } from "@/components/GlassModal";
import { boundedTextEnd } from "@/lib/sessionImport/eventView";
import { ImportedReaderSearch } from "./ImportedReaderSearch";
import { ImportCopyButton } from "./ImportCopyButton";

type Tr = (key: MessageKey, vars?: Vars) => string;
type TextHit = { index: number; length: number };
const MAX_LITERAL_QUERY_BYTES = 4096;

/** A literal search keeps offsets in the original text, including Unicode case folds. */
function findMatch(text: string, query: string, after: number, direction: -1 | 1): TextHit | null {
  if (!query) return null;
  const pattern = new RegExp(query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), "giu");
  if (direction === 1) {
    pattern.lastIndex = after;
    let match = pattern.exec(text);
    if (!match) { pattern.lastIndex = 0; match = pattern.exec(text); }
    return match ? { index: match.index, length: match[0].length } : null;
  }
  let previous: TextHit | null = null;
  let last: TextHit | null = null;
  for (let match = pattern.exec(text); match; match = pattern.exec(text)) {
    last = { index: match.index, length: match[0].length };
    if (match.index < after) previous = last;
  }
  return previous ?? last;
}

export function ImportedTextReader({ text, tr, onClose }: { text: string; tr: Tr; onClose: () => void }) {
  const starts = useMemo(() => {
    const offsets = [0];
    for (let next = boundedTextEnd(text); next < text.length; next = boundedTextEnd(text, next)) offsets.push(next);
    return offsets;
  }, [text]);
  const [page, setPage] = useState(0);
  const [query, setQuery] = useState("");
  const [searched, setSearched] = useState("");
  const [hit, setHit] = useState<TextHit | null>(null);
  const [noHits, setNoHits] = useState(false);
  const [queryTooLong, setQueryTooLong] = useState(false);
  const start = starts[Math.min(page, starts.length - 1)];
  const visible = text.slice(start, boundedTextEnd(text, start));
  const search = (direction: -1 | 1 = 1, restart = false) => {
    const needle = restart ? query : searched;
    // Validate only a submitted literal; do not alter spaces or an IME draft.
    // The code-unit check also avoids allocating UTF-8 for a very large paste.
    if (needle.length > MAX_LITERAL_QUERY_BYTES ||
        new TextEncoder().encode(needle).byteLength > MAX_LITERAL_QUERY_BYTES) {
      setQueryTooLong(true);
      setSearched("");
      setHit(null);
      setNoHits(false);
      return;
    }
    setQueryTooLong(false);
    const offset = restart || hit === null ? 0 : hit.index + (direction === 1 ? hit.length : 0);
    const match = findMatch(text, needle, offset, direction);
    setSearched(needle);
    setHit(match);
    setNoHits(!!needle && match === null);
    if (match !== null) setPage(Math.max(0, starts.findIndex((value, index) =>
      value <= match.index && (starts[index + 1] ?? text.length + 1) > match.index)));
  };
  const localTr: Tr = (key, vars) => tr(key === "sessionImport.searchPlaceholder"
    ? "sessionImport.textSearchPlaceholder" : key, vars);
  const highlight = hit && hit.index >= start && hit.index < start + visible.length ? hit.index - start : null;
  return <div onKeyDown={(event) => {
    // Portal events still bubble through the underlying reader's React tree.
    // Leave Tab/Escape to GlassModal; keep other keys out of background shortcuts.
    if (event.key !== "Tab" && event.key !== "Escape") event.stopPropagation();
  }}><GlassModal open onClose={onClose} title={tr("sessionImport.readFullText")}
    closeLabel={tr("sessionImport.close")} wrapBody size="lg"
    className="session-import-modal session-import-text-reader"
    footer={<div className="session-import__toolbar">
      <button type="button" className="btn btn--ghost" disabled={page === 0}
        onClick={() => setPage((value) => value - 1)}>{tr("sessionImport.previousPage")}</button>
      <button type="button" className="btn btn--ghost" disabled={page + 1 >= starts.length}
        onClick={() => setPage((value) => value + 1)}>{tr("sessionImport.nextPage")}</button>
      <ImportCopyButton key={page} tr={tr} text={visible} labelKey="sessionImport.copyVisible" />
    </div>}
  >
    <ImportedReaderSearch tr={localTr} busy={false} query={query} hasHits={hit !== null}
      onQueryChange={setQuery} onSearch={() => search(1, true)} onClear={() => {
        setQuery(""); setSearched(""); setHit(null); setNoHits(false); setQueryTooLong(false);
      }} />
    {queryTooLong ? <p role="alert" className="settings-row__desc">
      {tr("sessionImport.searchQueryTooLong")}
    </p> : null}
    {hit !== null ? <div className="session-import__toolbar">
      <button type="button" className="btn btn--ghost" onClick={() => search(-1)}>{tr("sessionImport.prevHit")}</button>
      <button type="button" className="btn btn--ghost" onClick={() => search(1)}>{tr("sessionImport.nextHit")}</button>
    </div> : null}
    {noHits ? <p role="status">{tr("sessionImport.textSearchNoHits")}</p> : null}
    <p role="status">{tr("sessionImport.textPage", { n: page + 1, total: starts.length })}</p>
    <pre className="session-import__full-text">{highlight === null ? visible : <>
      {visible.slice(0, highlight)}<mark>{visible.slice(highlight, highlight + hit!.length)}</mark>
      {visible.slice(highlight + hit!.length)}
    </>}</pre>
  </GlassModal></div>;
}
