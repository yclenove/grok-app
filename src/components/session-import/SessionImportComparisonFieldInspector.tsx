import { useEffect, useRef } from "react";
import { Select } from "@/components/Select";
import { useSessionImportComparisonField } from "@/hooks/useSessionImportComparisonField";
import type { ComparisonFieldContext } from "@/lib/sessionImport/comparisonFieldState";
import { importErrorKey } from "@/lib/sessionImport/errorCopy";
import { ImportCopyButton } from "./ImportCopyButton";
import type { SessionImportTFn } from "./sessionImportHostLabels";

export function SessionImportComparisonFieldInspector(props: {
  context: ComparisonFieldContext; fields: string[]; tr: SessionImportTFn; onClose: () => void;
}) {
  const { context, tr } = props;
  const reader = useSessionImportComparisonField(context);
  const { open } = reader;
  const contextKey = JSON.stringify(context);
  const focus = useRef<HTMLHeadingElement>(null);
  useEffect(() => { void open("kind"); focus.current?.focus(); }, [contextKey, open]);
  const page = reader.page;
  const sideLabel = tr(context.side === "left" ? "sessionImport.comparison.sideA" : "sessionImport.comparison.sideB");
  return <section className="session-import__comparison-inspector" aria-label={tr("sessionImport.comparison.inspect")}>
    <div className="session-import__toolbar">
      <h3 ref={focus} tabIndex={-1}>{sideLabel} · {tr("sessionImport.comparison.position", { n: context.position + 1 })}</h3>
      <button type="button" className="btn btn--ghost" onClick={props.onClose}>{tr("sessionImport.comparison.closeFields")}</button>
    </div>
    <p className="settings-row__desc">{tr("sessionImport.comparison.fieldNotice")}</p>
    <code>{context.eventId}</code>
    {reader.fieldNames.length ? <Select aria-label={tr("sessionImport.comparison.fieldLabel")} value={reader.field ?? "kind"}
      options={[...new Set([...(reader.field ? [reader.field] : []), ...props.fields, ...reader.fieldNames])].map((field) => ({ value: field, label: field }))}
      onChange={(field) => void open(field)} /> : null}
    {reader.loading ? <p role="status">{tr("sessionImport.peekLoading")}</p> : null}
    {reader.error ? <div role="alert"><p>{tr(importErrorKey(reader.error))}</p>
      <ImportCopyButton text={reader.error} tr={tr} labelKey="sessionImport.copyText" />
      <button type="button" className="btn" disabled={reader.loading} onClick={() => void reader.retry()}>{tr("sessionImport.comparison.retryField")}</button></div> : null}
    {page ? <>
      {page.available ? <>
        <p className="settings-row__desc">{tr("sessionImport.comparison.byteRange", { start: page.startByte, end: page.endByte, total: page.totalBytes })}</p>
        <pre className="session-import__comparison-field-text" aria-label={tr("sessionImport.comparison.jsonSegment")}><code>{page.text}</code></pre>
        <ImportCopyButton text={page.text} tr={tr} labelKey="sessionImport.comparison.copySegment" />
        <details><summary>{tr("sessionImport.comparison.references")}</summary><code>{page.fieldsDigest}</code><br /><code>{page.fieldDigest}</code></details>
      </> : <p role="status">{tr("sessionImport.comparison.fieldUnavailable")}</p>}
      <div className="session-import__toolbar">
        <button type="button" className="btn btn--sm" disabled={reader.loading || !!reader.error || !reader.pageIndex}
          onClick={() => void reader.previousPage()}>{tr("sessionImport.previousPage")}</button>
        <button type="button" className="btn btn--sm" disabled={reader.loading || !!reader.error || !page.nextCursor}
          onClick={() => void reader.nextPage()}>{tr("sessionImport.nextPage")}</button>
      </div>
    </> : null}
  </section>;
}
