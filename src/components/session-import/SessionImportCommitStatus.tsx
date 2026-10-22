import type { ImportCommitPhase } from "@/hooks/useSessionImport";
import type { SessionImportTFn } from "./sessionImportHostLabels";

export function SessionImportCommitStatus(props: {
  phase: ImportCommitPhase;
  tr: SessionImportTFn;
}) {
  if (!props.phase) return null;
  const label = props.tr(props.phase === "planning"
    ? "sessionImport.start.planning" : "sessionImport.start.submitting");
  return (
    <div className="session-import__commit-status" data-import-commit-phase={props.phase}>
      <p className="settings-row__desc" role="status" aria-atomic="true">{label}</p>
      <div className="session-import__progress" role="progressbar" aria-label={label} data-state="queued">
        <span />
      </div>
    </div>
  );
}
