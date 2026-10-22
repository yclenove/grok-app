import type {
  SourceComparisonEvent, SourceComparisonPage, SourceComparisonSide, SourceComparisonSummary,
} from "@/lib/api/sessionMigrationComparison";
import { groupStateError, validateGroupCandidateId, validateGroupScope, type ImportGroupScope } from "./groupSelectionState";

export const COMPARISON_UI_PAGE_SIZE = 50;
export type SourceComparisonOwner = {
  scope: ImportGroupScope;
  groupId: string;
  left: SourceComparisonSide;
  right: SourceComparisonSide;
};
export type SourceComparisonState = {
  owner: SourceComparisonOwner | null;
  page: SourceComparisonPage | null;
  pageIndex: number;
  loading: boolean;
  error: string | null;
};
export function emptySourceComparison(): SourceComparisonState {
  return { owner: null, page: null, pageIndex: 0, loading: false, error: null };
}
const id = (value: string) => typeof value === "string" && value.length > 0;
const count = (value: number) => Number.isSafeInteger(value) && value >= 0;
const nullableText = (value: string | null) => value === null || typeof value === "string";
const kinds = new Set(["message", "tool-call", "tool-result", "compaction", "metadata", "unsupported"]);
const outcomes = new Set(["complete", "partial", "unsupported", "failed"]);

export function copyComparisonOwner(owner: SourceComparisonOwner): SourceComparisonOwner {
  return { scope: { ...owner.scope }, groupId: owner.groupId, left: { ...owner.left }, right: { ...owner.right } };
}

export function validateComparisonOwner(owner: SourceComparisonOwner) {
  validateGroupScope(owner.scope);
  if (!id(owner.groupId) || owner.left.candidateId === owner.right.candidateId ||
    owner.left.previewId === owner.right.previewId) groupStateError("invalid comparison pair");
  for (const side of [owner.left, owner.right]) {
    validateGroupCandidateId(owner.scope, side.candidateId);
    if (![side.previewId, side.sourceRevision, side.leafId].every(id)) groupStateError("invalid comparison side");
  }
}

function validateSummary(
  owner: SourceComparisonOwner, expected: SourceComparisonSide, actual: SourceComparisonSummary,
  previous?: SourceComparisonSummary,
) {
  if (!actual || actual.previewId !== expected.previewId || actual.candidateId !== expected.candidateId ||
    actual.sourceRevision !== expected.sourceRevision || actual.leafId !== expected.leafId ||
    actual.groupId !== owner.groupId || !id(actual.snapshotDigest) || !count(actual.eventCount) ||
    !nullableText(actual.title) || !nullableText(actual.cwd) || !outcomes.has(actual.outcome)) {
    groupStateError("foreign or invalid fixed comparison summary");
  }
  if (previous && (actual.snapshotDigest !== previous.snapshotDigest || actual.eventCount !== previous.eventCount ||
    actual.title !== previous.title || actual.cwd !== previous.cwd || actual.outcome !== previous.outcome)) {
    groupStateError("comparison snapshot changed between pages");
  }
}

function validateEvent(event: SourceComparisonEvent | null, shouldExist: boolean) {
  if (!shouldExist) {
    if (event !== null) groupStateError("comparison missing-side mismatch");
    return;
  }
  if (!event || !id(event.eventId) || !kinds.has(event.kind) || !nullableText(event.timestamp) ||
    !["source", "file-hint", "unknown"].includes(event.timeOrigin) || !Array.isArray(event.spans) ||
    typeof event.excerpt !== "string" || [...event.excerpt].length > 512 ||
    typeof event.truncated !== "boolean" || !id(event.fieldsDigest)) groupStateError("invalid comparison event summary");
  for (const span of event.spans) {
    if (!span || !id(span.artifactId) || typeof span.record !== "string" ||
      (span.block !== undefined && !count(span.block))) groupStateError("invalid comparison source span");
  }
}

const strings = (values: string[]) => Array.isArray(values) && values.every(id) && new Set(values).size === values.length;

/** Fail closed on mixed owners or broken paging; never infer history equivalence. */
export function validateComparisonPage(
  owner: SourceComparisonOwner, page: SourceComparisonPage, start: number, previous: SourceComparisonPage | null,
) {
  if (!page || page.version !== 1 || page.alignment !== "position-only" || page.equivalence !== "unverified") {
    throw new Error("SCHEMA_UNSUPPORTED: fixed-preview positional comparison");
  }
  validateSummary(owner, owner.left, page.left, previous?.left);
  validateSummary(owner, owner.right, page.right, previous?.right);
  if (!count(page.totalPositions) || page.totalPositions !== Math.max(page.left.eventCount, page.right.eventCount) ||
    !Array.isArray(page.rows) || page.rows.length > COMPARISON_UI_PAGE_SIZE || !count(start) ||
    start + page.rows.length > page.totalPositions || (!page.rows.length && start !== page.totalPositions) ||
    typeof page.complete !== "boolean" || page.complete !== (start + page.rows.length === page.totalPositions) ||
    (page.nextCursor === null) !== page.complete || (page.nextCursor !== null && !id(page.nextCursor)) ||
    typeof page.uncertain !== "boolean" || !strings(page.metadataDifferences) ||
    page.metadataDifferences.some((field) => !["title", "cwd", "relations"].includes(field))) {
    groupStateError("invalid comparison pagination");
  }
  if (new TextEncoder().encode(JSON.stringify(page)).length > 256 * 1024) {
    throw new Error("RESOURCE_LIMIT: comparison response byte budget");
  }
  for (const [index, row] of page.rows.entries()) {
    if (!row || row.position !== start + index || !strings(row.differences) || !strings(row.metadataDifferences) ||
      row.metadataDifferences.some((field) => !["event-id", "source-time"].includes(field)) ||
      typeof row.uncertain !== "boolean") groupStateError("invalid comparison position");
    validateEvent(row.left, row.position < page.left.eventCount);
    validateEvent(row.right, row.position < page.right.eventCount);
    const missing = row.left === null || row.right === null;
    if (missing !== row.differences.includes("missing-side") || (missing && !row.uncertain)) {
      groupStateError("invalid comparison missing-side uncertainty");
    }
  }
  if (!page.uncertain && (page.left.outcome !== "complete" || page.right.outcome !== "complete" ||
    page.rows.some((row) => row.uncertain))) groupStateError("comparison uncertainty omitted");
}
