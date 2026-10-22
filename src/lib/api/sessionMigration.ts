/** Session / workspace import IPC. */

import { invoke } from "./host";
import type {
  AllowedSelection,
  SelectionSet,
} from "@/lib/sessionImport/selection";
import type { PendingDurablePlan } from "@/lib/sessionImport/durablePlan";

export * from "./sessionMigrationLocations";
export * from "./sessionMigrationGroups";
export * from "./sessionMigrationComparison";
export * from "./sessionMigrationComparisonField";
export * from "./sessionMigrationIdentityAssociation";
export * from "./sessionMigrationJobs";

export type ImportProvider = "codex" | "claude-code" | "cursor" | "generic-file";

export type SourceWorkspaceRoot = {
  rootId: string;
  displayName?: string | null;
  pathSyntax: "windows" | "posix";
  host: "local" | "wsl" | "ssh" | "unknown";
  originalPath: string;
  sourceUri?: string | null;
  hostIdentity?: string | null;
  unsupportedUri?: boolean;
  availability: "exists" | "missing" | "unresolved" | "offline";
  evidence: string[];
};

export type WorkspaceRootChoice = {
  rootId: string;
  targetProjectId: string | null;
  explicitTarget?: string | null;
  primary: boolean;
};

export type WorkspaceImportSource = { path: string; kind: "file" | "folder"; sourceId?: string | null };
export type WorkspaceImportPreview = {
  source: WorkspaceImportSource;
  workspace: SourceWorkspace;
  fingerprint: string;
  savedRevision: string | null;
};
export type WorkspaceImportPage = { items: SourceWorkspace[]; nextCursor: string | null };
export type SavedWorkspace = { workspace: SourceWorkspace; revision: string };

export type SourceWorkspace = {
  sourceWorkspaceId: string;
  sourceIdentity?: { sourceId: string; provider: ImportProvider; locator: string } | null;
  displayName: string;
  workspaceFile?: string | null;
  roots: SourceWorkspaceRoot[];
  defaultRootId?: string | null;
  evidence: string[];
  mappingChoices?: WorkspaceRootChoice[];
  sourceFingerprint?: string | null;
};

export type DiscoveredRoot = {
  provider: ImportProvider;
  kind: string;
  path: string;
  exists: boolean;
  readable: boolean;
};

export type SourceCandidate = {
  candidateId: string;
  provider: ImportProvider;
  nativeSessionId: string;
  title: string;
  cwd?: string | null;
  startedAt?: string | null;
  updatedAt?: string | null;
  locator: string;
  sourceRevision: string;
  archived?: boolean | null;
  completeness: string;
};

export type ScanState = {
  scanId: string;
  epoch: number;
  candidates: SourceCandidate[];
  extraCandidates?: SourceCandidate[];
  issues?: Array<{ provider: string; path: string; code: string; message: string }>;
  complete: boolean;
  discoveredCount: number;
  state: "queued" | "scanning" | "paused" | "cancelling" | "completed" | "cancelled" | "failed" | "expired";
  stateVersion: number;
  issueCount: number;
  extraCount: number;
  selection: CandidateSelection;
};

export type ScanSource = {
  provider: string; path: string; sourceId?: string; profile?: string; locationId?: string; host?: string;
};
export type CandidateSelection = {
  version: 1;
  selectionId: string;
  scanId: string;
  epoch: number;
  mode: "explicit" | "frozen-scan";
  frozenThrough?: number | null;
  selectedCount: number;
  stateVersion: number;
};

export type CandidateSelectionPage = {
  candidates: SourceCandidate[];
  nextCursor: string | null;
  selectedCount: number;
};

export type CandidateSelectionMembership = {
  selectionId: string;
  stateVersion: number;
  selectedIds: string[];
};

export type PrepareBatchState =
  | "queued"
  | "preparing"
  | "cancelling"
  | "completed"
  | "completed-with-issues"
  | "failed"
  | "cancelled"
  | "planned";

export type PrepareBatchStatus = {
  version: 1;
  batchId: string;
  scanId: string;
  epoch: number;
  selectionId: string;
  selectionStateVersion: number;
  state: PrepareBatchState;
  stateVersion: number;
  total: number;
  prepared: number;
  failed: number;
  skipped: number;
  active: number;
  cancelRequested: boolean;
  createdAt: string;
  updatedAt: string;
};

