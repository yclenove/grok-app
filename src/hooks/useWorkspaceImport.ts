import { useCallback, useEffect, useRef, useState } from "react";
import * as api from "@/lib/api/sessionMigration";
import { pickDirectory } from "@/lib/api/fs";

export type WorkspaceImportClient = Pick<typeof api,
  "sessionImportWorkspacePreview" | "sessionImportWorkspaceSave" | "sessionImportWorkspaces"
  | "sessionImportWorkspaceGet" | "sessionImportWorkspaceUpdate" | "sessionImportWorkspaceApply"
  | "sessionImportProjectCreate"> & { pickDirectory: typeof pickDirectory };

const workspaceImportClient: WorkspaceImportClient = {
  sessionImportWorkspacePreview: (source) => api.sessionImportWorkspacePreview(source),
  sessionImportWorkspaceSave: (request) => api.sessionImportWorkspaceSave(request),
  sessionImportWorkspaces: (cursor) => api.sessionImportWorkspaces(cursor),
  sessionImportWorkspaceGet: (id) => api.sessionImportWorkspaceGet(id),
  sessionImportWorkspaceUpdate: (request) => api.sessionImportWorkspaceUpdate(request),
  sessionImportWorkspaceApply: (request) => api.sessionImportWorkspaceApply(request),
  sessionImportProjectCreate: (id, path) => api.sessionImportProjectCreate(id, path),
  pickDirectory: () => pickDirectory(),
};

function rootChoices(workspace: api.SourceWorkspace): api.WorkspaceRootChoice[] {
  const saved = workspace.mappingChoices ?? [];
  const primary = saved.find((choice) => choice.primary)?.rootId
    ?? workspace.defaultRootId ?? workspace.roots[0]?.rootId;
  return workspace.roots.map((root) => {
    const previous = saved.find((choice) => choice.rootId === root.rootId);
    return {
      rootId: root.rootId, targetProjectId: previous?.targetProjectId ?? null,
      ...(previous?.explicitTarget ? { explicitTarget: previous.explicitTarget } : {}),
      primary: root.rootId === primary,
    };
  });
}

/** Workspace metadata is a different intent from transcript discovery. No scan
 * API is reachable here, including for an empty workspace or a missing root. */
