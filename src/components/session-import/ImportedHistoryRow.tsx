import { IconExportMd, IconFork } from "@/components/icons";
import { openImportedHistory } from "@/lib/sessionImport/open";
import type { CatalogEntry } from "@/lib/api/sessionMigration";
import type { MessageKey, Vars } from "@/i18n";
import { ImportIconButton } from "./ImportIconButton";

export function ImportedHistoryRow({ entry, providerLabel, exporting, exportBusy, onExport, onOpen = openImportedHistory, onContinue, tr }: {
  entry: CatalogEntry;
  providerLabel: string;
  exporting: boolean;
  exportBusy: boolean;
  onExport: (id: string) => void;
  onOpen?: typeof openImportedHistory;
  onContinue?: (id: string) => void;
  tr: (key: MessageKey, vars?: Vars) => string;
}) {
  const target = { importId: entry.importId, ...(entry.currentRevision ? { revision: entry.currentRevision } : {}) };
  return (
    <li className="tree-imported__row">
      <button type="button" className="btn btn--ghost tree-imported__title"
        onClick={() => onOpen(target)}>
        <span className="tree-imported__name">{entry.alias || entry.title}</span>
        <span className="tree-imported__meta">
          {providerLabel ? <span>{providerLabel}</span> : null}
          {entry.createdAt ? <time dateTime={entry.createdAt}>{entry.createdAt.slice(0, 10)}</time> : null}
        </span>
      </button>
      <div className="tree-imported__row-actions">
        <ImportIconButton label={tr("sessionImport.exportMarkdown")} disabled={exportBusy}
          aria-busy={exporting} onClick={() => onExport(entry.importId)}>
          <IconExportMd size={15} />
        </ImportIconButton>
        <ImportIconButton label={tr("sessionImport.continueInGrok")}
          onClick={() => onContinue ? onContinue(entry.importId) : onOpen({ ...target, continue: true })}>
          <IconFork size={15} />
        </ImportIconButton>
      </div>
    </li>
  );
}