export type PrepareOverride = {
  candidateId: string;
  previewId: string;
  leafId: string;
  selection: AllowedSelection;
};

export type PrepareBatchItem = {
  candidateId: string;
  title: string;
  provider: string;
  sourceRevision: string;
  state: "queued" | "preparing" | "prepared" | "failed" | "skipped";
  attempt: number;
  previewId?: string | null;
  error?: { code: string; message: string; retry: string; scope: string } | null;
};

export type PrepareBatchItemPage = {
  items: PrepareBatchItem[];
  nextCursor: string | null;
};

export function sessionImportWorkspacePreview(source: WorkspaceImportSource) {
  return invoke<WorkspaceImportPreview>("session_import_workspace_preview", { source });
}

export function sessionImportWorkspaceSave(request: {
  source: WorkspaceImportSource;
  expectedFingerprint: string;
  expectedSavedRevision: string | null;
  choices: WorkspaceRootChoice[];
}) {
  return invoke<SourceWorkspace>("session_import_workspace_save", { request });
}

export function sessionImportWorkspaces(cursor: string | null = null) {
  return invoke<WorkspaceImportPage>("session_import_workspaces", { cursor, limit: 20 });
}

export function sessionImportWorkspaceGet(sourceWorkspaceId: string) {
  return invoke<SavedWorkspace>("session_import_workspace_get", { sourceWorkspaceId });
}

export function sessionImportWorkspaceApply(request: { sourceWorkspaceId: string; expectedRevision: string }) {
  return invoke<import("@/lib/multiRootWorkspace").WorkspaceRecord>("session_import_workspace_apply", { request });
}

export function sessionImportProjectCreate(requestId: string, targetPath: string) {
  return invoke<{ project: { id: string; name: string; path: string; trusted: boolean }; created: boolean }>(
    "session_import_project_create", { requestId, targetPath },
  );
}

export function sessionImportWorkspaceUpdate(request: {
  sourceWorkspaceId: string;
  expectedRevision: string;
  choices: WorkspaceRootChoice[];
  sourceId?: string | null;
}) {
  return invoke<SavedWorkspace>("session_import_workspace_update", { request });
}

export type JobItem = {
  candidateId: string;
  title?: string | null;
  phase: string;
  importId?: string | null;
  revisionId?: string | null;
  outcome?: string | null;
  error?: string | null;
  mappingProjectId?: string | null;
};

export type WorkspaceMapping = {
  rootId: string;
  source?: { sourceId: string; provider: string } | null;
  targetProjectId?: string | null;
  explicitTarget?: string | null;
  unassigned: boolean;
  userChoiceVersion: number;
  primary?: boolean;
  suggestedTarget?: string | null;
  createProject?: boolean;
  importUnassignedAnyway?: boolean;
};

export type JobRecord = {
  jobId: string;
  requestId: string;
  planHash: string;
  state: string;
  stateVersion: number;
  items: JobItem[];
  createdAt: string;
  itemCount?: number;
  nextItemCursor?: string | null;
  progress?: { done: number; retryable: number } | null;
};

export type CatalogEntry = {
  importId: string;
  installationId: string;
  provider: string;
  nativeSessionId: string;
  title: string;
  alias?: string | null;
  hidden: boolean;
  currentRevision?: string | null;
  createdAt: string;
};

export type ImportedRevisionSummary = {
  revisionId: string;
  createdAt: string;
  outcome: "complete" | "partial" | "unsupported" | "failed";
};

export type ImportedRevisionPage = {
  importId: string;
  currentRevision: string;
  snapshot: string;
  items: ImportedRevisionSummary[];
  previousCursor: string | null;
  nextCursor: string | null;
  total: number;
};

export function sessionImportRevisions(importId: string, cursor: string | null = null, limit = 50) {
  return invoke<ImportedRevisionPage>("session_import_revisions", { importId, cursor, limit });
}

export type {
  AllowedSelection,
  PreviewSelection,
  SelectionSet,
} from "@/lib/sessionImport/selection";

