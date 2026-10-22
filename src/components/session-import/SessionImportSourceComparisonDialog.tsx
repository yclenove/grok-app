import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { GlassModal } from "@/components/GlassModal";
import { Select } from "@/components/Select";
import { useSessionImportGroupMembers } from "@/hooks/useSessionImportGroupMembers";
import { useSessionImportComparisonPreviews } from "@/hooks/useSessionImportComparisonPreviews";
import { useSessionImportComparisonPrimaryMember } from "@/hooks/useSessionImportComparisonPrimaryMember";
import { useSessionImportSourceComparison } from "@/hooks/useSessionImportSourceComparison";
import type { SourceComparisonEvent } from "@/lib/api/sessionMigrationComparison";
import type { ComparisonFieldContext } from "@/lib/sessionImport/comparisonFieldState";
import { comparisonContextKey, type ComparisonPreviewContext, type ComparisonPreviewSlot, type ComparisonSideName } from "@/lib/sessionImport/comparisonPreviewState";
import { importErrorKey } from "@/lib/sessionImport/errorCopy";
import { ImportCopyButton } from "./ImportCopyButton";
import { sourceGroupFormatKeys } from "./sessionImportGroupFormatKeys";
import type { SessionImportTFn } from "./sessionImportHostLabels";
import { SessionImportComparisonFieldInspector } from "./SessionImportComparisonFieldInspector";
import type { useSessionImportComparisonDialog } from "@/hooks/useSessionImportComparisonDialog";

function ignoreImeActivation(event: KeyboardEvent) {
  if ((event.nativeEvent.isComposing || event.nativeEvent.keyCode === 229) &&
    (event.key === "Enter" || event.key === " ")) {
    event.preventDefault(); event.stopPropagation();
  }
}

function ComparisonEvent(props: { event: SourceComparisonEvent | null; label: string; tr: SessionImportTFn; disabled: boolean; onInspect: () => void }) {
  const { event, label, tr } = props;
  return <section className="session-import__comparison-event" aria-label={label}>
    <h4>{label}</h4>
    {event ? <>
    <code>{event.kind}</code>
    <p className="session-import__comparison-text">{event.excerpt}</p>
    {event.truncated ? <p className="settings-row__desc">{tr("sessionImport.comparison.excerpt")}</p> : null}
    <button type="button" className="btn btn--sm btn--ghost" disabled={props.disabled} onKeyDown={ignoreImeActivation} onClick={props.onInspect}>{tr("sessionImport.comparison.inspect")}</button>
    <p className="settings-row__desc">{tr("sessionImport.comparison.sourceTime")}: <code>{event.timestamp ?? "—"}</code></p>
    <details>
      <summary>{tr("sessionImport.comparison.references")}</summary>
      <code>{event.eventId}</code>
      <ul>{event.spans.map((span, index) => <li key={index}><code>{span.artifactId} · {span.record}{span.block === undefined ? "" : ` · ${span.block}`}</code></li>)}</ul>
      <code>{event.fieldsDigest}</code>
    </details>
    </> : <p className="settings-row__desc">{tr("sessionImport.comparison.missing")}</p>}
  </section>;
}

type AdoptionControls = Pick<ReturnType<typeof useSessionImportComparisonDialog>, "locked" | "isLocked" | "busy" | "phase" |
  "error" | "canRecompare" | "requestAdoption" | "retry" | "recompare">;

