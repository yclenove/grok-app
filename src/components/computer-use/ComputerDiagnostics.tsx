import { createT, type Locale } from "@/i18n";
import type { ComputerStatus } from "@/lib/api/computerUse";

/** Raw protocol vocabulary belongs here, never in the primary action surface. */
export function ComputerDiagnostics({ locale, status }: { locale: Locale; status: ComputerStatus }) {
  const tr = createT(locale);
  return (
    <details className="cu-panel__diag">
      <summary>{tr("cu.panel.diagnostics")}</summary>
      <dl className="cu-panel__diag-list">
        <dt>{tr("cu.panel.diagRun")}</dt><dd>{status.runId ?? "—"}</dd>
        <dt>{tr("cu.panel.diagTarget")}</dt><dd>{status.targetId ?? "—"}</dd>
        <dt>{tr("cu.panel.diagBackend")}</dt><dd>{status.backend}</dd>
        <dt>{tr("cu.panel.diagStop")}</dt><dd>{status.stopState}</dd>
        {status.mcpCatalog ? <>
          <dt>{tr("cu.panel.diagMcp")}</dt>
          <dd>{status.mcpCatalog.cleanupPending ? "cleanup_pending" : status.mcpCatalog.pending ? "pending" : "applied"}</dd>
          <dt>{tr("cu.panel.diagMcpGeneration")}</dt>
          <dd>{status.mcpCatalog.desiredGeneration}/{status.mcpCatalog.appliedGeneration ?? "—"}</dd>
          {status.mcpCatalog.lastError ? <>
            <dt>{tr("cu.panel.diagMcpError")}</dt><dd>{status.mcpCatalog.lastError}</dd>
          </> : null}
        </> : null}
      </dl>
      <ol className="cu-panel__trace" aria-label={tr("cu.panel.trace")}>
        {status.traces.filter((trace) => trace.audience === "ui").slice(-8).map((trace, i) => (
          <li key={`${trace.ms}-${trace.kind}-${i}`}>{trace.kind}{trace.detail ? ` · ${trace.detail}` : ""}</li>
        ))}
      </ol>
    </details>
  );
}
