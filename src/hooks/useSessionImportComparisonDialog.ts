import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { comparisonContextKey, copyComparisonContext, type ComparisonPreviewContext } from "@/lib/sessionImport/comparisonPreviewState";
import { copyFixedPreviewTransfer, fixedScopeKey, type FixedPreviewTransferRequest } from "@/lib/sessionImport/fixedPreviewTransfer";
import type { useSessionImportUi } from "./useSessionImport";
import type { useSessionImportContentStates } from "./useSessionImportContentStates";

type Ui = Pick<ReturnType<typeof useSessionImportUi>, "open" | "step" | "busy" | "selectionView" | "fixedAdoption" |
  "getComparisonContext" | "adoptFixedPreview" | "reconcileFixedPreview" | "cancelFixedAdoption" |
  "ownsFixedComparison" | "dismissFixedAdoptionStatus" | "getFixedAdoptionStatus" | "refreshScan">;
type Content = ReturnType<typeof useSessionImportContentStates>;
type Intent = { request: FixedPreviewTransferRequest; retry: boolean };
type Dialog = { context: ComparisonPreviewContext; key: number };

/** One Host-owned comparison and consent; child readiness changes cannot cancel an owned ACK/read gap. */
export function useSessionImportComparisonDialog(ui: Ui, content: Content, setConsentLock: (locked: boolean) => void) {
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const current = useRef<Dialog | null>(null);
  const latest = useRef({ ui, content }); latest.current = { ui, content };
  const serial = useRef(0);
  const command = useRef<number | null>(null);
  const [working, setWorking] = useState(false);
  const [consent, setConsent] = useState<Intent | null>(null);
  const consentRef = useRef<Intent | null>(null);
  const lastRequest = useRef<FixedPreviewTransferRequest | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [needsRecompare, setNeedsRecompare] = useState(false);
  const recovery = useRef(false);
  const [revealSerial, setRevealSerial] = useState(0);
  const release = useCallback((completed: boolean) => {
    ++serial.current; command.current = null; current.current = null;
    consentRef.current = null; lastRequest.current = null; recovery.current = false;
    setConsentLock(false); setConsent(null); setDialog(null); setWorking(false); setError(null); setNeedsRecompare(false);
    if (!completed) {
      latest.current.ui.cancelFixedAdoption?.();
      latest.current.content.cancelFixedTransfer?.();
    }
  }, [setConsentLock]);
  const close = useCallback(() => release(false), [release]);
  useEffect(() => () => {
    ++serial.current; command.current = null; current.current = null; consentRef.current = null; lastRequest.current = null;
    setConsentLock(false);
    latest.current.ui.cancelFixedAdoption?.(); latest.current.content.cancelFixedTransfer?.();
  }, [setConsentLock]);
  const sameScope = useCallback((context: ComparisonPreviewContext) => {
    const { ui } = latest.current;
    return ui.open && ui.step === "content" && ui.selectionView?.domain === "fixed-groups" &&
      fixedScopeKey(ui.selectionView.scope) === fixedScopeKey(context.scope);
  }, []);
  useLayoutEffect(() => {
    const context = current.current?.context;
    if (!context) return;
    const fresh = ui.getComparisonContext?.(context.group.groupId);
    const ordinary = fresh && comparisonContextKey(fresh) === comparisonContextKey(context);
    if (!sameScope(context) || (!ordinary && !ui.ownsFixedComparison?.(context) && command.current === null && !recovery.current)) close();
  }, [ui.open, ui.step, ui.selectionView, ui.fixedAdoption, ui.getComparisonContext, ui.ownsFixedComparison, close, sameScope]);
  const open = useCallback((context: ComparisonPreviewContext) => {
    const { ui } = latest.current;
    const fresh = ui.getComparisonContext?.(context.group.groupId);
    if (command.current !== null || consentRef.current || ui.busy || !fresh ||
      comparisonContextKey(fresh) !== comparisonContextKey(context) || !sameScope(context) || !ui.dismissFixedAdoptionStatus()) return;
    const next = { context: copyComparisonContext(fresh), key: ++serial.current };
    current.current = next; lastRequest.current = null; recovery.current = false;
    setDialog(next); setError(null); setNeedsRecompare(false);
  }, [sameScope]);
  const run = useCallback(async (intent: Intent, discardEdits: boolean) => {
    const context = current.current?.context;
    if (!context || command.current !== null || !sameScope(context) ||
      comparisonContextKey(intent.request.context) !== comparisonContextKey(context)) return;
    const owner = ++serial.current; command.current = owner;
    setWorking(true); setError(null);
    lastRequest.current = intent.request;
    const owns = () => command.current === owner && current.current?.context === context && sameScope(context);
    try {
      const { ui, content } = latest.current;
      const transfer = { ...content.fixedPreviewTransfer,
        run: (request: FixedPreviewTransferRequest, authority: Parameters<Content["adoptFixedPreview"]>[1]) =>
          content.adoptFixedPreview(request, authority, { discardEdits }) };
      const accepted = await (intent.retry ? ui.reconcileFixedPreview(transfer) : ui.adoptFixedPreview(intent.request, transfer));
      if (!owns()) return;
      if (accepted) { setRevealSerial((value) => value + 1); release(true); }
      else {
        const status = ui.getFixedAdoptionStatus(), transferError = content.fixedPreviewTransfer.getError();
        if (!status.error && !transferError) {
          setError("PLAN_STALE: fixed adoption was refused; compare the current versions again");
          recovery.current = true; setNeedsRecompare(true);
        } else if (status.phase === "failed" && /(?:PLAN_STALE|SCHEMA_UNSUPPORTED|RESOURCE_LIMIT):/.test(status.error ?? transferError ?? "")) {
          recovery.current = true; setNeedsRecompare(true);
        }
      }
    } catch (cause) { if (owns()) setError(String(cause)); }
    finally { if (owns()) { command.current = null; setWorking(false); } }
  }, [release, sameScope]);
  const requestAdoption = useCallback((input: FixedPreviewTransferRequest, retry = false) => {
    if (command.current !== null || consentRef.current || !current.current) return;
    const phase = latest.current.ui.getFixedAdoptionStatus().phase;
    if (retry ? phase !== "reconcile-required" :
      phase === "adopting" || phase === "reconcile-required" || phase === "uncertain" || phase === "stale") return;
    try {
      const request = copyFixedPreviewTransfer(input), context = current.current.context;
      if (!sameScope(context) || comparisonContextKey(request.context) !== comparisonContextKey(context)) return;
      const slots = latest.current.content.byCandidate;
      const dirty = !!slots[request.context.primary.candidateId]?.dirty || !!slots[request.member.candidate.candidateId]?.dirty;
      const intent = { request, retry };
      if (dirty) { consentRef.current = intent; setConsentLock(true); setConsent(intent); }
      else void run(intent, false);
    } catch (cause) {
      const error = String(cause);
      setError(error);
      if (/(?:PLAN_STALE|SCHEMA_UNSUPPORTED|RESOURCE_LIMIT):/.test(error)) {
        recovery.current = true; setNeedsRecompare(true);
      }
    }
  }, [run, sameScope, setConsentLock]);
  const cancelConsent = useCallback(() => { consentRef.current = null; setConsentLock(false); setConsent(null); }, [setConsentLock]);
  const confirmConsent = useCallback(() => {
    const intent = consentRef.current;
    if (!intent) return;
    cancelConsent();
    void run(intent, true); // Release consent lock and reserve selection in this same event.
  }, [cancelConsent, run]);
  const retry = useCallback(() => {
    if (latest.current.ui.getFixedAdoptionStatus().phase === "reconcile-required" && lastRequest.current) requestAdoption(lastRequest.current, true);
  }, [requestAdoption]);
  const recompare = useCallback(async () => {
    const context = current.current?.context;
    if (!context || command.current !== null || consentRef.current || latest.current.ui.getFixedAdoptionStatus().busy || !sameScope(context)) return;
    const owner = ++serial.current; command.current = owner;
    recovery.current = true; setNeedsRecompare(true); setWorking(true); setError(null);
    latest.current.ui.cancelFixedAdoption(); latest.current.content.cancelFixedTransfer();
    const owns = () => command.current === owner && current.current?.context === context && sameScope(context);
    try {
      if (!(await latest.current.ui.refreshScan())) throw new Error("PLAN_STALE: current versions could not be refreshed");
      if (!owns()) return;
      const fresh = latest.current.ui.getComparisonContext(context.group.groupId);
      if (!fresh) throw new Error("PLAN_STALE: source group is unavailable after refresh");
      latest.current.ui.dismissFixedAdoptionStatus();
      const next = { context: fresh, key: ++serial.current };
      current.current = next; lastRequest.current = null; recovery.current = false;
      setDialog(next); setNeedsRecompare(false);
    } catch (cause) { if (owns()) setError(String(cause)); }
    finally { if (command.current === owner) { command.current = null; setWorking(false); } }
  }, [sameScope]);
  const isLocked = useCallback(() => {
    const phase = latest.current.ui.getFixedAdoptionStatus().phase;
    return command.current !== null || !!consentRef.current || recovery.current ||
      phase === "adopting" || phase === "reconcile-required" || phase === "uncertain" || phase === "stale";
  }, []);
  const phase = ui.fixedAdoption?.phase ?? "idle";
  const blockedPhase = phase === "adopting" || phase === "reconcile-required" || phase === "uncertain" || phase === "stale";
  return { dialog, consent, close, open, cancelConsent, confirmConsent, requestAdoption, retry, recompare, revealSerial,
    busy: working || !!consent, phase, error: error ?? ui.fixedAdoption?.error ?? null,
    locked: working || !!consent || blockedPhase || needsRecompare, isLocked,
    canRecompare: needsRecompare || phase === "uncertain" || phase === "stale" };
}
