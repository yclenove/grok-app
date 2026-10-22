import { useCallback, useEffect, useRef, useState, useSyncExternalStore, type Dispatch, type RefObject, type SetStateAction } from "react";
import type { createT } from "@/i18n";
import * as api from "@/lib/api";
import type { AppDialog } from "@/lib/app/appDialogTypes";
import type { SessionRow } from "@/lib/app/sidebarModels";
import { planDisplayMarkdown } from "@/lib/planBody";
import { recordPlanHistory } from "@/lib/planHistory";
import { shouldExitComposerPlanModeAfterDecision } from "@/lib/planModePro";
import { getPlanReplyClaims, samePlanReplyTarget, type PlanReplyOwner } from "@/lib/planReplyClaims";
import { closedSessionPlan, planStateToStored, type SessionPlanState } from "@/lib/planSession";
import type { SideWorkbenchState } from "@/lib/sideWorkbench";
import { isSameView, type ViewFocus } from "@/lib/viewFocus";

type Decision = "approved" | "cancelled" | "abandoned";
export type PlanResolutionContext = {
  plan: SessionPlanState;
  planRef: RefObject<SessionPlanState>;
  planBySessionRef: RefObject<Map<string, SessionPlanState>>;
  viewingSessionIdRef: RefObject<string | null>;
  currentViewFocus: () => ViewFocus;
  setPlan: Dispatch<SetStateAction<SessionPlanState>>;
  markPlanPendingBadge: (sessionId: string, plan: SessionPlanState) => void;
  sessionsRef: RefObject<SessionRow[]>;
  projectId: string | null;
  mode: string;
  goalMode: boolean;
  setMode: (mode: "agent") => void;
  setGoalMode: (value: boolean) => void;
  sideWorkbench: SideWorkbenchState;
  asideCollapsed: boolean;
  planOpenedAsideRef: RefObject<boolean>;
  collapseAsidePersisted: () => void;
  setAppDialog: Dispatch<SetStateAction<AppDialog>>;
  showToast: (message: string, duration: number) => void;
  tr: ReturnType<typeof createT>;
};
type Target = PlanReplyOwner & {
  cache: Map<string, SessionPlanState>;
  focus: ViewFocus;
  projectId: string | null;
  mode: string;
  goalMode: boolean;
  composerRevision: number;
  asideRevision: number;
  planAsideId: string | null;
};
type Revision = { owner: Target; note: string };

/** Decisions belong to the captured gate, even if the user navigates while the
 * Host writes. Only successful replies archive or settle UI; failures can retry. */
