import { useRef, useState, type ReactNode } from "react";
import { openSessionImport, type openImportedHistory } from "@/lib/sessionImport/open";
import { IconChevronDown, IconChevronUp, IconFileUp, IconPlus, IconRefresh, IconSearch } from "@/components/icons";
import type { useImportedCatalog } from "@/hooks/useImportedCatalog";
import {
  sessionImportExportMarkdown,
} from "@/lib/api/sessionMigration";
import type { MessageKey, Vars } from "@/i18n";
import { IMPORTED_LIST_WINDOW } from "@/lib/sessionImport/catalogView";
import { ImportIconButton } from "./ImportIconButton";
import { ImportedHistoryRow } from "./ImportedHistoryRow";

type TFn = (key: MessageKey, vars?: Vars) => string;

function providerLabel(provider: string, tr: TFn): string {
  if (provider === "codex") return tr("sessionImport.source.codex");
  if (provider === "claude-code") return tr("sessionImport.source.claude");
  if (provider === "cursor") return tr("sessionImport.source.cursor");
  if (provider === "generic-file") return tr("sessionImport.source.file");
  return provider;
}

export function ImportedHistoryGroupContent({
  tr, variant = "sidebar", catalog, workspaceFilter, exportMarkdown = sessionImportExportMarkdown,
  onImport = openSessionImport, onOpen, onContinue,
}: {
  tr: TFn;
  variant?: "sidebar" | "settings";
  catalog: Pick<ReturnType<typeof useImportedCatalog>,
    "visible" | "query" | "setQuery" | "provider" | "setProvider" | "providers" |
    "sort" | "setSort" | "loadMore" | "nextCursor" | "total" | "error" |
    "workspace" | "busy" | "queryPending" | "load" | "windowed">;
  workspaceFilter: ReactNode;
  exportMarkdown?: typeof sessionImportExportMarkdown;
  onImport?: typeof openSessionImport;
  onOpen?: typeof openImportedHistory;
  onContinue?: (id: string) => void;
}) {
  const {
    visible,
    query,
    setQuery,
    provider,
    setProvider,
    providers,
    sort,
    setSort,
    loadMore,
    nextCursor,
    total,
    error,
    workspace, busy, queryPending, load, windowed,
  } = catalog;
  const providerOptions = provider && !providers.includes(provider) ? [...providers, provider] : providers;
  const filtered = query.trim().length > 0 || provider !== null || workspace !== null;
  const [notice, setNotice] = useState<"copied" | "error" | null>(null);
  const [exportingId, setExportingId] = useState<string | null>(null);
  const exportPending = useRef(false);

  const exportOne = async (importId: string) => {
    if (exportPending.current) return;
    exportPending.current = true;
    setExportingId(importId);
    setNotice(null);
    try {
      const md = await exportMarkdown(importId);
      await navigator.clipboard.writeText(md);
      setNotice("copied");
    } catch {
      setNotice("error");
    } finally {
      exportPending.current = false;
      setExportingId(null);
    }
  };

  return (
    <div className={variant === "settings" ? "settings-imported" : "tree-imported"}>
      <div className="tree-imported__head">
        <div className="tree-imported__heading">
          <span>{tr("sessionImport.importedGroup")}</span>
          <span className="tree-imported__count">{tr("sessionImport.importedCount", { n: total })}</span>
        </div>
        <ImportIconButton label={tr("sessionImport.refresh")} disabled={busy && !queryPending}
          onClick={() => void load()}>
          <IconRefresh size={15} />
        </ImportIconButton>
      </div>
      {error ? (
        <p role="alert" className="tree-imported__notice tree-imported__notice--error">
          {tr("sessionImport.catalogError")}
        </p>
      ) : null}
      <div className="tree-imported__searchbar">
        <label className="tree-imported__search-field">
          <IconSearch size={14} />
          <input type="search" className="input tree-imported__search"
            placeholder={tr("sessionImport.searchPlaceholder")} value={query}
            onChange={(e) => setQuery(e.target.value)} aria-label={tr("sessionImport.searchPlaceholder")} />
        </label>
        <ImportIconButton label={tr(sort === "newest" ? "sessionImport.sortNewest" : "sessionImport.sortOldest")}
          onClick={() => setSort(sort === "newest" ? "oldest" : "newest")}>
          {sort === "newest" ? <IconChevronDown size={15} /> : <IconChevronUp size={15} />}
        </ImportIconButton>
      </div>
      {workspaceFilter}
      {providerOptions.length > 1 || provider ? (
        <div className="tree-imported__providers" role="group" aria-label={tr("sessionImport.importedGroup")}>
          <button
            type="button"
            className="btn btn--ghost btn--sm"
            aria-pressed={provider == null}
            data-selected={provider == null ? "1" : "0"}
            onClick={() => setProvider(null)}
          >
            {tr("sessionImport.providerAll")}
          </button>
          {providerOptions.map((p) => (
            <button
              key={p}
              type="button"
              className="btn btn--ghost btn--sm"
              aria-pressed={provider === p}
              data-selected={provider === p ? "1" : "0"}
              onClick={() => setProvider(p)}
            >
              {providerLabel(p, tr)}
            </button>
          ))}
        </div>
      ) : null}
      {notice === "copied" ? (
        <p role="status" className="tree-imported__notice">{tr("sessionImport.copiedExport")}</p>
      ) : null}
      {notice === "error" ? (
        <p role="alert" className="tree-imported__notice tree-imported__notice--error">{tr("sessionImport.err.generic")}</p>
      ) : null}
      {busy ? <p role="status" className="tree-imported__empty">{tr("sessionImport.workspaceFilter.loading")}</p> : null}
      {windowed ? <p className="tree-imported__empty">{tr("sessionImport.listWindow", { n: IMPORTED_LIST_WINDOW })}</p> : null}
      {visible.length === 0 && !busy && !error ? (
        <p className="tree-imported__empty">{tr(filtered ? "sessionImport.filterEmpty" : "sessionImport.noImported")}</p>
      ) : visible.length > 0 ? (
        <ul className="session-import__list tree-imported__list" aria-busy={busy}>
          {visible.map((e) => (
            <ImportedHistoryRow key={e.importId} entry={e} tr={tr} providerLabel={providerLabel(e.provider, tr)}
              exporting={exportingId === e.importId} exportBusy={exportingId !== null}
              onExport={(id) => void exportOne(id)} onOpen={onOpen} onContinue={onContinue} />
          ))}
        </ul>
      ) : null}
      {nextCursor ? (
        <button type="button" className="btn btn--ghost btn--sm" disabled={busy} onClick={() => void loadMore()}>
          {tr("sessionImport.loadMore")}
        </button>
      ) : null}
      {variant === "sidebar" ? (
        <div className="tree-imported__footer">
          <button
            type="button"
            className="btn btn--ghost btn--sm tree-imported__entry"
            aria-label={tr("sessionImport.entry")}
            onClick={() => onImport({ mode: "wizard" })}
          >
            <IconPlus size={15} />{tr("sessionImport.step.import")}
          </button>
          <ImportIconButton
            label={tr("sessionImport.fileMode")}
            onClick={() => onImport({ mode: "file" })}
          >
            <IconFileUp size={15} />
          </ImportIconButton>
        </div>
      ) : null}
    </div>
  );
}
