import type {
  CatalogEntry, ContinuationDraftView, DiscoveredRoot, JobRecord,
  PrepareBatchItem, PrepareBatchStatus, SourceCandidate, SourceRegistration,
} from "@/lib/api/sessionMigration";
import type { SessionImportPreviewClient } from "@/hooks/useSessionImportContentStates";

export const SYNTHETIC_ROOT = "C:\\synthetic-import-review\\source-with-a-very-long-folder-name\\archived-sessions";
export const SYNTHETIC_PROJECT = "C:\\synthetic-import-review\\workspace-with-a-very-long-folder-name\\project-🙂-锚点";
export const LONG_PATH = `${SYNTHETIC_ROOT}\\session-🙂-锚点\\chat.json`;

export const candidate: SourceCandidate = {
  candidateId: "synthetic-1",
  provider: "codex",
  nativeSessionId: "synthetic-native",
  title: "Synthetic session 🙂 锚点",
  cwd: SYNTHETIC_PROJECT,
  locator: LONG_PATH,
  sourceRevision: "rev-synthetic",
  completeness: "body",
  updatedAt: "2026-10-03T00:00:00Z",
};

export const root: DiscoveredRoot = {
  provider: "codex",
  kind: "home",
  path: SYNTHETIC_ROOT,
  exists: true,
  readable: true,
};

export const registration: SourceRegistration = {
  sourceId: "src-synthetic",
  provider: "codex",
  rootPath: SYNTHETIC_ROOT,
  profile: "synthetic",
};

export const prepareStatus: PrepareBatchStatus = {
  version: 1,
  batchId: "batch-synthetic",
  scanId: "scan-synthetic",
  epoch: 1,
  selectionId: "sel-synthetic",
  selectionStateVersion: 1,
  state: "completed-with-issues",
  stateVersion: 2,
  total: 2,
  prepared: 1,
  failed: 1,
  skipped: 0,
  active: 0,
  cancelRequested: false,
  createdAt: "2026-10-03T00:00:00Z",
  updatedAt: "2026-10-03T00:01:00Z",
};

export const prepareItems: PrepareBatchItem[] = [
  {
    candidateId: "synthetic-1",
    title: candidate.title,
    provider: "codex",
    sourceRevision: "rev-synthetic",
    state: "prepared",
    attempt: 1,
  },
  {
    candidateId: "synthetic-2",
    title: "Unreadable archive",
    provider: "codex",
    sourceRevision: "rev-missing",
    state: "failed",
    attempt: 1,
    error: { code: "SOURCE_BUSY", message: "locked", retry: "manual", scope: "file" },
  },
];

export function jobFor(surface: string): JobRecord {
  return {
    jobId: "job-synthetic",
    requestId: "req-synthetic",
    planHash: "plan-synthetic",
    state: surface === "import" ? "running" : "completed_with_issues",
    stateVersion: 3,
    createdAt: "2026-10-03T00:00:00Z",
    itemCount: 2,
    progress: { done: surface === "import" ? 1 : 2, retryable: 1 },
    items: [
      {
        candidateId: "synthetic-1",
        title: candidate.title,
        phase: surface === "import" ? "running" : "committed",
        importId: "imp:00000000-0000-4000-8000-000000000001",
        outcome: surface === "import" ? null : "complete",
      },
      {
        candidateId: "synthetic-2",
        title: "Unreadable archive",
        phase: "failed",
        error: "SOURCE_BUSY: locked",
      },
    ],
  };
}

export const catalogEntry: CatalogEntry = {
  importId: "imp:00000000-0000-4000-8000-000000000001",
  installationId: "synthetic-installation",
  provider: "codex",
  nativeSessionId: "synthetic-native",
  title: "Synthetic session 🙂 锚点",
  alias: null,
  hidden: false,
  currentRevision: "rev-synthetic",
  createdAt: "2026-10-03T00:00:00Z",
};

export const reviewCatalogEntries: CatalogEntry[] = [catalogEntry, {
  ...catalogEntry, importId: "imp:00000000-0000-4000-8000-000000000002",
  provider: "claude-code", nativeSessionId: "synthetic-native-2",
  title: "Synthetic archived session", createdAt: "2026-10-02T00:00:00Z",
}];

export const draft: ContinuationDraftView = {
  appSessionId: "synthetic-session",
  importId: catalogEntry.importId,
  revisionId: "rev-synthetic",
  leaf: "leaf-synthetic",
  state: "prepared",
  text: "Synthetic continuation draft. Not sent. 🙂 锚点",
  textHash: "hash-synthetic",
};

export const reviewCandidates: SourceCandidate[] = [candidate, {
  ...candidate, candidateId: "synthetic-2", nativeSessionId: "synthetic-native-2",
  provider: "claude-code", title: "Synthetic archived session", archived: true,
  completeness: "prefix", sourceRevision: "rev-synthetic-2",
}];

export const reviewPreviewClient: SessionImportPreviewClient = {
  async preview(id, revision) {
    const item = reviewCandidates.find((row) => row.candidateId === id);
    if (!item || (revision && revision !== item.sourceRevision)) {
      throw new Error("PREVIEW_EXPIRED: synthetic owner");
    }
    return {
      previewId: `preview-${id}`,
      adapter: {
        outcome: "complete", eventCount: 3, activeLeaf: "leaf-a",
        leaves: ["leaf-a", "leaf-b"],
        inventory: { imported: { messages: 3 }, sourceTokens: 64 },
      },
    };
  },
  async events(previewId, cursor, _limit, leaf) {
    if (!reviewCandidates.some((row) => previewId === `preview-${row.candidateId}`) ||
        (leaf && !["leaf-a", "leaf-b"].includes(leaf)) ||
        (cursor != null && cursor !== "page-2")) {
      throw new Error("PREVIEW_EXPIRED: synthetic scope");
    }
    const events = [
      { eventId: "note-1", role: "user", text: "Synthetic user note 🙂 锚点" },
      { eventId: "note-2", role: "assistant", text: `Synthetic **assistant** note · ${leaf ?? "leaf-a"}` },
      { eventId: "note-3", role: "user", text: "Synthetic follow-up. Not sent." },
    ].map((event) => ({ ...event, kind: "message", selectable: true, defaultSelected: true, truncated: false }));
    return { events: cursor ? events.slice(2) : events.slice(0, 2), nextCursor: cursor ? null : "page-2" };
  },
};
