import { useEffect, useRef, useState } from "react";
import "./WorkspaceRuntimeRecovery.css";
import { createT, type Locale } from "@/i18n";
import { GlassModal } from "./GlassModal";
import { WORKSPACE_REVIEW_EVENT } from "@/lib/workspaceRuntime";
import { confirmWorkspaceRuntime, previewWorkspaceRuntime, type WorkspaceRuntimeReview } from "@/lib/api/workspaceRuntime";

/** Local recovery UI: no message dispatch, no App shell state. */
export function WorkspaceRuntimeRecovery({ locale, sessionId, onManage }: { locale: Locale; sessionId: string | null; onManage?: () => void }) {
  const tr = createT(locale);
  const generation = useRef(0);
  const pending = useRef(false);
  const saving = useRef(false);
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [review, setReview] = useState<WorkspaceRuntimeReview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState(false);

  useEffect(() => {
    const load = (event: Event) => {
      if ((event as CustomEvent<unknown>).detail !== sessionId || !sessionId || pending.current) return;
      const token = ++generation.current;
      pending.current = true;
      setOpen(true); setBusy(true); setError(null); setDone(false); setReview(null);
      void previewWorkspaceRuntime(sessionId).then((value) => {
        if (generation.current === token && value.sessionId === sessionId) setReview(value);
      }).catch((reason: unknown) => {
        if (generation.current === token) setError(String(reason));
      }).finally(() => {
        if (generation.current === token) { pending.current = false; setBusy(false); }
      });
    };
    window.addEventListener(WORKSPACE_REVIEW_EVENT, load);
    return () => {
      window.removeEventListener(WORKSPACE_REVIEW_EVENT, load);
      generation.current += 1;
      pending.current = false;
      saving.current = false;
    };
  }, [sessionId]);

  // A review belongs to one visible chat. A late reply must not reopen it.
  useEffect(() => { setOpen(false); setReview(null); setBusy(false); }, [sessionId]);
  const close = () => {
    if (saving.current) return;
    generation.current += 1; pending.current = false;
    setOpen(false); setReview(null); setBusy(false);
  };
  const confirm = async () => {
    if (!review || review.sessionId !== sessionId || pending.current) return;
    const token = generation.current;
    pending.current = true; saving.current = true; setBusy(true); setError(null);
    try {
      await confirmWorkspaceRuntime(review);
      if (generation.current === token) { setDone(true); setReview(null); }
    } catch (reason) {
      if (generation.current === token) { setError(String(reason)); setReview(null); }
    } finally {
      if (generation.current === token) { pending.current = false; saving.current = false; setBusy(false); }
    }
  };
  return <GlassModal open={open} onClose={close} showClose={!saving.current} title={tr("workspace.runtime.title")} closeLabel={tr("common.close")} size="lg" wrapBody className="workspace-runtime-modal"
    footer={<>
      <button type="button" className="btn btn--ghost" disabled={saving.current} onClick={close}>{tr(done ? "common.close" : "common.cancel")}</button>
      {onManage && !done && <button type="button" className="btn btn--ghost" disabled={busy} onClick={() => { close(); onManage(); }}>{tr("workspace.multiRoot.menu")}</button>}
      {review && !done && <button type="button" className="btn btn--solid" disabled={busy} onClick={() => { void confirm(); }}>{tr("workspace.runtime.confirm")}</button>}
    </>}>
    <p role="status">{tr(done ? "workspace.runtime.ready" : busy ? "workspace.runtime.loading" : "workspace.runtime.explain")}</p>
    {review && <div>
      <p>{tr("workspace.runtime.directory", { path: review.sshAlias ? `${review.sshAlias}:${review.cwd}` : review.cwd })}</p>
      <ul>{review.roots.map((root) => <li key={root.path}>
        <code>{root.path}</code>{" · "}{tr(root.role === "primary" ? "workspace.multiRoot.rolePrimary" : "workspace.multiRoot.roleExtra")}
        {" · "}{tr(root.access === "write" ? "workspace.multiRoot.accessWrite" : "workspace.multiRoot.accessRead")}
      </li>)}</ul>
    </div>}
    {error && <div role="alert"><p>{tr("workspace.runtime.failed")}</p><details><summary>{tr("attach.details")}</summary><pre>{error}</pre></details></div>}
  </GlassModal>;
}