/** Pair-owned reading and deliberate adoption; merely opening never edits import selection. */
export function SessionImportSourceComparisonDialog(props: {
  context: ComparisonPreviewContext;
  tr: SessionImportTFn;
  onClose: () => void;
  adoption?: AdoptionControls;
}) {
  const { context, tr } = props;
  const locked = props.adoption?.locked ?? false;
  const mutate = (action: () => unknown) => {
    if (!props.adoption?.isLocked()) void action();
  };
  const [inspector, setInspector] = useState<{ context: ComparisonFieldContext; fields: string[]; pageKey: string; trigger: HTMLButtonElement | null } | null>(null);
  const previews = useSessionImportComparisonPreviews(context);
  const primaryMetadata = useSessionImportComparisonPrimaryMember(context);
  const members = useSessionImportGroupMembers();
  const comparison = useSessionImportSourceComparison(context.scope);
  const { start } = previews;
  const { load: loadPrimaryMetadata } = primaryMetadata;
  const { open: openMembers } = members;
  const { getPair } = previews;
  const { open: openComparison, close: closeComparison } = comparison;
  const contextKey = comparisonContextKey(context);
  const committedContext = useRef(context);
  useLayoutEffect(() => { committedContext.current = context; });
  const pairKey = JSON.stringify(previews.pair);
  useEffect(() => {
    const context = committedContext.current;
    void start();
    void loadPrimaryMetadata();
    void openMembers(context.scope, context.group);
  }, [start, loadPrimaryMetadata, openMembers, contextKey]);
  useLayoutEffect(() => {
    const pair = getPair();
    if (pair) void openComparison(context.group.groupId, pair.left, pair.right);
    else closeComparison();
  }, [pairKey, getPair, context.group.groupId, openComparison, closeComparison]);
  const visiblePage = comparison.page && comparison.owner && JSON.stringify({
    left: comparison.owner.left, right: comparison.owner.right,
  }) === pairKey ? comparison.page : null;
  const rowPageKey = JSON.stringify([contextKey, pairKey, visiblePage?.rows.map((row) => row.position)]);
  useLayoutEffect(() => { setInspector(null); }, [rowPageKey, locked]);
  const inspect = (position: number, side: ComparisonSideName, event: SourceComparisonEvent | null) => {
    if (props.adoption?.isLocked() || !comparison.owner || !event) return;
    setInspector({ pageKey: rowPageKey, fields: visiblePage?.rows.find((row) => row.position === position)?.differences.filter((field) => field !== "missing-side") ?? [],
      trigger: document.activeElement instanceof HTMLButtonElement ? document.activeElement : null,
      context: { owner: comparison.owner, side, position, eventId: event.eventId, fieldsDigest: event.fieldsDigest } });
  };
  const sideCard = (side: ComparisonSideName, slot: ComparisonPreviewSlot | null) => {
    const metadata = side === "left" ? primaryMetadata.member : slot?.member;
    const summary = visiblePage?.[side];
    const canAdopt = !!props.adoption && !locked && !comparison.loading && !comparison.error &&
      !!slot?.previewId && !!slot.leafId && !slot.loading && !slot.error && !!slot.adapter && !!metadata &&
      (side !== "left" || (!primaryMetadata.loading && !primaryMetadata.error)) &&
      summary?.previewId === slot.previewId && summary?.leafId === slot.leafId &&
      summary?.candidateId === metadata.candidate.candidateId && summary?.sourceRevision === metadata.candidate.sourceRevision;
    return <section className="session-import__comparison-source" aria-label={tr(side === "left" ? "sessionImport.comparison.sideA" : "sessionImport.comparison.sideB")}>
      <h3>{tr(side === "left" ? "sessionImport.comparison.sideA" : "sessionImport.comparison.sideB")}</h3>
      {slot ? <>
        <p>{slot.candidate.title}</p>
        {side === "left" && primaryMetadata.loading ? <p role="status">{tr("sessionImport.versions.loading")}</p> : null}
        {side === "left" && primaryMetadata.error ? <div role="alert"><p>{tr(importErrorKey(primaryMetadata.error))}</p>
          <ImportCopyButton text={primaryMetadata.error} tr={tr} labelKey="sessionImport.copyText" />
          <button type="button" className="btn btn--sm" disabled={locked || primaryMetadata.loading} onClick={() => mutate(loadPrimaryMetadata)}>{tr("sessionImport.versions.retry")}</button></div> : null}
        {metadata ? <p className="settings-row__desc">{tr(sourceGroupFormatKeys[metadata.format])} · <code>{metadata.rootPath}</code></p> : null}
        <p className="session-import__locator" title={slot.candidate.locator}>{slot.candidate.locator}</p>
        <ImportCopyButton text={slot.candidate.locator} tr={tr} labelKey="sessionImport.copySource" />
        {slot.loading ? <p role="status">{tr("sessionImport.peekLoading")}</p> : null}
        {slot.error ? <div role="alert"><p>{tr(importErrorKey(slot.error))}</p>
          <ImportCopyButton text={slot.error} tr={tr} labelKey="sessionImport.copyText" />
          <button type="button" className="btn btn--sm" disabled={locked || slot.loading} onClick={() => mutate(() => previews.retry(side))}>{tr("sessionImport.comparison.retryPreview")}</button></div> : null}
        {slot.summary && !slot.leafId ? <p role="status">{tr("sessionImport.comparison.empty")}</p> : null}
        {slot.summary && slot.leafId ? <Select value={slot.leafId} options={slot.summary.leaves.map((leaf) => ({ value: leaf, label: leaf }))}
          aria-label={tr("sessionImport.selectLeaf")} disabled={locked || slot.loading || comparison.loading}
          onChange={(leaf) => mutate(() => previews.chooseLeaf(side, leaf))} /> : null}
        {props.adoption ? <button type="button" className="btn btn--primary" disabled={!canAdopt}
          onKeyDown={ignoreImeActivation} onClick={() => {
            if (!canAdopt || props.adoption?.isLocked() || !metadata || !summary || !slot.previewId) return;
            props.adoption?.requestAdoption({ context, member: metadata,
              preview: { previewId: slot.previewId, adapter: slot.adapter }, comparison: summary });
          }}>{tr(side === "left" ? "sessionImport.comparison.useA" : "sessionImport.comparison.useB")}</button> : null}
      </> : <p>{tr("sessionImport.comparison.waitingB")}</p>}
    </section>;
  };
  return <GlassModal open onClose={props.onClose} title={tr("sessionImport.comparison.title")}
    size="lg" wrapBody className="session-import-modal session-import-comparison" closeLabel={tr("sessionImport.close")}
    footer={<>
      <p role="status">{tr("sessionImport.page", { n: comparison.pageIndex + 1 })}</p>
      <button type="button" className="btn btn--ghost" disabled={locked || !visiblePage || comparison.loading || !!comparison.error || comparison.pageIndex === 0}
        onClick={() => mutate(comparison.previousPage)}>{tr("sessionImport.previousPage")}</button>
      <button type="button" className="btn btn--ghost" disabled={locked || !visiblePage?.nextCursor || comparison.loading || !!comparison.error}
        onClick={() => mutate(comparison.nextPage)}>{tr("sessionImport.nextPage")}</button>
      <button type="button" className="btn" onClick={props.onClose}>{tr("sessionImport.close")}</button>
    </>}>
    <p className="settings-row__desc">{tr("sessionImport.comparison.notice")}</p>
    {props.adoption && (props.adoption.busy || props.adoption.error) ? <div
      className="session-import__comparison-status" role={props.adoption.error ? "alert" : "status"}>
      <p>{tr(props.adoption.busy ? "sessionImport.comparison.adopting" :
        props.adoption.phase === "reconcile-required" ? "sessionImport.comparison.reconcileRequired" :
        props.adoption.phase === "uncertain" ? "sessionImport.comparison.uncertainAdoption" :
        props.adoption.canRecompare ? "sessionImport.comparison.staleAdoption" : "sessionImport.comparison.adoptionFailed")}</p>
      {props.adoption.error ? <ImportCopyButton text={props.adoption.error} tr={tr} labelKey="sessionImport.copyText" /> : null}
      {props.adoption.phase === "reconcile-required" ? <button type="button" className="btn btn--sm"
        disabled={props.adoption.busy} onKeyDown={ignoreImeActivation} onClick={props.adoption.retry}>{tr("sessionImport.comparison.retryRead")}</button> : null}
      {props.adoption.canRecompare ? <button type="button" className="btn btn--sm"
        disabled={props.adoption.busy} onKeyDown={ignoreImeActivation} onClick={() => void props.adoption?.recompare()}>{tr("sessionImport.comparison.recompare")}</button> : null}
      {props.adoption.error ? <details><summary>{tr("error.details")}</summary><pre>{props.adoption.error}</pre></details> : null}
    </div> : null}
    {previews.error ? <div role="alert"><p>{tr(importErrorKey(previews.error))}</p>
      <ImportCopyButton text={previews.error} tr={tr} labelKey="sessionImport.copyText" /></div> : null}
    <div className="session-import__comparison-pair">{sideCard("left", previews.left)}{sideCard("right", previews.right)}</div>
    <section aria-label={tr("sessionImport.comparison.selectB")} className="session-import__comparison-versions">
      <h3>{tr("sessionImport.comparison.selectB")}</h3>
      {members.loading ? <p role="status">{tr("sessionImport.versions.loading")}</p> : null}
      {members.error ? <div role="alert"><p>{tr(importErrorKey(members.error))}</p>
        <ImportCopyButton text={members.error} tr={tr} labelKey="sessionImport.copyText" />
        <button type="button" className="btn" disabled={locked || members.loading} onClick={() => mutate(members.retry)}>{tr("sessionImport.versions.retry")}</button></div> : null}
      {members.ready && !members.members.length && !members.loading ? <p role="status">{tr("sessionImport.versions.empty")}</p> : null}
      <ul>{members.members.map((member) => <li key={member.candidate.candidateId}>
        <button type="button" className="btn btn--ghost" aria-pressed={previews.right?.candidate.candidateId === member.candidate.candidateId}
          disabled={locked || members.loading || previews.right?.loading || member.candidate.candidateId === context.primary.candidateId}
          onKeyDown={ignoreImeActivation} onClick={() => mutate(() => previews.chooseRight(member))}>
          <span>{tr(sourceGroupFormatKeys[member.format])} · {member.candidate.title}</span>
          <code>{member.candidate.locator}</code>
        </button></li>)}</ul>
      <div className="session-import__toolbar">
        <button type="button" className="btn btn--sm" disabled={locked || members.loading || members.pageIndex === 0} onClick={() => mutate(members.previousPage)}>{tr("sessionImport.previousPage")}</button>
        <button type="button" className="btn btn--sm" disabled={locked || members.loading || !members.nextCursor || !members.members.length} onClick={() => mutate(members.nextPage)}>{tr("sessionImport.nextPage")}</button>
      </div>
    </section>
    {comparison.loading ? <p role="status">{tr("sessionImport.peekLoading")}</p> : null}
    {comparison.error ? <div role="alert"><p>{tr(importErrorKey(comparison.error))}</p>
      <ImportCopyButton text={comparison.error} tr={tr} labelKey="sessionImport.copyText" />
      <button type="button" className="btn" disabled={locked || comparison.loading} onClick={() => mutate(comparison.retry)}>{tr("sessionImport.comparison.retry")}</button></div> : null}
    {visiblePage ? <>
      {visiblePage.metadataDifferences.length ? <p>{tr("sessionImport.comparison.metadata", { fields: visiblePage.metadataDifferences.join(", ") })}</p> : null}
      {visiblePage.uncertain ? <p role="status">{tr("sessionImport.comparison.uncertain")}</p> : null}
      {!locked && inspector?.pageKey === rowPageKey ? <SessionImportComparisonFieldInspector key={JSON.stringify(inspector.context)} context={inspector.context} fields={inspector.fields} tr={tr}
        onClose={() => { const trigger = inspector.trigger; setInspector(null); if (trigger?.isConnected) trigger.focus(); }} /> : null}
      <ol className="session-import__comparison-rows">
        {visiblePage.rows.map((row) => <li key={row.position}>
          <h3>{tr("sessionImport.comparison.position", { n: row.position + 1 })}</h3>
          <p>{row.differences.length ? tr("sessionImport.comparison.fields", { fields: row.differences.join(", ") }) : tr("sessionImport.comparison.noDifferences")}</p>
          {row.metadataDifferences.length ? <p className="settings-row__desc">{tr("sessionImport.comparison.metadata", { fields: row.metadataDifferences.join(", ") })}</p> : null}
          {row.uncertain ? <p className="settings-row__desc">{tr("sessionImport.comparison.uncertain")}</p> : null}
          <div className="session-import__comparison-pair">
            <ComparisonEvent event={row.left} label={tr("sessionImport.comparison.sideA")} tr={tr} disabled={locked} onInspect={() => inspect(row.position, "left", row.left)} />
            <ComparisonEvent event={row.right} label={tr("sessionImport.comparison.sideB")} tr={tr} disabled={locked} onInspect={() => inspect(row.position, "right", row.right)} />
          </div>
        </li>)}
      </ol>
    </> : null}
  </GlassModal>;
}
