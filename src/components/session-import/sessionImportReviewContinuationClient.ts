import type { ImportContinuationClient } from "@/hooks/useImportContinuation";
import type { CatalogEntry, ContinuationDraftView, HandoffDraft } from "@/lib/api/sessionMigration";
import { defaultSelection } from "@/lib/sessionImport/selection";
import type { WorkspaceRecord } from "@/lib/multiRootWorkspace";
import type { ContinuationModalClient } from "./SessionImportContinuationModal";
import { catalogEntry, draft, SYNTHETIC_PROJECT } from "./sessionImportReviewFixtures";
import { reviewReaderClient } from "./sessionImportReviewReaderClient";

const workspace: WorkspaceRecord = {
  id: "workspace-synthetic", name: "Synthetic execution workspace", primaryProjectId: "project-synthetic",
  capability: "contextOnly", updatedAt: "2026-10-03T00:00:00Z",
  roots: [{ path: SYNTHETIC_PROJECT, role: "primary", access: "write" },
    { path: `${SYNTHETIC_PROJECT}\\reference`, role: "extra", access: "read" }],
};
const workspaces: WorkspaceRecord[] = [workspace,
  { ...workspace, id: "workspace-blocked", name: "Synthetic blocked workspace", capability: "blocked" },
  { ...workspace, id: "workspace-other-project", name: "Synthetic other project", primaryProjectId: "project-other" }];

/** An isolated, versioned store. All methods are local; none can fall back to IPC. */
export function createReviewContinuationClient({ locked = false, failCreateOnce = false, entry = catalogEntry }: {
  locked?: boolean; failCreateOnce?: boolean; entry?: CatalogEntry;
} = {}) {
  let current: HandoffDraft | null = null;
  let version = 0;
  let failed = false;
  let created: ContinuationDraftView | null = null;
  const validate = (id: string, hash: string) => {
    if (!current || current.draftId !== id || current.draftHash !== hash) {
      throw new Error("PLAN_STALE: synthetic handoff ownership");
    }
    return current;
  };
  const replace = (next: HandoffDraft) => {
    current = { ...next, draftHash: `handoff-hash-synthetic-${++version}` };
    return structuredClone(current);
  };
  const continuation: ImportContinuationClient = {
    async sessionImportContinuePrepare(id, opts) {
      if (id !== entry.importId || (opts?.revision && opts.revision !== entry.currentRevision) ||
        (opts?.leaf && opts.leaf !== draft.leaf) || opts?.projectId) {
        throw new Error("REVISION_MISMATCH: synthetic handoff scope");
      }
      if (!current) replace({
        draftId: "handoff-synthetic", draftHash: "", importId: id, revisionId: entry.currentRevision ?? draft.revisionId,
        leaf: draft.leaf, text: draft.text ?? "", coverage: "full-selection", estimatedTokens: 64,
        runtimeCapability: "fragment-only", creationVersion: 1, workspaceProjectConstraint: null,
        projectId: null, workspace: null, creationRequestId: locked ? "synthetic-restored-request" : null,
        fragmentEventIds: ["reader-1", "reader-2"], readableEventIds: ["reader-1", "reader-2", "reader-3"],
        selection: { ...defaultSelection(), eventIds: ["reader-1", "reader-2", "reader-3"], pinnedEventIds: ["reader-1"] },
      });
      return structuredClone(current!);
    },
    async sessionImportContinueWorkspace(id, hash, workspaceId) {
      const owned = validate(id, hash);
      const chosen = workspaces.find((row) => row.id === workspaceId);
      if (owned.creationRequestId || (workspaceId && (!chosen || chosen.capability === "blocked"))) {
        throw new Error("MAPPING_CONFLICT: synthetic workspace");
      }
      return replace({ ...owned, workspace: chosen ?? null, projectId: chosen?.primaryProjectId ?? null });
    },
    async sessionImportContinueSelection(id, hash, selection) {
      const owned = validate(id, hash);
      if (owned.creationRequestId || selection.eventIds.some((eventId) => !owned.selection?.eventIds.includes(eventId)) ||
        selection.pinnedEventIds.some((eventId) => !selection.eventIds.includes(eventId))) {
        throw new Error("PLAN_STALE: synthetic scope expansion");
      }
      return replace({ ...owned, selection: structuredClone(selection), estimatedTokens: selection.eventIds.length * 16,
        fragmentEventIds: [...selection.eventIds], readableEventIds: [...selection.eventIds],
        text: `Synthetic handoff for ${selection.eventIds.join(", ")}. Not sent.` });
    },
    async sessionImportContinueRestore(id, hash) { return structuredClone(validate(id, hash)); },
    async sessionImportContinueDiscard(id, hash) { validate(id, hash); current = null; },
    async sessionImportContinueCreate(id, hash, requestId, editedText) {
      const owned = validate(id, hash);
      if (owned.creationRequestId && owned.creationRequestId !== requestId) {
        throw new Error("MAPPING_CONFLICT: synthetic creation request");
      }
      current = { ...owned, creationRequestId: requestId, text: editedText ?? owned.text };
      if (failCreateOnce && !failed) {
        failed = true;
        throw new Error("SNAPSHOT_UNAVAILABLE: synthetic creation interrupted");
      }
      created ??= { ...draft, importId: entry.importId, revisionId: owned.revisionId,
        text: current.text, textHash: `synthetic-created-${version}` };
      return { appSessionId: created.appSessionId };
    },
  };
  const modal: ContinuationModalClient = {
    async workspacesList() {
      return structuredClone(workspaces);
    },
    eventContext: reviewReaderClient("idle", entry).context,
  };
  return { continuation, modal, getCreated: () => created ? structuredClone(created) : null };
}
