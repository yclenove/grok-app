import { useId, useState } from "react";
import type { MessageKey, Vars } from "@/i18n";
import type { ImportedRevisionHealth } from "@/lib/api/sessionMigration";

export function ImportedRevisionNotice({ health, tr }: {
  health: ImportedRevisionHealth | null | undefined;
  tr: (key: MessageKey, vars?: Vars) => string;
}) {
  const [expanded, setExpanded] = useState(false);
  const detailsId = useId();
  if (!health || (health.outcome === "complete" && health.issueCount === 0)) return null;
  const unavailable = health.outcome === "unknown" || health.issueCount == null;
  return (
    <section className="session-import__revision-notice" aria-label={tr("sessionImport.revision.title")}>
      <p role="status">
        <strong>{tr(unavailable ? "sessionImport.revision.unavailable" : "sessionImport.revision.incomplete")}</strong>
        {health.issueCount != null ? <> {tr("sessionImport.revision.issueCount", { n: health.issueCount })}</> : null}
      </p>
      <p className="settings-row__desc">{tr("sessionImport.revision.scope")}</p>
      {health.issues.length > 0 ? (
        <>
          <button type="button" className="btn btn--sm" aria-expanded={expanded} aria-controls={detailsId}
            onClick={() => setExpanded((value) => !value)}>
            {tr("sessionImport.revision.details")}
          </button>
          {expanded ? (
            <div id={detailsId}>
              <ol className="session-import__revision-issues">
                {health.issues.map((issue, index) => (
                  <li key={index}>
                    {issue.code ? <code>{issue.code}</code> : null}
                    <p>{issue.message || tr("sessionImport.revision.unknownIssue")}</p>
                  </li>
                ))}
              </ol>
              {health.diagnosticsLimited ? (
                <p className="settings-row__desc">{tr("sessionImport.revision.limited", {
                  shown: health.issues.length, total: health.issueCount ?? health.issues.length,
                })}</p>
              ) : null}
            </div>
          ) : null}
        </>
      ) : null}
    </section>
  );
}
