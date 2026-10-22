import type { MessageKey } from "@/i18n";
import type { ReactNode } from "react";
import type { SourceCandidate } from "@/lib/api/sessionMigration";
import type { SessionImportTFn } from "./sessionImportHostLabels";
import { ImportCopyButton } from "./ImportCopyButton";

export function SessionImportCandidateRow({
  candidate: c,
  rowId = c.candidateId,
  selected,
  focused,
  peekBusy,
  disabled,
  versionPicker,
  tr,
  onFocus,
  onToggle,
  onPeek,
}: {
  candidate: SourceCandidate;
  rowId?: string;
  selected: boolean;
  focused?: boolean;
  peekBusy: boolean;
  disabled?: boolean;
  versionPicker?: ReactNode;
  tr: SessionImportTFn;
  onFocus: () => void;
  onToggle: () => void;
  onPeek: () => void;
}) {
  const completenessKey: MessageKey | null =
    c.completeness === "prefix"
      ? "sessionImport.completenessPrefix"
      : c.completeness === "body"
        ? "sessionImport.completenessBody"
        : null;
  const when = (c.updatedAt || c.startedAt || "").slice(0, 10);
  const providerKey: MessageKey | null = c.provider === "codex"
    ? "sessionImport.source.codex"
    : c.provider === "claude-code" ? "sessionImport.source.claude"
      : c.provider === "cursor" ? "sessionImport.source.cursor"
        : c.provider === "generic-file" ? "sessionImport.source.file" : null;
  return (
    <li
      className="session-import__candidate"
      data-import-candidate={rowId}
      data-import-preview-candidate={c.candidateId}
      data-focused={focused ? "1" : "0"}
      data-selected={selected ? "1" : "0"}
      tabIndex={focused ? 0 : -1}
      onFocus={onFocus}
    >
      <label className="session-import__candidate-select">
        <input type="checkbox" checked={selected} disabled={disabled} onChange={onToggle} />
        <span className="session-import__candidate-copy">
          <span className="session-import__candidate-title">{c.title}</span>
          <span className="session-import__candidate-meta">
            <span>{providerKey ? tr(providerKey) : c.provider}</span>
            {when ? <time dateTime={when}>{when}</time> : null}
          </span>
          {c.cwd ? <span className="session-import__locator" title={c.cwd}>{c.cwd}</span> : null}
          {c.locator ? (
            <span className="session-import__locator" title={c.locator}>
              {tr("sessionImport.viewSource")}: {c.locator}
            </span>
          ) : null}
        </span>
      </label>
      <div className="session-import__candidate-footer">
        <span className="session-import__candidate-badges">
          {c.archived ? (
            <span className="session-import__badge">{tr("sessionImport.archived")}</span>
          ) : null}
          {completenessKey ? (
            <span
              className="session-import__badge"
              data-kind={c.completeness === "prefix" ? "partial" : "ok"}
            >
              {tr(completenessKey)}
            </span>
          ) : null}
        </span>
        <span className="session-import__candidate-actions">
          {versionPicker}
          <button
            type="button"
            className="btn btn--sm btn--ghost"
            data-import-peek="1"
            disabled={disabled || peekBusy}
            onClick={onPeek}
          >
            {tr("sessionImport.preview")}
          </button>
          {c.locator ? (
            <ImportCopyButton text={c.locator} tr={tr} labelKey="sessionImport.copySource" />
          ) : null}
        </span>
      </div>
    </li>
  );
}
