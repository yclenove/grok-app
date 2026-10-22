import type { ImportedReaderClient } from "@/hooks/useImportedReader";
import type { CatalogEntry } from "@/lib/api/sessionMigration";
import { catalogEntry, draft } from "./sessionImportReviewFixtures";

export type ReviewReaderState = "idle" | "searching" | "empty" | "error" | "hits";

const events = [
  { id: "reader-1", kind: "message", role: "user", text: "Synthetic user note 🙂 锚点" },
  { id: "reader-2", kind: "message", role: "assistant", text: "Synthetic **assistant** note. Not sent." },
  { id: "reader-3", kind: "message", role: "user", text: "Synthetic second page 🙂 锚点" },
  { id: "reader-4", kind: "unsupported", source_type: "synthetic-unknown" },
];

/** Every method is local and validates its scope; export fails before clipboard access. */
export function reviewReaderClient(state: ReviewReaderState, entry: CatalogEntry = catalogEntry): ImportedReaderClient {
  const revisionId = entry.currentRevision ?? draft.revisionId;
  const validate = (id: string, revision?: string | null, leaf?: string | null) => {
    if (id !== entry.importId || (revision && revision !== revisionId) ||
        (leaf && leaf !== draft.leaf)) throw new Error("REVISION_MISMATCH: synthetic scope");
  };
  return {
    async events(id, revision, cursor, _limit, leaf) {
      validate(id, revision, leaf);
      if (cursor && !["page-1", "page-2"].includes(cursor)) throw new Error("CURSOR_STALE: synthetic");
      const second = cursor === "page-2";
      return {
        events: events.slice(second ? 2 : 0, second ? 4 : 2),
        health: { outcome: "partial", issueCount: 1, issues: [{ code: "UNSUPPORTED", message: "Synthetic unsupported record" }], diagnosticsLimited: false },
        previousCursor: second ? "page-1" : null, nextCursor: second ? null : "page-2",
        total: events.length, start: second ? 2 : 0, end: second ? 4 : 2,
        revision: revisionId, leaf: draft.leaf,
      };
    },
    async context(id, eventId, revision, leaf) {
      validate(id, revision, leaf);
      const index = events.findIndex((event) => event.id === eventId);
      if (index < 0) throw new Error("EVENT_MISSING: synthetic");
      return {
        events, targetIndex: index, ordinal: index, position: index, total: events.length,
        revision: revisionId, leaf: draft.leaf, anchorCursor: "page-1",
        previousCursor: null, nextCursor: null,
      };
    },
    async search(id, query, revision, cursor, leaf) {
      validate(id, revision, leaf);
      if (cursor) throw new Error("CURSOR_STALE: synthetic");
      if (state === "error") throw new Error("SOURCE_BUSY: synthetic search");
      if (state === "searching") await new Promise((resolve) => setTimeout(resolve, 1500));
      const normalized = query.trim().toLowerCase();
      return {
        hits: state === "empty" || !normalized ? [] : events.flatMap((event, ordinal) =>
          event.text?.toLowerCase().includes(normalized)
            ? [{ eventId: event.id, ordinal, snippet: event.text, kind: event.kind }] : []),
        previousCursor: null, nextCursor: null,
      };
    },
    async exportMarkdown(id, revision, leaf) {
      validate(id, revision, leaf);
      throw new Error("EXPORT_UNAVAILABLE: synthetic review does not write the clipboard");
    },
  };
}
