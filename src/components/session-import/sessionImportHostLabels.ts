import type { MessageKey, Vars } from "@/i18n";

export type SessionImportTFn = (key: MessageKey, vars?: Vars) => string;

export function outcomeLabel(raw: string, tr: SessionImportTFn): string {
  switch (raw) {
    case "complete":
      return tr("sessionImport.outcome.complete");
    case "partial":
      return tr("sessionImport.outcome.partial");
    case "unsupported":
      return tr("sessionImport.outcomeUnsupported");
    case "failed":
      return tr("sessionImport.job.failed");
    default:
      return raw;
  }
}

export function coverageLabel(coverage: string, tr: SessionImportTFn): string {
  switch (coverage) {
    case "full-selection":
      return tr("sessionImport.coverageFull");
    case "truncated-to-budget":
      return tr("sessionImport.coverageTruncated");
    case "pinned-over-budget":
      return tr("sessionImport.coveragePinned");
    case "boundary-over-budget":
      return tr("sessionImport.coverageBoundary");
    default:
      return coverage;
  }
}