export function useWorkspaceImport(onProjectsChanged?: () => void, client: WorkspaceImportClient = workspaceImportClient) {
  const generation = useRef(0);
  const pending = useRef(false);
  const projectRequests = useRef(new Map<string, string>());
  const [createdProjects, setCreatedProjects] = useState<Array<{ id: string; name: string }>>([]);
  const [projectNotice, setProjectNotice] = useState<{ rootId: string; name: string } | null>(null);
  const [mode, setMode] = useState<"preview" | "saved" | "edit" | "list" | null>(null);
  const [preview, setPreview] = useState<api.WorkspaceImportPreview | null>(null);
  const [workspace, setWorkspace] = useState<api.SourceWorkspace | null>(null);
  const [choices, setChoices] = useState<api.WorkspaceRootChoice[]>([]);
  const [items, setItems] = useState<api.SourceWorkspace[]>([]);
  const [cursor, setCursor] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [editBase, setEditBase] = useState<api.SourceWorkspace | null>(null);
  const [savedRevision, setSavedRevision] = useState<string | null>(null);
  const [sourceId, setSourceId] = useState<string | null>(null);
  const [nativeWorkspace, setNativeWorkspace] = useState<Awaited<ReturnType<typeof api.sessionImportWorkspaceApply>> | null>(null);

  const reset = useCallback(() => {
    generation.current += 1;
    pending.current = false;
    setMode(null);
    setPreview(null);
    setWorkspace(null);
    setChoices([]);
    setItems([]);
    setCursor(null);
    setBusy(false);
    setError(null);
    setEditBase(null);
    setSavedRevision(null);
    setSourceId(null);
    setNativeWorkspace(null);
    setProjectNotice(null);
  }, []);

  useEffect(() => () => { generation.current += 1; }, []);

  const load = useCallback(async (source: api.WorkspaceImportSource) => {
    reset();
    const token = generation.current;
    setMode("preview");
    pending.current = true;
    setBusy(true);
    try {
      const next = await client.sessionImportWorkspacePreview(source);
      if (generation.current !== token) return;
      setPreview(next);
      setSourceId(next.workspace.sourceIdentity?.sourceId ?? null);
      setWorkspace(next.workspace);
      setChoices(rootChoices(next.workspace));
    } catch (cause) {
      if (generation.current === token) setError(String(cause));
    } finally {
      if (generation.current === token) { pending.current = false; setBusy(false); }
    }
  }, [client, reset]);

  const save = useCallback(async () => {
    if (pending.current || (mode !== "preview" && mode !== "edit")) return;
    if (mode === "preview" && !preview) return;
    if (mode === "edit" && (!workspace || !savedRevision)) return;
    const token = generation.current;
    pending.current = true;
    setBusy(true);
    setError(null);
    try {
      let saved: api.SourceWorkspace;
      if (mode === "edit" && workspace && savedRevision) {
        saved = (await client.sessionImportWorkspaceUpdate({
          sourceWorkspaceId: workspace.sourceWorkspaceId, expectedRevision: savedRevision, choices,
          ...(sourceId ? { sourceId } : {}),
        })).workspace;
      } else if (mode === "preview" && preview) {
        saved = await client.sessionImportWorkspaceSave({
          source: preview.source, expectedFingerprint: preview.fingerprint,
          expectedSavedRevision: preview.savedRevision, choices,
        });
      } else return;
      if (generation.current !== token) return;
      setWorkspace(saved);
      setSourceId(saved.sourceIdentity?.sourceId ?? null);
      setChoices(saved.mappingChoices ?? choices);
      setMode("saved");
      setPreview(null);
      setEditBase(null);
      setSavedRevision(null);
    } catch (cause) {
      if (generation.current === token) setError(String(cause));
    } finally {
      if (generation.current === token) { pending.current = false; setBusy(false); }
    }
  }, [choices, client, mode, preview, savedRevision, sourceId, workspace]);

  const list = useCallback(async (more = false) => {
    if (more && (pending.current || !cursor)) return;
    if (!more) reset();
    const token = generation.current;
    setMode("list");
    setBusy(true);
    pending.current = true;
    setError(null);
    try {
      const page = await client.sessionImportWorkspaces(more ? cursor : null);
      if (generation.current !== token) return;
      setItems((previous) => more
        ? [...previous, ...page.items.filter((item) => !previous.some((row) => row.sourceWorkspaceId === item.sourceWorkspaceId))]
        : page.items);
      setCursor(page.nextCursor);
    } catch (cause) {
      if (generation.current === token) setError(String(cause));
    } finally {
      if (generation.current === token) { pending.current = false; setBusy(false); }
    }
  }, [client, cursor, reset]);

  const showSaved = useCallback((saved: api.SourceWorkspace) => {
    reset();
    setMode("saved");
    setWorkspace(saved);
    setSourceId(saved.sourceIdentity?.sourceId ?? null);
    setChoices(rootChoices(saved));
  }, [reset]);

  const edit = useCallback(async () => {
    if (!workspace || pending.current || (mode !== "saved" && mode !== "edit")) return;
    const selected = workspace;
    reset();
    const token = generation.current;
    setMode("edit");
    setWorkspace(selected);
    setEditBase(selected);
    setChoices(rootChoices(selected));
    pending.current = true;
    setBusy(true);
    try {
      const next = await client.sessionImportWorkspaceGet(selected.sourceWorkspaceId);
      if (generation.current !== token) return;
      setWorkspace(next.workspace);
      setSourceId(next.workspace.sourceIdentity?.sourceId ?? null);
      setEditBase(next.workspace);
      setChoices(rootChoices(next.workspace));
      setSavedRevision(next.revision);
    } catch (cause) {
      if (generation.current === token) setError(String(cause));
    } finally {
      if (generation.current === token) { pending.current = false; setBusy(false); }
    }
  }, [client, mode, reset, workspace]);

  const cancelEdit = useCallback(() => {
    if (pending.current) return;
    if (editBase) showSaved(editBase);
    else reset();
  }, [editBase, reset, showSaved]);

  const editable = mode === "preview" || (mode === "edit" && savedRevision !== null);

  const chooseSource = useCallback(async (nextSourceId: string | null) => {
    if (pending.current || !editable) return;
    if (mode === "edit") {
      // Adoption is saved together with the mapping draft. Existing identities
      // cannot be reassigned; that would change the meaning of external links.
      if (!workspace?.sourceIdentity) setSourceId(nextSourceId);
      return;
    }
    if (!preview || nextSourceId === sourceId) return;
    const token = generation.current;
    pending.current = true;
    setBusy(true);
    setError(null);
    try {
      const next = await client.sessionImportWorkspacePreview({ ...preview.source, sourceId: nextSourceId });
      if (generation.current !== token) return;
      setPreview(next);
      setWorkspace(next.workspace);
      setSourceId(next.workspace.sourceIdentity?.sourceId ?? null);
      setChoices(rootChoices(next.workspace));
      setProjectNotice(null);
    } catch (cause) {
      if (generation.current === token) setError(String(cause));
    } finally {
      if (generation.current === token) { pending.current = false; setBusy(false); }
    }
  }, [client, editable, mode, preview, sourceId, workspace]);

  const chooseTarget = useCallback(async (rootId: string): Promise<boolean> => {
    if (!editable || pending.current || !choices.some((row) => row.rootId === rootId)) return false;
    const token = generation.current;
    pending.current = true;
    setBusy(true);
    setError(null);
    try {
      const path = await client.pickDirectory();
      if (generation.current !== token) return false;
      if (path) setChoices((previous) => previous.map((row) => row.rootId === rootId
        ? { ...row, targetProjectId: null, explicitTarget: path } : row));
      return true;
    } catch (cause) {
      if (generation.current !== token) return false;
      setError(String(cause));
      return true;
    } finally {
      if (generation.current === token) { pending.current = false; setBusy(false); }
    }
  }, [choices, client, editable]);

  const setProject = useCallback((rootId: string, targetProjectId: string | null) => {
    if (pending.current || !editable) return;
    setChoices((previous) => previous.map((row) => row.rootId === rootId
      ? { ...row, targetProjectId, explicitTarget: undefined } : row));
  }, [editable]);

  const createProject = useCallback(async (rootId: string) => {
    const target = choices.find((choice) => choice.rootId === rootId)?.explicitTarget;
    if (!target || !workspace || !editable || pending.current) return;
    const token = generation.current;
    const key = JSON.stringify([workspace.sourceWorkspaceId, rootId, target]);
    const requestId = projectRequests.current.get(key) ?? crypto.randomUUID();
    projectRequests.current.set(key, requestId);
    pending.current = true;
    setBusy(true);
    setError(null);
    setProjectNotice(null);
    try {
      const result = await client.sessionImportProjectCreate(requestId, target);
      // The native project survives leaving/cancelling the mapping draft.
      // Refresh failures must not turn a committed creation into a failed one.
      void Promise.resolve().then(() => onProjectsChanged?.()).catch((cause) => {
        console.warn("Could not refresh the project list after import creation", cause);
      });
      if (generation.current !== token) return;
      setCreatedProjects((previous) => [result.project, ...previous.filter((row) => row.id !== result.project.id)]);
      setChoices((previous) => previous.map((choice) => choice.rootId === rootId
        ? { ...choice, targetProjectId: result.project.id, explicitTarget: undefined } : choice));
      setProjectNotice({ rootId, name: result.project.name });
    } catch (cause) {
      if (generation.current === token) setError(String(cause));
    } finally {
      if (generation.current === token) { pending.current = false; setBusy(false); }
    }
  }, [choices, client, editable, onProjectsChanged, workspace]);

  const setPrimary = useCallback((rootId: string) => {
    if (pending.current || !editable || !choices.some((row) => row.rootId === rootId)) return;
    setChoices((previous) => previous.map((row) => ({ ...row, primary: row.rootId === rootId })));
  }, [choices, editable]);

  const createNative = useCallback(async () => {
    if (pending.current || mode !== "saved" || !workspace) return;
    const token = generation.current;
    pending.current = true;
    setBusy(true);
    setError(null);
    try {
      const saved = await client.sessionImportWorkspaceGet(workspace.sourceWorkspaceId);
      if (generation.current !== token) return;
      // A list/save result is what the user reviewed. Never silently create a
      // copy of a newer mapping fetched between that review and this click.
      if (JSON.stringify(saved.workspace) !== JSON.stringify(workspace)) {
        setError("PLAN_STALE: workspace mappings changed");
        return;
      }
      const created = await client.sessionImportWorkspaceApply({
        sourceWorkspaceId: workspace.sourceWorkspaceId, expectedRevision: saved.revision,
      });
      if (generation.current === token) setNativeWorkspace(created);
    } catch (cause) {
      if (generation.current === token) setError(String(cause));
    } finally {
      if (generation.current === token) { pending.current = false; setBusy(false); }
    }
  }, [client, mode, workspace]);

  return { mode, preview, workspace, choices, items, cursor, busy, error, editable, sourceId, chooseSource, nativeWorkspace, createNative, createdProjects, projectNotice, createProject, reset, load, save, list, showSaved, edit, cancelEdit, chooseTarget, setProject, setPrimary };
}

export type WorkspaceImportUi = ReturnType<typeof useWorkspaceImport>;