export type HandoffDraft = {
  draftId: string;
  importId: string;
  revisionId: string;
  leaf: string;
  text: string;
  coverage: string;
  estimatedTokens: number;
  fragmentEventIds?: string[];
  readableEventIds?: string[];
  runtimeCapability: string;
  projectId?: string | null;
  draftHash: string;
  creationVersion?: number;
  workspaceProjectConstraint?: string | null;
  workspace?: import("@/lib/multiRootWorkspace").WorkspaceRecord | null;
  creationRequestId?: string | null;
  selection?: AllowedSelection;
};

export async function sessionImportListRoots() {
  return invoke<DiscoveredRoot[]>("session_import_list_roots");
}

export async function sessionImportScanStart(
  sources?: ScanSource[],
) {
  return invoke<ScanState>("session_import_scan_start", {
    req: { sources: sources ?? null },
  });
}

export async function sessionImportScanSelectCurrent(
  scanId: string,
  epoch: number,
) {
  return invoke<string[]>("session_import_scan_select_current", {
    scanId,
    epoch,
  });
}

export function sessionImportScanSelectionGet(scanId: string, epoch: number) {
  return invoke<CandidateSelection>("session_import_scan_selection_get", { scanId, epoch });
}

export function sessionImportScanSelectionFreeze(scanId: string, epoch: number) {
  return invoke<CandidateSelection>("session_import_scan_selection_freeze", { scanId, epoch });
}

export function sessionImportScanSelectionClear(
  scanId: string,
  epoch: number,
  selectionId: string,
) {
  return invoke<CandidateSelection>("session_import_scan_selection_clear", {
    scanId,
    epoch,
    selectionId,
  });
}

export function sessionImportScanSelectionSet(
  scanId: string,
  epoch: number,
  selectionId: string,
  candidateIds: string[],
  selected: boolean,
) {
  return invoke<CandidateSelection>("session_import_scan_selection_set", {
    scanId,
    epoch,
    selectionId,
    candidateIds,
    selected,
  });
}

export function sessionImportScanSelectionInvert(
  scanId: string,
  epoch: number,
  selectionId: string,
  candidateIds: string[],
) {
  return invoke<CandidateSelection>("session_import_scan_selection_invert", {
    scanId,
    epoch,
    selectionId,
    candidateIds,
  });
}

export function sessionImportScanSelectionPage(
  scanId: string,
  epoch: number,
  selectionId: string,
  cursor?: string | null,
  limit?: number,
) {
  return invoke<CandidateSelectionPage>("session_import_scan_selection_page", {
    scanId,
    epoch,
    selectionId,
    cursor: cursor ?? null,
    limit: limit ?? null,
  });
}

export function sessionImportScanSelectionMembership(
  scanId: string,
  epoch: number,
  selectionId: string,
  candidateIds: string[],
) {
  return invoke<CandidateSelectionMembership>("session_import_scan_selection_membership", {
    scanId,
    epoch,
    selectionId,
    candidateIds,
  });
}

export async function sessionImportScanRefresh(scanId: string, epoch: number) {
  return invoke<ScanState>("session_import_scan_refresh", { scanId, epoch });
}

export async function sessionImportScanPump(scanId: string, epoch: number) {
  return invoke<ScanState>("session_import_scan_pump", { scanId, epoch });
}

export function sessionImportScanGet(scanId: string, epoch: number) {
  return invoke<ScanState>("session_import_scan_get", { scanId, epoch });
}

export function sessionImportScanCancel(scanId: string, epoch: number) {
  return invoke<ScanState>("session_import_scan_cancel", { scanId, epoch });
}

export function sessionImportScanContinue(scanId: string, epoch: number) {
  return invoke<ScanState>("session_import_scan_continue", { scanId, epoch });
}

export type JobActionRequest = { expectedStateVersion: number; requestId: string };

export async function sessionImportRetryFailed(jobId: string, request: JobActionRequest) {
  return invoke<JobRecord>("session_import_retry_failed", { jobId, ...request });
}

export async function sessionImportScanPage(
  scanId: string,
  epoch: number,
  cursor?: string | null,
  limit?: number,
) {
  return invoke<{
    candidates: SourceCandidate[];
    nextCursor: string | null;
    discoveredCount: number;
    complete: boolean;
  }>("session_import_scan_page", {
    scanId,
    epoch,
    cursor: cursor ?? null,
    limit: limit ?? null,
  });
}

