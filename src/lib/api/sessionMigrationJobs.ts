import { invoke } from "./host";
import type { JobRecord } from "./sessionMigration";

/** Display metadata only; opening a job still uses the durable job reader. */
export type ImportJobSummary = {
  jobId: string;
  state: JobRecord["state"] | null;
  createdAt: string | null;
  itemCount: number;
};
export type ImportJobHistoryPage = { items: ImportJobSummary[]; nextCursor: string | null };

export function sessionImportJobHistory(cursor: string | null = null): Promise<ImportJobHistoryPage> {
  return invoke("session_import_job_history", { cursor, limit: 20 });
}
