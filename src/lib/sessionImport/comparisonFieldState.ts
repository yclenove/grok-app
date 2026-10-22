import type { SourceComparisonFieldPage } from "@/lib/api/sessionMigrationComparisonField";
import { copyComparisonOwner, validateComparisonOwner, type SourceComparisonOwner } from "./sourceComparisonState";
import { groupStateError } from "./groupSelectionState";

export const COMPARISON_FIELD_BYTES = 4096;
export type ComparisonFieldContext = {
  owner: SourceComparisonOwner;
  side: "left" | "right";
  position: number;
  eventId: string;
  fieldsDigest: string;
};
export type ComparisonFieldState = {
  field: string | null;
  fieldNames: string[];
  page: SourceComparisonFieldPage | null;
  pageIndex: number;
  loading: boolean;
  error: string | null;
};
export const emptyComparisonField = (): ComparisonFieldState => ({ field: null, fieldNames: [], page: null, pageIndex: 0, loading: false, error: null });
export const fieldName = (value: string) => typeof value === "string" && /^[a-zA-Z0-9_]{1,128}$/.test(value);
const digest = (value: string | null) => typeof value === "string" && /^[a-f0-9]{64}$/.test(value);
const count = (value: number) => Number.isSafeInteger(value) && value >= 0;

export function copyFieldContext(context: ComparisonFieldContext): ComparisonFieldContext {
  return { ...context, owner: copyComparisonOwner(context.owner) };
}
export function validateFieldContext(context: ComparisonFieldContext) {
  validateComparisonOwner(context.owner);
  if (!["left", "right"].includes(context.side) || !count(context.position) ||
    typeof context.eventId !== "string" || !context.eventId || !digest(context.fieldsDigest)) groupStateError("invalid comparison field owner");
}

export function validateFieldPage(context: ComparisonFieldContext, field: string,
  page: SourceComparisonFieldPage, start: number, previous: SourceComparisonFieldPage | null, attemptedCursor?: string) {
  if (!page || page.version !== 1) throw new Error("SCHEMA_UNSUPPORTED: comparison field");
  for (const side of ["left", "right"] as const) {
    const actual = page[side], expected = context.owner[side];
    if (!actual || actual.previewId !== expected.previewId || actual.candidateId !== expected.candidateId ||
      actual.sourceRevision !== expected.sourceRevision || actual.leafId !== expected.leafId) groupStateError("foreign comparison field pair");
  }
  if (page.side !== context.side || page.position !== context.position || page.field !== field ||
    page.eventId !== context.eventId || page.fieldsDigest !== context.fieldsDigest ||
    typeof page.available !== "boolean" || typeof page.text !== "string" ||
    !count(page.startByte) || !count(page.endByte) || !count(page.totalBytes) ||
    page.startByte !== start || page.endByte < start || page.endByte > page.totalBytes || page.totalBytes > 0xffffffff ||
    typeof page.complete !== "boolean" || page.complete !== (page.endByte === page.totalBytes) ||
    (page.nextCursor === null) !== page.complete ||
    (page.nextCursor !== null && (typeof page.nextCursor !== "string" || !page.nextCursor || page.nextCursor === attemptedCursor)) ||
    !Array.isArray(page.fieldNames) || page.fieldNames.length > 64 || !page.fieldNames.every(fieldName) ||
    new Set(page.fieldNames).size !== page.fieldNames.length) groupStateError("invalid comparison field segment");
  const bytes = new TextEncoder().encode(page.text);
  if (bytes.length !== page.endByte - page.startByte || new TextDecoder().decode(bytes) !== page.text ||
    bytes.length > COMPARISON_FIELD_BYTES || new TextEncoder().encode(JSON.stringify(page)).length > 256 * 1024) {
    throw new Error("RESOURCE_LIMIT: comparison field byte budget");
  }
  if (page.available ? (!digest(page.fieldDigest) || !page.fieldNames.includes(field) ||
    page.totalBytes === 0 || (page.startByte < page.totalBytes && bytes.length === 0)) :
    (page.fieldDigest !== null || page.text !== "" || start !== 0 || page.totalBytes !== 0 || page.fieldNames.includes(field))) {
    groupStateError("comparison field availability mismatch");
  }
  if (previous && (page.available !== previous.available || page.totalBytes !== previous.totalBytes ||
    page.fieldDigest !== previous.fieldDigest || JSON.stringify(page.fieldNames) !== JSON.stringify(previous.fieldNames))) {
    groupStateError("comparison field changed between segments");
  }
}