export async function sessionImportPreview(
  candidateId: string,
  expectedSourceRevision?: string | null,
) {
  return invoke<{ previewId: string; adapter: unknown }>(
    "session_import_preview",
    {
      candidateId,
      expectedSourceRevision: expectedSourceRevision ?? null,
    },
  );
}

export async function sessionImportPreviewEvents(
  previewId: string,
  cursor?: string | number | null,
  limit?: number,
  leaf?: string | null,
) {
  return invoke<{
    events: Array<{
      eventId: string;
      kind: string;
      role?: string | null;
      timestamp?: string | null;
      text: string;
      truncated: boolean;
      selectable?: boolean;
      defaultSelected?: boolean;
      subagent?: boolean;
      fieldAvailability?: Record<string, boolean>;
      assetIds?: string[];
      displayParts?: unknown[];
      displayLimited?: boolean;
      citation?: string;
    }>;
    nextCursor: string | null;
  }>("session_import_preview_events", {
    previewId,
    cursor: cursor == null ? null : String(cursor),
    limit: limit ?? null,
    leaf: leaf ?? null,
  });
}

export function sessionImportPrepareStart(
  scanId: string,
  epoch: number,
  selectionId: string,
  overrides: PrepareOverride[],
) {
  return invoke<PrepareBatchStatus>("session_import_prepare_start", {
    scanId,
    epoch,
    selectionId,
    overrides,
  });
}

export function sessionImportPrepareGet(batchId: string) {
  return invoke<PrepareBatchStatus>("session_import_prepare_get", { batchId });
}

export function sessionImportPrepareItems(
  batchId: string,
  cursor?: string | null,
  limit?: number,
) {
  return invoke<PrepareBatchItemPage>("session_import_prepare_items", {
    batchId,
    cursor: cursor ?? null,
    limit: limit ?? null,
  });
}

export function sessionImportPrepareCancel(batchId: string) {
  return invoke<PrepareBatchStatus>("session_import_prepare_cancel", { batchId });
}

export function sessionImportPrepareRetry(batchId: string) {
  return invoke<PrepareBatchStatus>("session_import_prepare_retry", { batchId });
}

export function sessionImportPrepareMappings(
  batchId: string,
  projectId?: string | null,
) {
  return invoke<WorkspaceMapping[]>("session_import_prepare_mappings", {
    batchId,
    projectId: projectId ?? null,
  });
}

export function sessionImportPreparePlan(
  batchId: string,
  mappings: WorkspaceMapping[],
) {
  return invoke<PendingDurablePlan>("session_import_prepare_plan", { batchId, mappings });
}

export async function sessionImportPlan(
  previewIds: string[],
  mappings: WorkspaceMapping[],
  selectionSet: SelectionSet,
) {
  return invoke<PendingDurablePlan>("session_import_plan", {
    previewIds,
    mappings,
    selectionSet,
  });
}

export async function sessionImportCommit(
  planId: string,
  planHash: string,
  requestId: string,
  explicitReimport?: boolean,
) {
  return invoke<JobRecord>("session_import_commit", {
    planId,
    planHash,
    requestId,
    explicitReimport: explicitReimport ?? false,
  });
}

export async function sessionImportJobGet(jobId: string) {
  return invoke<JobRecord>("session_import_job_get", { jobId });
}

export async function sessionImportJobItems(
  jobId: string,
  cursor?: string | null,
  limit?: number,
) {
  return invoke<JobRecord>("session_import_job_items", {
    jobId,
    cursor: cursor ?? null,
    limit: limit ?? null,
  });
}

export async function sessionImportCancel(jobId: string, request: JobActionRequest) {
  return invoke<JobRecord>("session_import_cancel", { jobId, ...request });
}

export async function sessionImportUndo(jobId: string, request: JobActionRequest) {
  return invoke<JobRecord>("session_import_undo", { jobId, ...request });
}

export async function sessionImportDelete(importId: string) {
  return invoke<void>("session_import_delete", { importId });
}

export async function sessionImportProposeMappings(
  previewIds: string[],
  fallbackProject?: string | null,
) {
  return invoke<WorkspaceMapping[]>("session_import_propose_mappings", {
    previewIds,
    fallbackProject: fallbackProject ?? null,
  });
}

