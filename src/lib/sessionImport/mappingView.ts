import type { SourceRegistration, WorkspaceMapping } from "@/lib/api/sessionMigration";
import { sourceIdentityLabel } from "./sourceLabel";

export type MappingRow = WorkspaceMapping & { label: string; sourceLabel: string | null };

/** Keep server groups intact, including identical paths from different installations. */
export function mappingRows(mappings: WorkspaceMapping[], registrations: SourceRegistration[] = [], unknownFolder?: string): MappingRow[] {
  return mappings.map((row) => ({
    ...row,
    label: row.explicitTarget || row.suggestedTarget || (row.source ? unknownFolder : null) || row.rootId,
    sourceLabel: sourceIdentityLabel(row.source, registrations),
  }));
}

export function isPrimaryMapping(row: WorkspaceMapping, index: number, rows: WorkspaceMapping[]): boolean {
  if (rows.some((item) => item.primary)) return !!row.primary;
  return index === 0;
}
