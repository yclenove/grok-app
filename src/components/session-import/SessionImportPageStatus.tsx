import type { ImportStep } from "@/hooks/useSessionImport";
import type { SessionImportTFn } from "./sessionImportHostLabels";

const HEADING = {
  source: "sessionImport.step.source",
  content: "sessionImport.step.content",
  prepare: "sessionImport.step.prepare",
  mapping: "sessionImport.step.mapping",
  import: "sessionImport.step.import",
  result: "sessionImport.step.result",
} as const;

const STATUS = {
  source: "sessionImport.pageStatus.source",
  content: "sessionImport.pageStatus.content",
  prepare: "sessionImport.pageStatus.prepare",
  mapping: "sessionImport.pageStatus.mapping",
  import: "sessionImport.pageStatus.import",
  result: "sessionImport.pageStatus.result",
} as const;

const HELP = {
  source: "sessionImport.help.source",
  content: "sessionImport.help.content",
  prepare: "sessionImport.help.prepare",
  mapping: "sessionImport.help.mapping",
  import: "sessionImport.help.import",
  result: "sessionImport.help.result",
} as const;

/** One step heading, one status line, and optional help. Measured counts stay in the step body. */
export function SessionImportPageStatus({ tr, step }: { tr: SessionImportTFn; step: ImportStep }) {
  return (
    <div className="session-import__page-status">
      <h3 className="session-import__page-title">{tr(HEADING[step])}</h3>
      <p className="session-import__page-line">{tr(STATUS[step])}</p>
      <details className="session-import__help">
        <summary>{tr("sessionImport.helpSummary")}</summary>
        <p>{tr(HELP[step])}</p>
      </details>
    </div>
  );
}
