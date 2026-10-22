import type { MessageKey, Vars } from "@/i18n";
import { compactionHistoryLeaf } from "@/lib/sessionImport/eventView";
import { openImportedHistory } from "@/lib/sessionImport/open";

/** Read the conversation before a compaction, then return to the latest one. */
export function ImportedCompactionHistory({ importId, revision, leaf, events, busy, tr }: {
  importId: string | null;
  revision: string | null;
  /** Leaf explicitly requested for this reader; null is the latest conversation. */
  leaf: string | null;
  events: readonly unknown[];
  busy: boolean;
  tr: (key: MessageKey, vars?: Vars) => string;
}) {
  if (!importId || !revision) return null;
  const history = compactionHistoryLeaf(events);
  if (!leaf && !history) return null;
  return (
    <p className="settings-row__desc session-import__compaction-history">
      {history ? (
        <button type="button" className="btn btn--sm btn--ghost" disabled={busy}
          onClick={() => openImportedHistory({ importId, revision, leaf: history.leaf })}>
          {tr("sessionImport.compactionHistory", { n: history.count })}
        </button>
      ) : null}
      {leaf ? (
        <button type="button" className="btn btn--sm btn--ghost" disabled={busy}
          onClick={() => openImportedHistory({ importId, revision })}>
          {tr("sessionImport.compactionLatest")}
        </button>
      ) : null}
    </p>
  );
}
