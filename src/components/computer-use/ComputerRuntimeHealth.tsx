import { IconAlertTriangle, IconCircleCheck } from "@tabler/icons-react";
import type { createT, MessageKey } from "@/i18n";
import type { ComputerRuntimeStatus } from "@/lib/api/computerUse";

const ISSUE_KEYS: Record<string, MessageKey> = {
  missing_file: "cu.runtime.missing_file", hash_mismatch: "cu.runtime.hash_mismatch",
  arch_mismatch: "cu.runtime.arch_mismatch", version_mismatch: "cu.runtime.version_mismatch",
  permission_denied: "cu.runtime.permission_denied", port_failed: "cu.runtime.port_failed",
  interrupted_upgrade: "cu.runtime.interrupted_upgrade",
};

/** Summarize recovery steps without discarding the Host's component-level evidence. */
export function ComputerRuntimeHealth({ runtime, loading, tr }: {
  runtime: ComputerRuntimeStatus | null;
  loading: boolean;
  tr: ReturnType<typeof createT>;
}) {
  if (loading) return <p role="status">{tr("cu.status.checking")}</p>;
  if (!runtime) return <p role="status" className="cu-settings__health" data-state="unknown">{tr("cu.settings.runtimeUnknown")}</p>;
  const codes = [...new Set(runtime.issues.map((issue) => issue.code))];
  return <>
    <div className="cu-settings__health" data-state={codes.length ? "warning" : "ready"} role="status">
      {codes.length ? <IconAlertTriangle size={18} aria-hidden="true" />
        : <IconCircleCheck size={18} aria-hidden="true" />}
      {codes.length ? <ul className="cu-settings__issues">{codes.map((code) => (
        <li key={code}>{tr(ISSUE_KEYS[code] ?? "cu.panel.error")}</li>
      ))}</ul> : <p>{tr("cu.settings.runtimeOk")}</p>}
    </div>
    {codes.length ? <details className="cu-settings__diagnostics">
      <summary>{tr("cu.settings.diagnostics")}</summary>
      <dl className="cu-panel__diag-list">
        {runtime.issues.map((issue, index) => <div key={index}>
          <dt><code>{issue.component}</code></dt>
          <dd><code>{issue.code}</code></dd>
        </div>)}
      </dl>
    </details> : null}
  </>;
}
