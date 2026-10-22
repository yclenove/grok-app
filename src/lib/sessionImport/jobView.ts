import type { JobItem, JobRecord } from "@/lib/api/sessionMigration";

export type JobUiStatus =
  | "queued"
  | "running"
  | "cancelling"
  | "completed"
  | "completed_with_issues"
  | "cancelled"
  | "failed"
  | "recovery_needed"
  | "unknown";

/**
 * Undo keeps an item for one of a few reasons the Host writes into the durable
 * undo record. That text is part of the record, so it is matched exactly here
 * instead of being reworded on the Host.
 */
export function undoConflictReasonKey(error: string | null | undefined):
  | "sessionImport.continuationConflict"
  | "sessionImport.undoReason.laterRevision"
  | "sessionImport.undoReason.ownership"
  | null {
  if (!error) return null;
  if (error.endsWith("continuation sessions still reference this import")) return "sessionImport.continuationConflict";
  if (error === "later revision kept") return "sessionImport.undoReason.laterRevision";
  if (error === "ownership receipt missing" || error === "previous revision is unknown") return "sessionImport.undoReason.ownership";
  return null;
}

export function jobUiStatus(state: string | undefined): JobUiStatus {
  switch (state) {
    case "queued":
    case "running":
    case "cancelling":
    case "completed":
    case "completed_with_issues":
    case "cancelled":
    case "failed":
    case "recovery_needed":
      return state;
    default:
      return "unknown";
  }
}

export function jobStatusKey(status: JobUiStatus):
  | "sessionImport.job.queued"
  | "sessionImport.job.running"
  | "sessionImport.job.cancelling"
  | "sessionImport.job.completed"
  | "sessionImport.job.completedWithIssues"
  | "sessionImport.job.cancelled"
  | "sessionImport.job.failed"
  | "sessionImport.job.recoveryNeeded"
  | "sessionImport.job.unknown" {
  switch (status) {
    case "queued":
      return "sessionImport.job.queued";
    case "cancelling":
      return "sessionImport.job.cancelling";
    case "completed_with_issues":
      return "sessionImport.job.completedWithIssues";
    case "cancelled":
      return "sessionImport.job.cancelled";
    case "failed":
      return "sessionImport.job.failed";
    case "completed":
      return "sessionImport.job.completed";
    case "running":
      return "sessionImport.job.running";
    case "recovery_needed":
      return "sessionImport.job.recoveryNeeded";
    default:
      return "sessionImport.job.unknown";
  }
}

/** Counts only. Unknown totals stay unknown — never a fake percent. */
export function jobProgress(job: Pick<JobRecord, "items" | "itemCount" | "progress"> | null): {
  done: number;
  total: number | null;
} {
  if (!job) return { done: 0, total: null };
  const done = job.progress?.done ?? job.items.filter(
    (item) => item.phase === "committed" || !!item.importId || item.phase === "failed" || item.phase === "cancelled"
      || item.phase === "tombstoned",
  ).length;
  const total = job.itemCount && job.itemCount > 0 ? job.itemCount : null;
  return { done, total };
}

/** New hosts report the whole job; older hosts retain their loaded-item fallback. */
export function jobCanRetry(job: Pick<JobRecord, "items" | "progress">): boolean {
  if (job.progress) return job.progress.retryable > 0;
  return job.items.some((item) => ["failed", "cancelled", "recovery_needed"].includes(item.phase));
}

export type JobItemGroup = "complete" | "partial" | "failed" | "kept" | "pending" | "stopped";

export function jobItemGroup(item: JobItem): JobItemGroup {
  // Undo and cancellation retain the original outcome; phase wins over that history.
  if (item.phase === "undone" || item.phase === "cancelled" || item.phase === "tombstoned") return "stopped";
  if (item.phase === "recovery_needed") return "failed";
  // Undo deliberately kept this import (continuation, later revision, unproven
  // ownership); it is still imported, so it is not a failure.
  if (item.phase === "conflict") return "kept";
  const outcome = item.outcome ?? item.phase;
  if (outcome === "failed" || item.phase === "failed") return "failed";
  if (outcome === "partial" || outcome === "unsupported") return "partial";
  if (item.phase === "committed" || outcome === "complete" || outcome === "deduplicated") {
    return "complete";
  }
  return "pending";
}

export function groupJobItems(items: JobItem[]): Record<JobItemGroup, JobItem[]> {
  const groups: Record<JobItemGroup, JobItem[]> = {
    complete: [],
    partial: [],
    failed: [],
    kept: [],
    pending: [],
    stopped: [],
  };
  for (const item of items) {
    groups[jobItemGroup(item)].push(item);
  }
  return groups;
}
