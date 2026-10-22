import { invoke } from "./host";
import type { SourceComparisonSide } from "./sessionMigrationComparison";

export type SourceComparisonFieldRequest = {
  version: 1;
  left: Readonly<SourceComparisonSide>;
  right: Readonly<SourceComparisonSide>;
  side: "left" | "right";
  position: number;
  field: string;
  expectedFieldsDigest: string;
  cursor?: string | null;
  limitBytes?: number | null;
};

export type SourceComparisonFieldPage = {
  version: 1;
  left: SourceComparisonSide;
  right: SourceComparisonSide;
  side: "left" | "right";
  position: number;
  field: string;
  eventId: string;
  fieldsDigest: string;
  fieldDigest: string | null;
  fieldNames: string[];
  available: boolean;
  text: string;
  startByte: number;
  endByte: number;
  totalBytes: number;
  nextCursor: string | null;
  /** Selected field tail only, never full-history completeness/equivalence. */
  complete: boolean;
};

function copySide(side: Readonly<SourceComparisonSide>): SourceComparisonSide {
  return { previewId: side.previewId, candidateId: side.candidateId,
    sourceRevision: side.sourceRevision, leafId: side.leafId };
}

export function sessionImportSourceComparisonField(request: Readonly<SourceComparisonFieldRequest>) {
  return invoke<SourceComparisonFieldPage>("session_import_source_comparison_field", {
    request: { version: request.version, left: copySide(request.left), right: copySide(request.right),
      side: request.side, position: request.position, field: request.field,
      expectedFieldsDigest: request.expectedFieldsDigest,
      ...(request.cursor !== undefined ? { cursor: request.cursor } : {}),
      ...(request.limitBytes !== undefined ? { limitBytes: request.limitBytes } : {}) },
  });
}
