import type { KeyboardEventHandler } from "react";
import type { MessageKey, Vars } from "@/i18n";
import type { ImportClipboardStatus } from "@/hooks/useImportClipboard";
import { importErrorKey } from "@/lib/sessionImport/errorCopy";

type Props = {
  tr: (key: MessageKey, vars?: Vars) => string;
  busy: boolean;
  continueBusy: boolean;
  hasPrevious: boolean;
  hasNext: boolean;
  hasText: boolean;
  hasIssue: boolean;
  copyStatus: ImportClipboardStatus;
  exportNotice: string | null;
  exportError: string | null;
  onPage: (direction: "previous" | "next") => void;
  onCopy: () => void;
  onIssue: () => void;
  onExport: () => void;
  onContinue: () => void;
  onKeyDown: KeyboardEventHandler<HTMLDivElement>;
};

/** Kept in GlassModal's footer so long transcripts cannot push actions away. */
export function ImportedReaderActions(props: Props) {
  const { tr } = props;
  return (
    <div className="import-reader-actions" onKeyDown={props.onKeyDown}>
      {props.hasPrevious || props.hasNext || props.hasIssue ? (
        <div className="import-reader-actions__navigation">
          <div className="import-reader-actions__group">
            {props.hasPrevious ? <button type="button" className="btn btn--ghost btn--sm" disabled={props.busy} onClick={() => props.onPage("previous")}>{tr("sessionImport.loadPrevious")}</button> : null}
            {props.hasNext ? <button type="button" className="btn btn--ghost btn--sm" disabled={props.busy} onClick={() => props.onPage("next")}>{tr("sessionImport.loadMore")}</button> : null}
          </div>
          <button type="button" className="btn btn--ghost btn--sm" disabled={!props.hasIssue} onClick={props.onIssue}>{tr("sessionImport.firstIssue")}</button>
        </div>
      ) : null}
      <div className="import-reader-actions__main">
        <div className="import-reader-actions__group">
          <button type="button" className="btn btn--ghost btn--sm" disabled={!props.hasText || props.copyStatus === "pending"} onClick={props.onCopy}>{tr("sessionImport.copyVisible")}</button>
          <button type="button" className="btn btn--ghost btn--sm" disabled={props.busy} onClick={props.onExport}>{tr("sessionImport.exportMarkdown")}</button>
        </div>
        <button type="button" className="btn btn--primary" disabled={props.continueBusy} onClick={props.onContinue}>{tr("sessionImport.continueInGrok")}</button>
      </div>
      {props.copyStatus === "copied" ? <p role="status">{tr("sessionImport.clipboardCopied")}</p> : null}
      {props.copyStatus === "failed" ? <p role="alert">{tr("sessionImport.clipboardFailed")}</p> : null}
      {props.exportNotice === "copied" ? <p role="status">{tr("sessionImport.copiedExport")}</p> : null}
      {props.exportNotice === "copyFailed" ? <p role="alert">{tr("sessionImport.clipboardFailed")}</p> : null}
      {props.exportError ? <p role="alert">{tr(importErrorKey(props.exportError))}</p> : null}
    </div>
  );
}
