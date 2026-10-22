import type { WorkspaceRelocationSave } from "@/lib/api/workspaceRelocation";

const key = (id: string) => `grok:import-workspace-relocation:v1:${id}`;

/** Only the small frozen confirmation is retained, never source contents. Host
 * receipts remain authoritative; storage is not permission to skip validation. */
export function readRelocationDraft(id: string): WorkspaceRelocationSave | null {
  const raw = localStorage.getItem(key(id));
  if (!raw) return null;
  if (raw.length > 32768) throw new Error("PLAN_STALE: invalid relocation draft");
  const value = JSON.parse(raw) as Partial<WorkspaceRelocationSave> | null;
  if (!value || typeof value.requestId !== "string" || !value.requestId || value.requestId.length > 256
    || value.preview?.sourceWorkspaceId !== id || typeof value.preview.path !== "string"
    || !value.preview.path || value.preview.path.length > 8192 || value.preview.path.includes("\0")
    || typeof value.preview.expectedRevision !== "string" || !/^[a-f0-9]{64}$/i.test(value.preview.expectedRevision)
    || typeof value.fingerprint !== "string" || !/^[a-f0-9]{64}$/i.test(value.fingerprint)
    || typeof value.proposalHash !== "string" || !/^[a-f0-9]{64}$/i.test(value.proposalHash)) {
    throw new Error("PLAN_STALE: invalid relocation draft");
  }
  return value as WorkspaceRelocationSave;
}

export function writeRelocationDraft(request: WorkspaceRelocationSave) {
  localStorage.setItem(key(request.preview.sourceWorkspaceId), JSON.stringify(request));
}

export function clearRelocationDraft(id: string, requestId: string) {
  // A late reply from another view must not erase a newer confirmation.
  if (readRelocationDraft(id)?.requestId === requestId) localStorage.removeItem(key(id));
}
