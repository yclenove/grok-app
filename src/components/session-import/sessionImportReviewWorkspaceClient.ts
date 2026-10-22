import type { WorkspaceImportClient } from "@/hooks/useWorkspaceImport";
import type { WorkspaceRelocationClient } from "@/hooks/useWorkspaceRelocation";
import type { SavedWorkspace, SourceWorkspace, WorkspaceImportSource, WorkspaceRootChoice } from "@/lib/api/sessionMigration";
import type { WorkspaceRelocationInput, WorkspaceRelocationPreview, WorkspaceRelocationSave } from "@/lib/api/workspaceRelocation";
import type { WorkspaceRecord } from "@/lib/multiRootWorkspace";
import { registration, SYNTHETIC_PROJECT } from "./sessionImportReviewFixtures";

export const REVIEW_WORKSPACE_FILE = `${SYNTHETIC_PROJECT}\\review.code-workspace`;
export const REVIEW_RELOCATED_FOLDER = "C:\\synthetic-import-review\\relocated-workspace-🙂-锚点";
export const reviewWorkspaceProjects = [{ id: "proj-synthetic", name: "Synthetic project", path: SYNTHETIC_PROJECT }];

const copy = <T,>(value: T): T => structuredClone(value);

/** Versioned in-memory adapter for the production workspace controllers. Every
 * picker and persistence method is local; no IPC, storage or source reads. */
