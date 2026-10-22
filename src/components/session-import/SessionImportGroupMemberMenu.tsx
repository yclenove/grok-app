import { useCallback, useEffect, useRef, useState, type RefObject } from "react";
import { createPortal } from "react-dom";
import { useSessionImportGroupMembers } from "@/hooks/useSessionImportGroupMembers";
import type { SourceGroup } from "@/lib/api/sessionMigrationGroups";
import type { ImportGroupScope } from "@/lib/sessionImport/groupSelectionState";
import { installDialogFocus, isDialogFocusAllowed, listFocusable } from "@/lib/a11yFocus";
import { useFloatingMenu } from "@/lib/floatingMenu";
import { importErrorKey } from "@/lib/sessionImport/errorCopy";
import type { SessionImportTFn } from "./sessionImportHostLabels";
import { sourceGroupFormatKeys as formatKeys } from "./sessionImportGroupFormatKeys";

/** One mounted menu owns one lazy group; changing scope remounts its request owner. */
export function SessionImportGroupMemberMenu(props: {
  scope: ImportGroupScope;
  group: SourceGroup;
  candidateId: string;
  disabled: boolean;
  busy: boolean;
  tr: SessionImportTFn;
  triggerRef: RefObject<HTMLButtonElement | null>;
  menuId: string;
  onClose: () => void;
  onChoose: (groupId: string, candidateId: string) => Promise<boolean>;
  onRefresh: () => void | Promise<unknown>;
}) {
  const members = useSessionImportGroupMembers();
  const panelRef = useRef<HTMLDivElement>(null);
  const seed = useRef({ scope: { ...props.scope }, group: props.group });
  const owner = useRef(0);
  const flight = useRef(false);
  const [phase, setPhase] = useState<"choose" | "refresh" | null>(null);
  const [needsReview, setNeedsReview] = useState(false);
  const restore = useRef(true);
  const tabDirection = useRef<-1 | 1 | null>(null);
  const closeRef = useRef(props.onClose);
  closeRef.current = props.onClose;
  const close = useCallback(() => closeRef.current(), []);
  useEffect(() => {
    void members.open(seed.current.scope, seed.current.group);
    return () => { ++owner.current; flight.current = false; };
  }, [members.open]);
  const { pos, style } = useFloatingMenu({ open: true, triggerRef: props.triggerRef, panelRef,
    onClose: () => { restore.current = false; close(); }, closeOnEscape: false,
    align: "end", fitContent: false, width: 420, estHeight: 340,
    deps: [members.members.length, members.loading, members.error, phase, needsReview] });
  const ready = !!pos;
  useEffect(() => {
    if (!ready) return;
    return installDialogFocus(() => panelRef.current, {
      initialFocus: () => panelRef.current?.querySelector<HTMLElement>("[aria-checked='true']:not([disabled])") ??
        listFocusable(panelRef.current)[0] ?? panelRef.current,
      onEscape: close,
      restoreFocus: () => {
        const trigger = props.triggerRef.current;
        const target = trigger?.disabled ? trigger.closest<HTMLElement>("[data-import-candidate]") : trigger;
        if (!restore.current || !target?.isConnected || !isDialogFocusAllowed(target)) return false;
        const dialog = target?.closest("[role='dialog'], [role='alertdialog']");
        const direction = tabDirection.current;
        const controls = direction && dialog ? listFocusable(dialog) : [];
        const index = controls.indexOf(target);
        // Tab's default was cancelled in the portal: advance explicitly after
        // its focus scope is removed, not just the parent trap's edge wrapping.
        const next = direction && controls.length
          ? controls[index < 0 ? (direction === 1 ? 0 : controls.length - 1)
            : (index + direction + controls.length) % controls.length] : target;
        next?.focus({ preventScroll: true });
        return false;
      },
      onTab: (event) => {
        event.preventDefault(); event.stopPropagation();
        tabDirection.current = event.shiftKey ? -1 : 1;
        close();
      },
    });
  }, [close, props.triggerRef, ready]);
  useEffect(() => {
    if (!ready || !panelRef.current || !isDialogFocusAllowed(panelRef.current)) return;
    const active = document.activeElement;
    if (active === document.body || active === panelRef.current ||
      (active instanceof HTMLElement && panelRef.current.contains(active) && active.hasAttribute("disabled"))) {
      (listFocusable(panelRef.current)[0] ?? panelRef.current).focus();
    }
  }, [ready, members.members, members.loading, phase, props.disabled]);

  const choose = async (candidateId: string) => {
    if (flight.current || props.disabled || members.loading || !members.ready) return;
    const request = owner.current;
    flight.current = true; setPhase("choose"); setNeedsReview(false);
    try {
      const accepted = await props.onChoose(props.group.groupId, candidateId);
      if (request !== owner.current) return;
      if (accepted) close(); else setNeedsReview(true);
    } catch { if (request === owner.current) setNeedsReview(true); }
    finally { if (request === owner.current) { flight.current = false; setPhase(null); } }
  };
  const refresh = async () => {
    if (flight.current || props.busy) return;
    const request = owner.current;
    flight.current = true; setPhase("refresh");
    try { const accepted = await props.onRefresh(); if (request === owner.current) setNeedsReview(accepted === false); }
    catch { if (request === owner.current) setNeedsReview(true); }
    finally { if (request === owner.current) { flight.current = false; setPhase(null); } }
  };
  return createPortal(<div ref={panelRef} id={props.menuId} role="menu" tabIndex={-1}
    aria-label={props.tr("sessionImport.versions.title")} aria-busy={members.loading || !!phase}
    className="menu-panel context-menu session-import__versions-menu" style={style}
    onMouseDown={(event) => event.stopPropagation()}
    onKeyDown={(event) => {
      if (event.nativeEvent.isComposing || event.nativeEvent.keyCode === 229) {
        if (event.key === "Enter" || event.key === " ") event.preventDefault();
        return;
      }
      if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
      event.preventDefault(); event.stopPropagation();
      const enabled = listFocusable(panelRef.current);
      if (!enabled.length) return;
      const index = enabled.indexOf(document.activeElement as HTMLElement);
      const next = event.key === "Home" ? 0 : event.key === "End" ? enabled.length - 1
        : event.key === "ArrowDown" ? (index + 1) % enabled.length : (index - 1 + enabled.length) % enabled.length;
      enabled[next]?.focus(); enabled[next]?.scrollIntoView?.({ block: "nearest" });
    }}>
    <p className="session-import__versions-heading">{props.tr("sessionImport.versions.title")}</p>
    {members.loading || phase ? <p role="status">{props.tr(phase === "choose" ? "sessionImport.versions.changing" : "sessionImport.versions.loading")}</p> : null}
    {members.error ? <p role="alert">{props.tr(importErrorKey(members.error))}</p> : null}
    {needsReview ? <p role="alert">{props.tr("sessionImport.versions.review")}</p> : null}
    {members.ready && !members.members.length && !members.loading ? <p role="status">{props.tr("sessionImport.versions.empty")}</p> : null}
    {members.members.map((member) => <button key={member.candidate.candidateId} type="button" role="menuitemradio"
      aria-checked={member.candidate.candidateId === props.candidateId}
      disabled={props.disabled || members.loading || !!phase} className="context-menu__item session-import__version-item"
      onClick={() => void choose(member.candidate.candidateId)}>
      <span className="session-import__version-copy">
        <span>{props.tr(formatKeys[member.format])} · {member.candidate.title}</span>
        <span className="session-import__version-path" title={member.rootPath}>{member.rootPath}</span>
        <span className="session-import__version-path" title={member.candidate.locator}>{member.candidate.locator}</span>
        {member.candidate.candidateId === props.group.firstMember.candidateId ? <span className="session-import__badge">{props.tr("sessionImport.versions.first")}</span> : null}
      </span>
    </button>)}
    <div className="session-import__versions-actions">
      {members.error ? <button type="button" role="menuitem" className="btn btn--sm" disabled={members.loading || !!phase} onClick={() => void members.retry()}>{props.tr("sessionImport.versions.retry")}</button> : null}
      {needsReview ? <button type="button" role="menuitem" className="btn btn--sm" disabled={props.busy || !!phase} onClick={() => void refresh()}>{props.tr("sessionImport.refresh")}</button> : null}
      {members.pageIndex > 0 ? <button type="button" role="menuitem" className="btn btn--sm" disabled={members.loading || !!phase} onClick={() => void members.previousPage()}>{props.tr("sessionImport.previousPage")}</button> : null}
      {members.nextCursor ? <button type="button" role="menuitem" className="btn btn--sm" disabled={members.loading || !!phase || !members.members.length} onClick={() => void members.nextPage()}>{props.tr("sessionImport.nextPage")}</button> : null}
      <button type="button" role="menuitem" className="btn btn--sm btn--ghost" onClick={close}>{props.tr("common.close")}</button>
    </div>
  </div>, document.body);
}
