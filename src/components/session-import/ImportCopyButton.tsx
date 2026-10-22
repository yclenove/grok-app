import type { MessageKey, Vars } from "@/i18n";
import { useImportClipboard } from "@/hooks/useImportClipboard";

export function ImportCopyButton({ text, tr, labelKey = "sessionImport.copyText" }: {
  text: string;
  tr: (key: MessageKey, vars?: Vars) => string;
  labelKey?: MessageKey;
}) {
  const clipboard = useImportClipboard(text);
  return (
    <span className="import-copy-action">
      <button type="button" className="btn btn--sm btn--ghost"
        disabled={clipboard.status === "pending"} onClick={() => void clipboard.copy(text)}>
        {tr(labelKey)}
      </button>
      {clipboard.status === "copied" ? <span role="status">{tr("sessionImport.clipboardCopied")}</span> : null}
      {clipboard.status === "failed" ? <span role="alert">{tr("sessionImport.clipboardFailed")}</span> : null}
    </span>
  );
}