export function createReviewWorkspaceClient(options: { conflictOnce?: boolean; relocationRetry?: boolean } = {}) {
  let version = 0;
  const hash = () => (++version).toString(16).padStart(64, "0");
  const fingerprint = hash();
  const records = new Map<string, SavedWorkspace>();
  const projects = new Map(reviewWorkspaceProjects.map((project) => [project.id, { ...project, trusted: false }]));
  const projectRequests = new Map<string, string>();
  const proposals = new Map<string, WorkspaceRelocationPreview>();
  const receipts = new Map<string, { request: WorkspaceRelocationSave; result: SavedWorkspace }>();
  const drafts = new Map<string, WorkspaceRelocationSave>();
  const nativeRecords = new Map<string, WorkspaceRecord>();
  let conflict = !!options.conflictOnce;
  let relocationRetry = !!options.relocationRetry;

  const identity = (source: WorkspaceImportSource) => {
    if (source.sourceId && source.sourceId !== registration.sourceId) throw new Error("SOURCE_NOT_FOUND: synthetic installation");
    return source.sourceId ? { sourceId: source.sourceId, provider: registration.provider, locator: source.path } : null;
  };
  const makeWorkspace = (source: WorkspaceImportSource): SourceWorkspace => ({
    sourceWorkspaceId: `ws:synthetic-${source.kind}-${source.sourceId ?? "unknown"}`,
    displayName: source.kind === "file" ? "Synthetic file workspace 🙂 锚点" : "Synthetic folder workspace 🙂 锚点",
    sourceIdentity: identity(source), workspaceFile: source.kind === "file" ? source.path : null,
    defaultRootId: "root-app", evidence: ["synthetic review"], sourceFingerprint: fingerprint,
    roots: source.kind === "folder" ? [
      { rootId: "root-app", displayName: "Application", originalPath: source.path,
        pathSyntax: "windows", host: "local", availability: "exists", evidence: [] },
    ] : [
      { rootId: "root-app", displayName: "Application", originalPath: `${SYNTHETIC_PROJECT}\\app`,
        pathSyntax: "windows", host: "local", availability: "exists", evidence: [] },
      { rootId: "root-docs", displayName: "Documentation", originalPath: `${SYNTHETIC_PROJECT}\\docs`,
        pathSyntax: "windows", host: "local", availability: "missing", evidence: [] },
      { rootId: "root-remote", displayName: "Remote archive", originalPath: "/archived/repo",
        sourceUri: "vscode-remote://ssh-remote+synthetic/archived/repo", hostIdentity: "ssh-remote+synthetic",
        pathSyntax: "posix", host: "ssh", availability: "offline", evidence: [] },
    ],
  });
  for (const kind of ["file", "folder"] as const) {
    const workspace = makeWorkspace({ kind, path: kind === "file" ? REVIEW_WORKSPACE_FILE : SYNTHETIC_PROJECT });
    workspace.mappingChoices = workspace.roots.map((root, index) => ({
      rootId: root.rootId, targetProjectId: index === 0 ? "proj-synthetic" : null,
      ...(index ? { explicitTarget: `${SYNTHETIC_PROJECT}\\mapped-${index}` } : {}), primary: index === 0,
    }));
    records.set(workspace.sourceWorkspaceId, { workspace, revision: hash() });
  }
  const get = (id: string) => {
    const saved = records.get(id);
    if (!saved) throw new Error("SOURCE_NOT_FOUND: synthetic workspace");
    return saved;
  };
  const checkRevision = (id: string, expected: string) => {
    const saved = get(id);
    if (saved.revision !== expected) throw new Error("PLAN_STALE: synthetic workspace revision");
    return saved;
  };
  const mapped = (workspace: SourceWorkspace, choices: WorkspaceRootChoice[]): SourceWorkspace => {
    if (choices.length !== workspace.roots.length || new Set(choices.map((choice) => choice.rootId)).size !== choices.length
      || choices.some((choice) => !workspace.roots.some((root) => root.rootId === choice.rootId))
      || choices.filter((choice) => choice.primary).length !== (workspace.roots.length ? 1 : 0)) {
      throw new Error("MAPPING_CONFLICT: synthetic roots");
    }
    return { ...workspace, mappingChoices: copy(choices), defaultRootId: choices.find((choice) => choice.primary)?.rootId };
  };
  const store = (workspace: SourceWorkspace) => {
    const saved = { workspace: copy(workspace), revision: hash() };
    records.set(workspace.sourceWorkspaceId, saved);
    return copy(saved);
  };

  const client: WorkspaceImportClient = {
    async pickDirectory() { return `${SYNTHETIC_PROJECT}\\chosen-target`; },
    async sessionImportWorkspacePreview(source) {
      const workspace = makeWorkspace(source);
      const saved = records.get(workspace.sourceWorkspaceId);
      if (saved) {
        workspace.mappingChoices = copy(saved.workspace.mappingChoices);
        workspace.defaultRootId = saved.workspace.defaultRootId;
      }
      return { source: copy(source), workspace, fingerprint, savedRevision: saved?.revision ?? null };
    },
    async sessionImportWorkspaceSave(request) {
      const workspace = makeWorkspace(request.source);
      if (request.expectedFingerprint !== fingerprint) throw new Error("SOURCE_CHANGED: synthetic fingerprint");
      if ((records.get(workspace.sourceWorkspaceId)?.revision ?? null) !== request.expectedSavedRevision) {
        throw new Error("PLAN_STALE: synthetic workspace revision");
      }
      return store(mapped(workspace, request.choices)).workspace;
    },
    async sessionImportWorkspaces(cursor) {
      const items = [...records.values()].map((saved) => saved.workspace);
      const index = cursor ? items.findIndex((item) => item.sourceWorkspaceId === cursor) + 1 : 0;
      if (cursor && !index) throw new Error("PLAN_STALE: synthetic page cursor");
      const page = items.slice(index, index + 1);
      return copy({ items: page, nextCursor: index + page.length < items.length ? page[0].sourceWorkspaceId : null });
    },
    async sessionImportWorkspaceGet(id) { return copy(get(id)); },
    async sessionImportWorkspaceUpdate(request) {
      const saved = checkRevision(request.sourceWorkspaceId, request.expectedRevision);
      if (conflict) {
        conflict = false; store(saved.workspace);
        throw new Error("PLAN_STALE: synthetic concurrent edit");
      }
      const workspace = mapped(saved.workspace, request.choices);
      if (request.sourceId && !workspace.sourceIdentity) workspace.sourceIdentity = identity({ kind: "file",
        path: workspace.workspaceFile ?? workspace.roots[0].originalPath, sourceId: request.sourceId });
      return store(workspace);
    },
    async sessionImportProjectCreate(requestId, targetPath) {
      const previous = projectRequests.get(requestId);
      if (previous && previous !== targetPath) throw new Error("MAPPING_CONFLICT: synthetic project request");
      projectRequests.set(requestId, targetPath);
      const existing = [...projects.values()].find((project) => project.path === targetPath);
      if (existing) return copy({ project: existing, created: false });
      const project = { id: `proj-synthetic-${projects.size}`, name: "Synthetic created project", path: targetPath, trusted: false };
      projects.set(project.id, project);
      return copy({ project, created: true });
    },
    async sessionImportWorkspaceApply(request) {
      const { workspace } = checkRevision(request.sourceWorkspaceId, request.expectedRevision);
      const choices = workspace.mappingChoices ?? [];
      const primary = choices.find((choice) => choice.primary);
      if (!primary?.targetProjectId || !projects.has(primary.targetProjectId) || choices.length !== workspace.roots.length
        || choices.some((choice) => !choice.explicitTarget && !projects.has(choice.targetProjectId ?? ""))) {
        throw new Error("WORKSPACE_UNRESOLVED: synthetic root mappings");
      }
      const key = JSON.stringify([workspace.sourceWorkspaceId, request.expectedRevision]);
      const existing = nativeRecords.get(key);
      if (existing) return copy(existing);
      const native: WorkspaceRecord = {
        id: `native:${workspace.sourceWorkspaceId}`, name: workspace.displayName, primaryProjectId: primary.targetProjectId,
        roots: choices.map((choice) => ({ path: projects.get(choice.targetProjectId ?? "")?.path ?? choice.explicitTarget!,
          role: choice.primary ? "primary" : "extra", access: choice.primary ? "write" : "read", pathOk: true })),
        capability: "contextOnly", updatedAt: "2026-10-03T00:00:00Z",
      };
      nativeRecords.set(key, native);
      return copy(native);
    },
  };
  const relocate = (request: WorkspaceRelocationInput): WorkspaceRelocationPreview => {
    const { workspace: before } = checkRevision(request.sourceWorkspaceId, request.expectedRevision);
    const retainedRootIds = before.roots.filter((root) => root.rootId !== "root-remote").map((root) => root.rootId);
    const addedRootIds = before.workspaceFile ? ["root-tests"] : [];
    const after: SourceWorkspace = { ...copy(before), sourceFingerprint: fingerprint,
      workspaceFile: before.workspaceFile ? request.path : null,
      roots: [
        ...before.roots.filter((root) => retainedRootIds.includes(root.rootId)).map((root) => ({ ...root,
          originalPath: before.workspaceFile ? `${REVIEW_RELOCATED_FOLDER}\\${root.rootId === "root-app" ? "app" : "docs"}` : request.path })),
        ...(before.workspaceFile ? [{ rootId: "root-tests", displayName: "Tests", originalPath: `${REVIEW_RELOCATED_FOLDER}\\tests`,
          pathSyntax: "windows" as const, host: "local" as const, availability: "exists" as const, evidence: [] }] : []),
      ], mappingChoices: [
        ...(before.mappingChoices ?? []).filter((choice) => retainedRootIds.includes(choice.rootId)),
        ...(before.workspaceFile ? [{ rootId: "root-tests", targetProjectId: null, primary: false }] : []),
      ],
    };
    const proposal = { request: copy(request), before: copy(before), after, retainedRootIds, addedRootIds,
      removedRoots: copy(before.roots.filter((root) => !retainedRootIds.includes(root.rootId))), fingerprint, proposalHash: hash() };
    proposals.set(proposal.proposalHash, copy(proposal));
    return proposal;
  };
  const relocation: WorkspaceRelocationClient = {
    async pickAttachFiles() { return [`${REVIEW_RELOCATED_FOLDER}\\review.code-workspace`]; },
    async pickDirectory() { return REVIEW_RELOCATED_FOLDER; },
    sessionImportWorkspaceGet: client.sessionImportWorkspaceGet,
    async workspaceRelocationPreview(request) { return copy(relocate(request)); },
    async workspaceRelocationSave(request) {
      const receipt = receipts.get(request.requestId);
      if (receipt) {
        if (JSON.stringify(receipt.request) !== JSON.stringify(request)) throw new Error("MAPPING_CONFLICT: synthetic request reuse");
        return copy(receipt.result);
      }
      const proposal = proposals.get(request.proposalHash);
      if (!proposal || request.fingerprint !== proposal.fingerprint || JSON.stringify(request.preview) !== JSON.stringify(proposal.request)) {
        throw new Error("PLAN_STALE: synthetic relocation proposal");
      }
      checkRevision(request.preview.sourceWorkspaceId, request.preview.expectedRevision);
      // Commit once, then simulate a lost response. A frozen retry returns the
      // original receipt even though the workspace revision has advanced.
      const result = store(proposal.after);
      receipts.set(request.requestId, copy({ request, result }));
      if (relocationRetry) { relocationRetry = false; throw new Error("SNAPSHOT_UNAVAILABLE: synthetic response lost"); }
      return copy(result);
    },
    readRelocationDraft(id) { return copy(drafts.get(id) ?? null); },
    writeRelocationDraft(request) { drafts.set(request.preview.sourceWorkspaceId, copy(request)); },
    clearRelocationDraft(id, requestId) { if (drafts.get(id)?.requestId === requestId) drafts.delete(id); },
  };
  return { client, relocation };
}
