/**
 * Multi-root workspace modal state (#1194 MVP-0).
 * Keep out of AppWorkbench — pass thin callbacks only.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import * as api from "@/lib/api";
import type { WorkspaceRecord, WorkspaceRoot } from "@/lib/multiRootWorkspace";
import {
  MAX_EXTRA_WORKSPACE_ROOTS,
  extraRoots,
} from "@/lib/multiRootWorkspace";

export type MultiRootWorkspaceTarget = {
  projectId: string;
  projectName: string;
  projectPath: string;
  sessionId: string | null;
  workspaceId: string | null;
};

export function useMultiRootWorkspace() {
  const generation = useRef(0);
  const pending = useRef(false);
  const [open, setOpen] = useState(false);
  const [target, setTarget] = useState<MultiRootWorkspaceTarget | null>(null);
  const [draft, setDraft] = useState<WorkspaceRecord | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [writeCapableMode, setWriteCapableMode] = useState(false);
  const [workspaces, setWorkspaces] = useState<WorkspaceRecord[]>([]);

  useEffect(() => () => { generation.current += 1; }, []);

  const close = useCallback(() => {
    generation.current += 1;
    pending.current = false;
    setOpen(false);
    setTarget(null);
    setDraft(null);
    setError(null);
    setBusy(false);
    setWorkspaces([]);
  }, []);

  const openFor = useCallback(async (requested: MultiRootWorkspaceTarget) => {
    let next = requested;
    const token = ++generation.current;
    pending.current = true;
    setTarget(next);
    setOpen(true);
    setError(null);
    setBusy(true);
    setDraft(null);
    setWorkspaces([]);
    try {
      if (next.sessionId) {
        const [sessions, projects] = await Promise.all([
          api.sessionsList(),
          api.projectsList(),
        ]);
        if (generation.current !== token) return;
        const session = sessions.find((row) => row.id === next.sessionId);
        const project = projects.find((row) => row.id === session?.projectId);
        if (!session || !project || project.sshAlias) {
          throw new Error("MAPPING_CONFLICT: current session project unavailable");
        }
        next = {
          ...next,
          projectId: project.id,
          projectName: project.name,
          projectPath: project.path,
          workspaceId: session.workspaceId ?? null,
        };
        setTarget(next);
      }
      try {
        const settings = await api.settingsGet();
        if (generation.current !== token) return;
        setWriteCapableMode(
          (settings.sessionDataMode || "").toLowerCase() === "independent",
        );
      } catch {
        if (generation.current !== token) return;
        setWriteCapableMode(false);
      }
      let ws: WorkspaceRecord | null = null;
      const list = await api.workspacesForProject(next.projectId);
      if (generation.current !== token) return;
      setWorkspaces(list);
      if (next.workspaceId) {
        ws = (await api.workspaceGet(next.workspaceId)) ?? null;
        if (generation.current !== token) return;
        if (ws && ws.primaryProjectId !== next.projectId) {
          throw new Error("MAPPING_CONFLICT: workspace project mismatch");
        }
      }
      if (!ws) {
        ws = list[0] ?? null;
      }
      if (!ws) {
        ws = {
          id: "",
          name: `${next.projectName} workspace`,
          primaryProjectId: next.projectId,
          roots: [
            {
              path: next.projectPath,
              role: "primary",
              access: "write",
              pathOk: true,
            },
          ],
          capability: "contextOnly",
          updatedAt: new Date().toISOString(),
        };
      }
      setDraft(ws);
    } catch (e) {
      if (generation.current !== token) return;
      setError(String(e));
      setDraft(null);
    } finally {
      if (generation.current === token) {
        pending.current = false;
        setBusy(false);
      }
    }
  }, []);

  const selectWorkspace = useCallback((id: string) => {
    if (pending.current) return;
    const selected = workspaces.find(
      (workspace) =>
        workspace.id === id && workspace.primaryProjectId === target?.projectId,
    );
    if (!selected) return;
    setDraft(selected);
    setError(null);
  }, [target?.projectId, workspaces]);

  const addExtraRoot = useCallback(async () => {
    if (!draft || pending.current) return;
    if (extraRoots(draft).length >= MAX_EXTRA_WORKSPACE_ROOTS) {
      setError(`max ${MAX_EXTRA_WORKSPACE_ROOTS}`);
      return;
    }
    const token = generation.current;
    pending.current = true;
    setBusy(true);
    setError(null);
    try {
      const picked = await api.pickDirectory();
      if (!picked || generation.current !== token) return;
      const validated = await api.workspaceValidateRoot(picked);
      if (generation.current !== token) return;
      const path = validated.path;
      if (draft.roots.some((r) => r.path === path)) {
        setError("duplicate");
        return;
      }
      setDraft({
        ...draft,
        roots: [
          ...draft.roots,
          {
            path,
            role: "extra",
            access: "read",
            pathOk: validated.pathOk ?? true,
          },
        ],
      });
    } catch (e) {
      if (generation.current === token) setError(String(e));
    } finally {
      if (generation.current === token) {
        pending.current = false;
        setBusy(false);
      }
    }
  }, [draft]);

  const removeExtraRoot = useCallback(
    (path: string) => {
      if (!draft || pending.current) return;
      setDraft({
        ...draft,
        roots: draft.roots.filter(
          (r) => !(r.role === "extra" && r.path === path),
        ),
      });
    },
    [draft],
  );

  const setExtraAccess = useCallback(
    (path: string, access: "read" | "write") => {
      if (!draft || pending.current) return;
      setDraft({
        ...draft,
        roots: draft.roots.map((r) =>
          r.role === "extra" && r.path === path ? { ...r, access } : r,
        ),
      });
    },
    [draft],
  );

  const save = useCallback(async () => {
    if (!draft || !target || pending.current) return null;
    const token = generation.current;
    pending.current = true;
    setBusy(true);
    setError(null);
    try {
      const roots: WorkspaceRoot[] = draft.roots.slice();
      const saved = await api.workspaceUpsert({
        id: draft.id || null,
        name: draft.name,
        primaryProjectId: target.projectId,
        roots,
      });
      if (generation.current !== token) return null;
      if (target.sessionId) {
        await api.sessionSetWorkspace(target.sessionId, saved.id);
        if (generation.current !== token) return null;
      }
      try {
        await api.settingsSet({ recentWorkspaceId: saved.id });
      } catch {
        /* soft */
      }
      if (generation.current !== token) return null;
      setDraft(saved);
      setWorkspaces((previous) => [
        saved,
        ...previous.filter((row) => row.id !== saved.id),
      ]);
      return saved;
    } catch (e) {
      if (generation.current === token) setError(String(e));
      return null;
    } finally {
      if (generation.current === token) {
        pending.current = false;
        setBusy(false);
      }
    }
  }, [draft, target]);

  const clearBinding = useCallback(async () => {
    if (!target?.sessionId || pending.current) return false;
    const token = generation.current;
    pending.current = true;
    setBusy(true);
    try {
      await api.sessionSetWorkspace(target.sessionId, null);
      return generation.current === token;
    } catch (e) {
      if (generation.current === token) setError(String(e));
      return false;
    } finally {
      if (generation.current === token) {
        pending.current = false;
        setBusy(false);
      }
    }
  }, [target]);

  return {
    open,
    target,
    draft,
    setDraft,
    busy,
    error,
    openFor,
    close,
    addExtraRoot,
    removeExtraRoot,
    setExtraAccess,
    save,
    clearBinding,
    writeCapableMode,
    workspaces,
    selectWorkspace,
  };
}
