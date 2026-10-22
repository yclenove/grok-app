import { useState } from "react";
import { Select, type SelectOption } from "@/components/Select";
import { createT, intlLocale, type Locale } from "@/i18n";
import { useImportedRevisions } from "@/hooks/useImportedRevisions";
import { openImportedHistory } from "@/lib/sessionImport/open";
import { importErrorKey } from "@/lib/sessionImport/errorCopy";

type Props = { importId: string; revision: string | null; locale: Locale };

function RevisionChoices({ importId, revision, locale }: Props) {
  const tr = createT(locale);
  const ui = useImportedRevisions(importId);
  const date = new Intl.DateTimeFormat(intlLocale(locale), { dateStyle: "medium", timeStyle: "short" });
  const choices: SelectOption[] = (ui.page?.items ?? []).map((item) => {
    const time = new Date(item.createdAt);
    const shortId = item.revisionId.slice(-8);
    const ambiguous = ui.page!.items.some((other) => other.revisionId !== item.revisionId && other.revisionId.endsWith(shortId));
    const label = `${Number.isNaN(time.getTime()) ? item.createdAt : date.format(time)} · ${ambiguous ? item.revisionId : shortId}`;
    return { value: item.revisionId, label: item.revisionId === ui.page?.currentRevision
      ? tr("sessionImport.revisions.current", { label }) : label };
  });
  if (revision && !choices.some((item) => item.value === revision)) {
    choices.unshift({ value: revision, label: tr("sessionImport.revisions.viewing", { revision: revision.slice(-8) }), disabled: true });
  }
  return <>
    {ui.busy ? <p role="status">{tr("sessionImport.revisions.loading")}</p> : null}
    {ui.error ? <p role="alert">{tr(importErrorKey(ui.error))}</p> : null}
    {ui.page ? <>
      <p className="settings-row__desc">{tr("sessionImport.revisions.count", { n: ui.page.total })}</p>
      <Select aria-label={tr("sessionImport.revisions.title")} title={revision ?? ui.page.currentRevision} value={revision ?? ui.page.currentRevision} options={choices}
        disabled={ui.busy || !!ui.error} onChange={(selected) => {
          if (selected !== revision && ui.canChoose(selected)) openImportedHistory({ importId, revision: selected });
        }} />
    </> : null}
    <div className="session-import__toolbar">
      {ui.page ? <>
        <button type="button" className="btn btn--sm" disabled={ui.busy || !ui.page.previousCursor}
          onClick={() => ui.move("previous")}>{tr("sessionImport.previousPage")}</button>
        <button type="button" className="btn btn--sm" disabled={ui.busy || !ui.page.nextCursor}
          onClick={() => ui.move("next")}>{tr("sessionImport.nextPage")}</button>
      </> : null}
      {ui.error ? <button type="button" className="btn btn--sm" disabled={ui.busy}
        onClick={ui.retry}>{tr("sessionImport.revisions.retry")}</button> : null}
      <button type="button" className="btn btn--sm" disabled={ui.busy}
        onClick={ui.refresh}>{tr("sessionImport.refresh")}</button>
    </div>
  </>;
}

export function ImportedRevisionPicker(props: Props) {
  const [expanded, setExpanded] = useState(false);
  const tr = createT(props.locale);
  return <details className="session-import__group" data-import-revisions="1"
    onToggle={(event) => setExpanded(event.currentTarget.open)} onKeyDown={(event) => {
      if (event.key === "n" || event.key === "p") event.stopPropagation();
    }}>
    <summary>{tr("sessionImport.revisions.title")}</summary>
    {expanded ? <RevisionChoices {...props} /> : null}
  </details>;
}
