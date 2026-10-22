import { expect, test } from "vitest";
import type { JobItem } from "@/lib/api/sessionMigration";
import { groupJobItems, jobCanRetry, jobItemGroup, jobProgress, jobStatusKey, jobUiStatus, undoConflictReasonKey } from "./jobView";

test("cancelling is not shown as a generic running job", () => {
  expect(jobStatusKey("queued")).toBe("sessionImport.job.queued");
  expect(jobUiStatus("cancelling")).toBe("cancelling");
  expect(jobStatusKey("cancelling")).toBe("sessionImport.job.cancelling");
  expect(jobStatusKey("running")).toBe("sessionImport.job.running");
});

test("unknown totals stay unknown instead of a fake percent", () => {
  expect(jobProgress({ items: [], itemCount: 0 })).toEqual({ done: 0, total: null });
  expect(jobProgress({ items: [], itemCount: undefined })).toEqual({ done: 0, total: null });
  const items: JobItem[] = [
    { candidateId: "a", phase: "committed", importId: "imp:1", outcome: "complete" },
    { candidateId: "b", phase: "running" },
  ];
  expect(jobProgress({ items, itemCount: 10 })).toEqual({ done: 1, total: 10 });
});

test("result items split into complete, partial, and failed groups", () => {
  const items: JobItem[] = [
    { candidateId: "ok", phase: "committed", outcome: "complete" },
    { candidateId: "part", phase: "committed", outcome: "partial" },
    { candidateId: "bad", phase: "failed", outcome: "failed" },
    { candidateId: "wait", phase: "queued" },
  ];
  const groups = groupJobItems(items);
  expect(groups.complete.map((i) => i.candidateId)).toEqual(["ok"]);
  expect(groups.partial.map((i) => i.candidateId)).toEqual(["part"]);
  expect(groups.failed.map((i) => i.candidateId)).toEqual(["bad"]);
  expect(groups.pending.map((i) => i.candidateId)).toEqual(["wait"]);
});

test("interrupted jobs retain the Host recovery state instead of claiming to run", () => {
  expect(jobUiStatus("recovery_needed")).toBe("recovery_needed");
  expect(jobStatusKey(jobUiStatus("recovery_needed"))).toBe("sessionImport.job.recoveryNeeded");
});

test("unknown Host states are not silently presented as active imports", () => {
  for (const state of [undefined, "", "future_host_state"]) {
    expect(jobUiStatus(state)).toBe("unknown");
    expect(jobStatusKey(jobUiStatus(state))).toBe("sessionImport.job.unknown");
  }
});

test("terminal phases override retained outcomes after undo, cancellation and recovery", () => {
  expect(jobItemGroup({ candidateId: "undo", phase: "undone", outcome: "complete", importId: "imp:kept" })).toBe("stopped");
  expect(jobItemGroup({ candidateId: "cancel", phase: "cancelled", outcome: "partial" })).toBe("stopped");
  expect(jobItemGroup({ candidateId: "gone", phase: "tombstoned", outcome: "tombstoned" })).toBe("stopped");
  expect(jobItemGroup({ candidateId: "recover", phase: "recovery_needed", outcome: "complete" })).toBe("failed");
  expect(jobItemGroup({ candidateId: "conflict", phase: "conflict", outcome: "complete" })).toBe("kept");
  expect(jobItemGroup({ candidateId: "kept-partial", phase: "conflict", outcome: "partial" })).toBe("kept");
  expect(jobItemGroup({ candidateId: "dedup", phase: "committed", outcome: "deduplicated" })).toBe("complete");
});

test("global progress and retry availability do not depend on the visible page", () => {
  const items: JobItem[] = [{ candidateId: "first", phase: "committed", outcome: "complete" }];
  const job = { items, itemCount: 1000, progress: { done: 1000, retryable: 800 } };
  expect(jobProgress(job)).toEqual({ done: 1000, total: 1000 });
  expect(jobCanRetry(job)).toBe(true);
  expect(jobCanRetry({ ...job, progress: { done: 1000, retryable: 0 } })).toBe(false);
  expect(jobCanRetry({ items: [{ candidateId: "old", phase: "recovery_needed" }] })).toBe(true);
});

test("undo conflict reasons map only the Host's exact durable texts", () => {
  expect(undoConflictReasonKey("MAPPING_CONFLICT: continuation sessions still reference this import"))
    .toBe("sessionImport.continuationConflict");
  expect(undoConflictReasonKey("later revision kept")).toBe("sessionImport.undoReason.laterRevision");
  expect(undoConflictReasonKey("ownership receipt missing")).toBe("sessionImport.undoReason.ownership");
  expect(undoConflictReasonKey("previous revision is unknown")).toBe("sessionImport.undoReason.ownership");
  expect(undoConflictReasonKey("MAPPING_CONFLICT: workspace project mismatch")).toBeNull();
  expect(undoConflictReasonKey("later revision kept, and more")).toBeNull();
  expect(undoConflictReasonKey(null)).toBeNull();
});