export type CatalogPage = {
  items: CatalogEntry[];
  nextCursor: string | null;
  total: number;
  providers?: string[];
};

export type CatalogQuery = {
  provider?: string | null;
  query?: string | null;
  sort?: "newest" | "oldest";
  workspace?: import("./sessionWorkspaceAssociation").ImportedWorkspaceFilter | null;
};

/** Exact metadata lookup, independent of the paged/filtered catalog window. */
export async function sessionImportEntry(importId: string): Promise<CatalogEntry | null> {
  return invoke<CatalogEntry | null>("session_import_entry", { importId });
}

export async function sessionImportEntries(
  cursor?: string | null,
  limit?: number,
  filter?: CatalogQuery,
): Promise<CatalogPage> {
  const raw = await invoke<
    | {
        items?: CatalogEntry[];
        entries?: CatalogEntry[];
        nextCursor?: string | null;
        total?: number;
        providers?: string[];
      }
    | CatalogEntry[]
  >("session_import_entries", {
    cursor: cursor ?? null,
    limit: limit ?? null,
    provider: filter?.provider ?? null,
    query: filter?.query ?? null,
    sort: filter?.sort ?? null,
    workspace: filter?.workspace ?? null,
  });
  if (Array.isArray(raw)) {
    return { items: raw, nextCursor: null, total: raw.length };
  }
  const items = raw.items ?? raw.entries ?? [];
  return {
    items,
    nextCursor: raw.nextCursor ?? null,
    total: raw.total ?? items.length,
    providers: raw.providers,
  };
}

export type RecoverySnapshot = {
  plan: PendingDurablePlan | null;
  job: JobRecord | null;
  prepare: PrepareBatchStatus | null;
  draft: HandoffDraft | null;
  expired: boolean;
  failed: string | null;
};

export async function sessionImportRecovery(): Promise<RecoverySnapshot> {
  const raw = await invoke<RecoverySnapshot>("session_import_recovery");
  return {
    plan: raw.plan ?? null,
    job: raw.job ?? null,
    prepare: raw.prepare ?? null,
    draft: raw.draft ?? null,
    expired: !!raw.expired,
    failed: raw.failed ?? null,
  };
}

export type CatalogRebuildPhase =
  | "idle"
  | "checking"
  | "backingUp"
  | "rebuilding"
  | "verifying"
  | "committing"
  | "complete"
  | "failed"
  | "cancelled";

export type CatalogRebuildReport = {
  fromVersion: number;
  toVersion: number;
  revisionsScanned: number;
  revisionsRebuilt: number;
  chunksWritten: number;
  searchableBytes: number;
  projectionSha256: string;
};

export type CatalogRebuildStatus = {
  running: boolean;
  phase: CatalogRebuildPhase;
  completedRevisions: number;
  totalRevisions: number;
  report: CatalogRebuildReport | null;
  errorCode: string | null;
};

export function sessionImportCatalogRebuild(): Promise<CatalogRebuildReport> {
  return invoke<CatalogRebuildReport>("session_import_catalog_rebuild");
}

export async function sessionImportCatalogRebuildStatus(): Promise<CatalogRebuildStatus> {
  const status = await invoke<CatalogRebuildStatus>(
    "session_import_catalog_rebuild_status",
  );
  return {
    ...status,
    report: status.report ?? null,
    errorCode: status.errorCode ?? null,
  };
}

export function sessionImportCatalogRebuildCancel(): Promise<boolean> {
  return invoke<boolean>("session_import_catalog_rebuild_cancel");
}

export type ImportedRevisionHealth = {
  outcome: "complete" | "partial" | "unsupported" | "failed" | "unknown";
  issueCount: number | null;
  issues: Array<{ code: string; message: string }>;
  diagnosticsLimited: boolean;
};

export async function sessionImportEvents(
  importId: string,
  revision?: string | null,
  cursor?: string | null,
  limit?: number,
  leaf?: string | null,
  direction?: "forward" | "backward",
) {
  return invoke<{
    events: unknown[];
    health?: ImportedRevisionHealth;
    previousCursor: string | null;
    nextCursor: string | null;
    total: number;
    start: number;
    end: number;
    revision: string;
    leaf: string;
  }>(
    "session_import_events",
    {
      importId,
      revision: revision ?? null,
      cursor: cursor ?? null,
      limit: limit ?? null,
      leaf: leaf ?? null,
      direction: direction ?? null,
    },
  );
}

