import { useEffect, useMemo, useRef, useState } from "react";
import { createT, type Locale } from "@/i18n";
import { IconDeviceDesktop } from "@/components/icons";
import * as api from "@/lib/api/computerUse";
import { computerUseErrorKey } from "@/lib/computer-use/errors";
import { requestComputerPanel } from "@/lib/computer-use/panelStore";
import { computerStateLabel, type ComputerTaskState } from "@/lib/computer-use/taskCard";

export type ComputerTaskCardProps = {
  locale: Locale;
  sessionId: string | null;
  runId: string | null;
  targetName: string;
  backend: string;
  state: ComputerTaskState;
  /** Exact-run, fresh Host readback, independent of tool-catalog cleanup. */
  stopConfirmed: boolean;
  failure?: string | null;
};

export function ComputerTaskCard({ locale, sessionId, runId, targetName, backend, state, stopConfirmed, failure }: ComputerTaskCardProps) {
  const tr = useMemo(() => createT(locale), [locale]);
  const [error, setError] = useState(false);
  const [pending, setPending] = useState<"pause" | "stop" | null>(null);
  const actionRef = useRef<"pause" | "stop" | null>(null);
  const revision = useRef(0);
  useEffect(() => () => { revision.current += 1; }, [sessionId, runId]);
  useEffect(() => {
    if (!stopConfirmed) return;
    // Native completion can precede the command's transport reply. Retire that
    // reply, not Host failures or the separately reported MCP cleanup state.
    revision.current += 1;
    actionRef.current = null;
    setPending(null);
    setError(false);
  }, [stopConfirmed]);
  async function runAction(action: "pause" | "stop") {
    if (!sessionId || !runId || actionRef.current === "stop" || (action === "pause" && actionRef.current)) return;
    const epoch = ++revision.current;
    actionRef.current = action;
    setPending(action);
    setError(false);
    try {
      await (action === "stop" ? api.computerStop : api.computerPause)({ sessionId, runId });
    } catch {
      if (epoch === revision.current) setError(true);
    } finally {
      if (epoch === revision.current) { actionRef.current = null; setPending(null); }
    }
  }
  return (
    <section className="cu-task-card" data-state={state} aria-label={tr("cu.title")}>
      <div className="cu-task-card__head">
        <span className="cu-panel__emblem cu-panel__emblem--small" aria-hidden><IconDeviceDesktop size={20} /></span>
        <div>
          <p className="cu-task-card__target" title={targetName}>{targetName}</p>
          <p className="cu-task-card__state">{tr(computerStateLabel[state])}</p>
        </div>
      </div>
      <div className="cu-task-card__actions" role="toolbar" aria-label={tr("cu.panel.controls")}>
        <button type="button" className="btn btn--ghost btn--sm" onClick={() => requestComputerPanel()}>
          {tr("cu.workspace.open")}
        </button>
        <button type="button" className="btn btn--ghost btn--sm"
          disabled={state !== "running" || !!pending} onClick={() => void runAction("pause")}>
          {tr("cu.panel.pause")}
        </button>
        <button type="button" className="btn btn--danger cu-panel__stop"
          disabled={stopConfirmed || state === "stop_requested" || pending === "stop"}
          onClick={() => void runAction("stop")}>
          {tr(!stopConfirmed && (pending === "stop" || state === "stop_requested") ? "cu.panel.stopping" : "cu.panel.stop")}
        </button>
      </div>
      <details className="cu-panel__diag">
        <summary>{tr("cu.panel.diagnostics")}</summary>
        <p className="cu-task-card__meta">{tr("cu.task.backend", { name: backend })}</p>
      </details>
      {error || failure ? <p role="alert" className="cu-panel__error">{tr(computerUseErrorKey(failure || "action failed"))}</p> : null}
    </section>
  );
}
