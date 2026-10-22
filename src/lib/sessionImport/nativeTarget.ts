import * as api from "@/lib/api";
import { mapSessionListRow, normalizeProject, type Project, type SessionRow } from "@/lib/app/sidebarModels";
import { withDeadline } from "@/lib/sessionConnectTimeout";
import { isChatSessionId } from "@/lib/chatAttach";

export type NativeTargetClient = Pick<typeof api, "sessionsList" | "projectsList">;

/** A newly published continuation is not necessarily in the painted sidebar yet.
 * Resolve its durable row and project before normal navigation can bind cwd,
 * trust, composer preferences or warm-connect. Never invent an orphan row. */
export async function resolveImportedNativeTarget(
  id: string,
  client: NativeTargetClient = api,
): Promise<{ row: SessionRow; project: Project | null }> {
  const nativeId = id.trim();
  if (!isChatSessionId(nativeId)) throw new Error("SESSION_OPEN_MISSING");
  const [sessions, projects] = await withDeadline(
    Promise.all([client.sessionsList(), client.projectsList()]),
    30_000,
    () => new Error("SESSION_OPEN_UNAVAILABLE"),
  );
  const saved = sessions.find((row) => row.id === nativeId && !row.archived);
  if (!saved) throw new Error("SESSION_OPEN_MISSING");
  const row = mapSessionListRow(saved);
  const savedProject = row.projectId ? projects.find((item) => item.id === row.projectId) : null;
  if (row.projectId && !savedProject) throw new Error("WORKSPACE_UNRESOLVED");
  return { row, project: savedProject ? normalizeProject(savedProject) : null };
}