export type ImportedEventContext = {
  events: unknown[];
  health?: ImportedRevisionHealth;
  targetIndex: number;
  ordinal: number;
  position: number;
  total: number;
  revision: string;
  leaf: string;
  anchorCursor: string;
  previousCursor: string | null;
  nextCursor: string | null;
};

export async function sessionImportEventContext(
  importId: string,
  eventId: string,
  revision?: string | null,
  leaf?: string | null,
  opts?: {
    before?: number;
    after?: number;
    cursor?: string | null;
  },
) {
  return invoke<ImportedEventContext>("session_import_event_context", {
    importId,
    revision: revision ?? null,
    leaf: leaf ?? null,
    eventId,
    before: opts?.before ?? 8,
    after: opts?.after ?? 8,
    cursor: opts?.cursor ?? null,
  });
}

export async function sessionImportSearch(
  importId: string,
  query: string,
  revision?: string | null,
  cursor?: string | null,
  leaf?: string | null,
  direction?: "forward" | "backward",
) {
  return invoke<{
    hits: Array<{ eventId: string; ordinal: number; snippet: string; kind: string }>;
    previousCursor: string | null;
    nextCursor: string | null;
  }>(
    "session_import_search",
    {
      importId,
      revision: revision ?? null,
      leaf: leaf ?? null,
      query,
      cursor: cursor ?? null,
      limit: 20,
      direction: direction ?? null,
    },
  );
}

export async function sessionImportExportMarkdown(
  importId: string,
  revision?: string | null,
  leaf?: string | null,
) {
  return invoke<string>("session_import_export_markdown", {
    importId,
    revision: revision ?? null,
    leaf: leaf ?? null,
  });
}

export async function sessionImportContinuePrepare(
  importId: string,
  opts?: {
    revision?: string | null;
    leaf?: string | null;
    projectId?: string | null;
    selection?: AllowedSelection | null;
  },
) {
  return invoke<HandoffDraft>("session_import_continue_prepare", {
    importId,
    revision: opts?.revision ?? null,
    leaf: opts?.leaf ?? null,
    projectId: opts?.projectId ?? null,
    capability: null,
    selection: opts?.selection ?? null,
  });
}

export function sessionImportContinueSelection(draftId: string, draftHash: string, selection: AllowedSelection) {
  return invoke<HandoffDraft>("session_import_continue_selection", { draftId, draftHash, selection });
}

export function sessionImportContinueWorkspace(draftId: string, draftHash: string, workspaceId: string | null) {
  return invoke<HandoffDraft>("session_import_continue_workspace", { draftId, draftHash, workspaceId });
}

export function sessionImportContinueRestore(draftId: string, draftHash: string) {
  return invoke<HandoffDraft>("session_import_continue_restore", { draftId, draftHash });
}

export function sessionImportContinueDiscard(draftId: string, draftHash: string) {
  return invoke<void>("session_import_continue_discard", { draftId, draftHash });
}

export async function sessionImportContinueCreate(
  draftId: string,
  draftHash: string,
  requestId: string,
  editedText?: string | null,
) {
  return invoke<{ appSessionId: string }>("session_import_continue_create", {
    draftId,
    draftHash,
    requestId,
    editedText: editedText ?? null,
  });
}

export type ContinuationDraftView = {
  appSessionId: string;
  importId: string;
  revisionId: string;
  leaf: string;
  state: "prepared" | "failed-before-dispatch" | "dispatching" | "running" | "completed" | "unknown";
  text: string | null;
  textHash: string | null;
};

export function sessionImportContinuationGet(appSessionId: string) {
  return invoke<ContinuationDraftView | null>("session_import_continuation_get", { appSessionId });
}

export function sessionImportContinuationSave(appSessionId: string, expectedTextHash: string, text: string) {
  return invoke<ContinuationDraftView>("session_import_continuation_save", { appSessionId, expectedTextHash, text });
}

export function isImportedHistoryId(id: string | null | undefined): boolean {
  if (!id) return false;
  return /^imp:[0-9a-f-]{36}$/i.test(id.trim());
}
