import { useMemo } from "react";
import { createT, type Locale } from "@/i18n";
import type { ComputerTaskView } from "@/lib/api/computerTask";

/** Retained presentation facts only. No native query or credential access. */
export function ComputerTaskMetadata({ locale, view }: { locale: Locale; view: ComputerTaskView }) {
  const tr = useMemo(() => createT(locale), [locale]);
  const rows = view.executions.filter((run) => run.targetName || run.backend);
  if (!rows.length) return null;
  return <details className="cu-panel__diag">
    <summary>{tr("cu.panel.diagnostics")}</summary>
    {rows.map((run) => <div key={run.runId}>
      {run.targetName ? <p className="cu-task-card__meta">{run.targetName}</p> : null}
      {run.backend ? <p className="cu-task-card__meta">{tr("cu.task.backend", { name: run.backend })}</p> : null}
    </div>)}
  </details>;
}