export function usePlanResolution(c: PlanResolutionContext) {
  const latest = useRef(c); latest.current = c;
  const versions = useRef({ projectId: c.projectId, mode: c.mode, goalMode: c.goalMode, composer: 0,
    activeId: c.sideWorkbench.activeId, collapsed: c.asideCollapsed, aside: 0 });
  const v = versions.current;
  if (v.projectId !== c.projectId || v.mode !== c.mode || v.goalMode !== c.goalMode) {
    v.projectId = c.projectId; v.mode = c.mode; v.goalMode = c.goalMode; v.composer++;
  }
  if (v.activeId !== c.sideWorkbench.activeId || v.collapsed !== c.asideCollapsed) {
    v.activeId = c.sideWorkbench.activeId; v.collapsed = c.asideCollapsed; v.aside++;
  }
  const [revision, setRevision] = useState<Revision | null>(null);
  const revisionRef = useRef(revision); revisionRef.current = revision;
  const cache = c.planBySessionRef.current;
  const claims = getPlanReplyClaims(cache);
  useSyncExternalStore(claims.subscribe, claims.snapshot, claims.snapshot);
  useEffect(() => {
    return claims.subscribeSettlements((owner, next) => {
      const now = latest.current;
      if (now.planBySessionRef.current !== cache || now.viewingSessionIdRef.current !== owner.sessionId) return;
      now.setPlan((shown) => latest.current.planBySessionRef.current === cache &&
        latest.current.viewingSessionIdRef.current === owner.sessionId &&
        samePlanReplyTarget(shown, owner.plan) ? next : shown);
    });
  }, [claims, cache]);

  const owns = useCallback((target: Target) => {
    const now = latest.current;
    return now.planBySessionRef.current === target.cache &&
      samePlanReplyTarget(target.cache.get(target.sessionId), target.plan);
  }, []);
  const inOriginalView = useCallback((target: Target) => {
    const now = latest.current;
    return now.viewingSessionIdRef.current === target.sessionId && isSameView(target.focus, now.currentViewFocus());
  }, []);
  const capture = useCallback((): Target | null => {
    const now = latest.current;
    const sessionId = now.viewingSessionIdRef.current;
    const plan = now.planRef.current;
    if (!sessionId || now.planBySessionRef.current.get(sessionId) !== plan) return null;
    const active = now.sideWorkbench.tabs.find((tab) => tab.id === now.sideWorkbench.activeId);
    return { sessionId, plan, cache: now.planBySessionRef.current, focus: { ...now.currentViewFocus() },
      projectId: now.projectId, mode: now.mode, goalMode: now.goalMode, composerRevision: versions.current.composer,
      asideRevision: versions.current.aside,
      planAsideId: !now.asideCollapsed && now.planOpenedAsideRef.current && active?.kind === "plan" ? active.id : null };
  }, []);

  const run = useCallback(async (target: Target, decision: Decision, feedback?: string, modalOwner?: Target) => {
    const ownerClaims = getPlanReplyClaims(target.cache);
    if (!owns(target) || (decision !== "abandoned" && target.plan.rpcId == null) || !ownerClaims.claim(target)) return;
    try {
      let settledRevision: string | null = null;
      if (target.plan.rpcId != null) {
        const reply = await api.sessionResolvePlan({ decision, feedback, sessionId: target.sessionId,
          rpcId: target.plan.rpcId, requestGeneration: target.plan.requestGeneration });
        settledRevision = reply?.planChromeRevision ?? null;
      } else {
        if (!target.plan.chromeRevision) {
          throw Object.assign(new Error("Plan chrome revision required"), { code: "PLAN_CHROME_CHANGED" });
        }
        // Read-only/progress chrome closes only the captured Host revision.
        await api.sessionPlanChromeSet(target.sessionId, planStateToStored(closedSessionPlan(
          latest.current.tr("plan.ready"), target.plan.toolCallId, null, target.plan.requestGeneration ?? null,
        )), target.plan.chromeRevision);
      }
      const now = latest.current;
      if (decision === "approved" || (decision === "abandoned" &&
          (target.plan.body.trim() || target.plan.entries.length > 0 || target.plan.rpcId != null))) {
        const row = now.sessionsRef.current.find((session) => session.id === target.sessionId);
        const title = row?.title?.trim() || (target.plan.title !== now.tr("plan.ready") ? target.plan.title : undefined);
        try { recordPlanHistory({ sessionId: target.sessionId, decision, title,
          bodyPreview: planDisplayMarkdown(target.plan.body, target.plan.entries) }); } catch { /* private mode / quota */ }
      }
      const canSettle = owns(target);
      if (canSettle) {
        const current = target.cache.get(target.sessionId)!;
        const next = decision === "abandoned" ? closedSessionPlan(now.tr("plan.ready"), target.plan.toolCallId,
          target.plan.rpcId, target.plan.requestGeneration ?? null) : {
            ...current, rpcId: null, requestGeneration: null, chromeRevision: settledRevision,
            gateStale: false, awaitingAgentApproval: false,
            userClosed: false, waiting: decision === "cancelled",
            visible: decision === "cancelled" && (!!current.body.trim() || current.entries.length > 0),
            barDismissed: decision === "approved" ? true : current.barDismissed,
          };
        target.cache.set(target.sessionId, next);
        now.markPlanPendingBadge(target.sessionId, next);
        ownerClaims.settle(target, next);
      }
      if (modalOwner) setRevision((modal) => modal?.owner === modalOwner ? null : modal);
      if (canSettle && inOriginalView(target)) {
        if (decision === "approved" && versions.current.composer === target.composerRevision &&
            shouldExitComposerPlanModeAfterDecision({ decision, composerMode: target.mode })) {
          now.setMode("agent"); now.setGoalMode(false);
          void api.composerPrefsSet({ projectId: target.projectId, sessionId: target.sessionId, mode: "agent" })
            .catch((error) => { if (inOriginalView(target)) latest.current.showToast(String(error), 4000); });
        }
        const active = now.sideWorkbench.tabs.find((tab) => tab.id === now.sideWorkbench.activeId);
        if (decision === "abandoned" && target.planAsideId && versions.current.aside === target.asideRevision &&
            active?.id === target.planAsideId && active.kind === "plan" && now.planOpenedAsideRef.current) {
          now.planOpenedAsideRef.current = false; now.collapseAsidePersisted();
        }
      }
    } catch (error) {
      if (owns(target) && inOriginalView(target)) {
        const code = error && typeof error === "object" && "code" in error ? String(error.code) : "";
        const changed = code === "PLAN_CHROME_CHANGED" || String(error).includes("plan chrome changed") ||
          String(error).includes("a live plan approval must be answered");
        latest.current.showToast(changed ? latest.current.tr("plan.changedRefresh") :
          code === "UNSUPPORTED" ? latest.current.tr("mirror.unsupported") : String(error), 4500);
      }
    } finally {
      ownerClaims.release(target);
    }
  }, [owns, inOriginalView]);

  const approvePlan = useCallback(async () => {
    const target = capture(); if (target) await run(target, "approved");
  }, [capture, run]);
  const openRequestPlanChanges = useCallback(() => {
    const target = capture();
    if (target && target.plan.rpcId != null && !getPlanReplyClaims(target.cache).has(target)) {
      setRevision((modal) => ({ owner: target, note: modal?.owner.cache === target.cache &&
        modal.owner.sessionId === target.sessionId && samePlanReplyTarget(target.plan, modal.owner.plan) ? modal.note : "" }));
    }
  }, [capture]);
  const requestPlanChanges = useCallback(async (note?: string) => {
    const target = capture();
    if (target) await run(target, "cancelled", note?.trim() || latest.current.tr("plan.reviseFeedback"));
  }, [capture, run]);
  const submitPlanRevision = useCallback(async () => {
    if (revision && revisionRef.current?.owner === revision.owner) {
      await run(revision.owner, "cancelled", revision.note.trim() || latest.current.tr("plan.reviseFeedback"), revision.owner);
    }
  }, [revision, run]);
  const closePlanRevise = useCallback(() => {
    setRevision((modal) => modal?.owner !== revision?.owner ||
      (modal && getPlanReplyClaims(modal.owner.cache).has(modal.owner)) ? modal : null);
  }, [revision]);
  const setPlanReviseNote = useCallback((note: string) => {
    setRevision((modal) => modal && modal.owner === revision?.owner &&
      !getPlanReplyClaims(modal.owner.cache).has(modal.owner) ? { ...modal, note } : modal);
  }, [revision]);
  const dismissPlan = useCallback(() => {
    const target = capture();
    if (!target || getPlanReplyClaims(target.cache).has(target) ||
        (!target.plan.visible && !target.plan.entries.length && !target.plan.body && target.plan.rpcId == null)) return;
    const tr = latest.current.tr;
    latest.current.setAppDialog({ kind: "confirm", title: tr("plan.dismissConfirmTitle"), message: tr("plan.dismissConfirmMessage"),
      confirmLabel: tr("plan.dismiss"), danger: false, onConfirm: () => run(target, "abandoned") });
  }, [capture, run]);
  const viewOwner = { sessionId: c.viewingSessionIdRef.current ?? "", plan: c.plan };
  return {
    plan: { ...c.plan, busy: claims.has(viewOwner) }, approvePlan, requestPlanChanges, openRequestPlanChanges, dismissPlan,
    planReviseOpen: !!revision && owns(revision.owner) && inOriginalView(revision.owner),
    planReviseNote: revision?.note ?? "", planReviseBusy: !!revision && getPlanReplyClaims(revision.owner.cache).has(revision.owner),
    setPlanReviseNote, closePlanRevise, submitPlanRevision,
  };
}
