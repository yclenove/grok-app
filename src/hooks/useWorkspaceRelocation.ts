import { useCallback, useEffect, useRef, useState } from "react";
import { pickAttachFiles, pickDirectory } from "@/lib/api/fs";
import { sessionImportWorkspaceGet, type SourceWorkspace } from "@/lib/api/sessionMigration";
import { workspaceRelocationPreview, workspaceRelocationSave, type WorkspaceRelocationPreview, type WorkspaceRelocationSave } from "@/lib/api/workspaceRelocation";
import { clearRelocationDraft, readRelocationDraft, writeRelocationDraft } from "@/lib/sessionImport/relocationDraft";

export type WorkspaceRelocationClient = {
  pickAttachFiles: typeof pickAttachFiles;
  pickDirectory: typeof pickDirectory;
  sessionImportWorkspaceGet: typeof sessionImportWorkspaceGet;
  workspaceRelocationPreview: typeof workspaceRelocationPreview;
  workspaceRelocationSave: typeof workspaceRelocationSave;
  readRelocationDraft: typeof readRelocationDraft;
  writeRelocationDraft: typeof writeRelocationDraft;
  clearRelocationDraft: typeof clearRelocationDraft;
};

const workspaceRelocationClient: WorkspaceRelocationClient = {
  pickAttachFiles: () => pickAttachFiles(),
  pickDirectory: () => pickDirectory(),
  sessionImportWorkspaceGet: (id) => sessionImportWorkspaceGet(id),
  workspaceRelocationPreview: (request) => workspaceRelocationPreview(request),
  workspaceRelocationSave: (request) => workspaceRelocationSave(request),
  readRelocationDraft: (id) => readRelocationDraft(id),
  writeRelocationDraft: (request) => writeRelocationDraft(request),
  clearRelocationDraft: (id, requestId) => clearRelocationDraft(id, requestId),
};

/** Mount per saved workspace. No transcript, native project or agent operations. */
export function useWorkspaceRelocation(workspace: SourceWorkspace, onSaved: (workspace: SourceWorkspace) => void,
  client: WorkspaceRelocationClient = workspaceRelocationClient) {
  const generation = useRef(0);
  const pending = useRef(false);
  const frozen = useRef<WorkspaceRelocationSave | null>(null);
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [preview, setPreview] = useState<WorkspaceRelocationPreview | null>(null);
  const [attempt, setAttempt] = useState<WorkspaceRelocationSave | null>(null);
  useEffect(() => () => { generation.current += 1; }, []);

  const start = useCallback(() => {
    if (pending.current) return;
    setOpen(true);
    setError(null);
    try {
      frozen.current = client.readRelocationDraft(workspace.sourceWorkspaceId);
      setAttempt(frozen.current);
    } catch (cause) { setError(String(cause)); }
  }, [client, workspace.sourceWorkspaceId]);

  const close = useCallback(() => {
    // A picker/preview can be cancelled locally. Once saving, wait for its result;
    // exiting the parent view still preserves the durable confirmation.
    if (pending.current && frozen.current) return;
    generation.current += 1;
    pending.current = false;
    setBusy(false);
    setOpen(false);
    setPreview(null);
    setError(null);
  }, []);

  const review = useCallback(async (path?: string) => {
    if (pending.current || frozen.current) return;
    const token = generation.current;
    pending.current = true;
    setBusy(true);
    setError(null);
    try {
      if (!path) {
        if (workspace.workspaceFile) {
          const files = await client.pickAttachFiles();
          if (!files.length) return;
          if (files.length !== 1) throw new Error("FORMAT_UNSUPPORTED: select one workspace file");
          path = files[0];
        } else path = (await client.pickDirectory()) ?? undefined;
      }
      if (generation.current !== token || !path) return;
      setPreview(null);
      const saved = await client.sessionImportWorkspaceGet(workspace.sourceWorkspaceId);
      if (generation.current !== token) return;
      const result = await client.workspaceRelocationPreview({ sourceWorkspaceId: workspace.sourceWorkspaceId, expectedRevision: saved.revision, path });
      if (generation.current === token) setPreview(result);
    } catch (cause) {
      if (generation.current === token) setError(String(cause));
    } finally {
      if (generation.current === token) { pending.current = false; setBusy(false); }
    }
  }, [client, workspace]);

  const save = useCallback(async () => {
    if (pending.current || (!preview && !frozen.current)) return;
    const token = generation.current;
    pending.current = true;
    setBusy(true);
    setError(null);
    try {
      const request = frozen.current ?? {
        requestId: crypto.randomUUID(), preview: preview!.request,
        fingerprint: preview!.fingerprint, proposalHash: preview!.proposalHash,
      };
      // If local persistence fails, do not dispatch an unrecoverable request.
      client.writeRelocationDraft(request);
      frozen.current = request;
      setAttempt(request);
      const result = await client.workspaceRelocationSave(request);
      client.clearRelocationDraft(workspace.sourceWorkspaceId, request.requestId);
      if (generation.current !== token) return;
      frozen.current = null;
      setAttempt(null);
      setPreview(null);
      setOpen(false);
      onSaved(result.workspace);
    } catch (cause) {
      if (generation.current !== token) return;
      const message = String(cause);
      setError(message);
      if (/^(?:Error: )?(?:PLAN_STALE|SOURCE_CHANGED|MAPPING_CONFLICT|SOURCE_NOT_FOUND|FORMAT_UNSUPPORTED|WORKSPACE_UNRESOLVED):/.test(message)) {
        // A definitive refusal allows a new review. Transport/storage errors keep
        // the exact attempt, since the Host might already have committed it.
        try {
          if (frozen.current) client.clearRelocationDraft(workspace.sourceWorkspaceId, frozen.current.requestId);
          frozen.current = null;
          setAttempt(null);
          setPreview(null);
        } catch { /* The original error and frozen retry remain visible. */ }
      }
    } finally {
      if (generation.current === token) { pending.current = false; setBusy(false); }
    }
  }, [client, onSaved, preview, workspace.sourceWorkspaceId]);

  return { open, busy, error, preview, attempt, start, close, review, save };
}
