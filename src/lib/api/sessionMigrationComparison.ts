/** Read-only positional comparison of two explicitly opened fixed previews. */
import { invoke } from "./host";

export type SourceComparisonSide = {
  previewId: string;
  candidateId: string;
  sourceRevision: string;
  leafId: string;
};

export type SourceComparisonRequest = {
  version: 1;
  left: Readonly<SourceComparisonSide>;
  right: Readonly<SourceComparisonSide>;
  cursor?: string | null;
  limit?: number | null;
};

export type SourceComparisonSummary = SourceComparisonSide & {
  snapshotDigest: string;
  groupId: string;
  eventCount: number;
  title: string | null;
  cwd: string | null;
  outcome: "complete" | "partial" | "unsupported" | "failed";
};

export type SourceComparisonEvent = {
  eventId: string;
  kind: "message" | "tool-call" | "tool-result" | "compaction" | "metadata" | "unsupported";
  timestamp: string | null;
  timeOrigin: "source" | "file-hint" | "unknown";
  spans: Array<{ artifactId: string; record: string; block?: number }>;
  excerpt: string;
  truncated: boolean;
  /** Digest of the full typed body, not just the bounded excerpt. */
  fieldsDigest: string;
};

export type SourceComparisonRow = {
  position: number;
  left: SourceComparisonEvent | null;
  right: SourceComparisonEvent | null;
  differences: string[];
  metadataDifferences: Array<"event-id" | "source-time">;
  uncertain: boolean;
};

export type SourceComparisonPage = {
  version: 1;
  alignment: "position-only";
  equivalence: "unverified";
  left: SourceComparisonSummary;
  right: SourceComparisonSummary;
  metadataDifferences: Array<"title" | "cwd" | "relations">;
  totalPositions: number;
  rows: SourceComparisonRow[];
  nextCursor: string | null;
  /** Paging reached the end; NOT proof that either source history is complete. */
  complete: boolean;
  /** Source outcomes and this page's rows only; not an all-page equality claim. */
  uncertain: boolean;
};

function copySide(side: Readonly<SourceComparisonSide>): SourceComparisonSide {
  return {
    previewId: side.previewId,
    candidateId: side.candidateId,
    sourceRevision: side.sourceRevision,
    leafId: side.leafId,
  };
}

export function sessionImportSourceComparison(request: Readonly<SourceComparisonRequest>) {
  // Copy the complete ordered scope before invoke can await its host transport.
  // Never rescan, pick another member or retry against a newer source revision.
  return invoke<SourceComparisonPage>("session_import_source_comparison", {
    request: {
      version: request.version,
      left: copySide(request.left),
      right: copySide(request.right),
      ...(request.cursor !== undefined ? { cursor: request.cursor } : {}),
      ...(request.limit !== undefined ? { limit: request.limit } : {}),
    },
  });
}
