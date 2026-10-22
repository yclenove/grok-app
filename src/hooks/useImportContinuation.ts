import { useCallback, useEffect, useRef, useState } from "react";
import * as api from "@/lib/api";
import type { HandoffDraft } from "@/lib/api/sessionMigration";
import type { AllowedSelection } from "@/lib/sessionImport/selection";

const message = (cause: unknown) => cause instanceof Error ? cause.message : String(cause);
type Attempt = { request: string; text: string };
export type ImportContinuationClient = Pick<typeof api,
  "sessionImportContinuePrepare" | "sessionImportContinueWorkspace" | "sessionImportContinueSelection" |
  "sessionImportContinueRestore" | "sessionImportContinueDiscard" | "sessionImportContinueCreate">;
/** One in-memory slot. Identity is the requested import, anchor/leaf, revision, and project. */
type ContinuationIntent = {
  importId: string;
  leaf: string | null;
  revision: string | null;
  projectId: string | null;
};

function sameContinuationIntent(owned: ContinuationIntent | null, requested: ContinuationIntent): boolean {
  return !!owned
    && owned.importId === requested.importId
    && owned.leaf === requested.leaf
    && owned.revision === requested.revision
    && owned.projectId === requested.projectId;
}

function intentFromDraft(next: HandoffDraft): ContinuationIntent {
  // A versioned draft owns its constraint, including explicit null. Anything else
  // uses the draft's own project. Neither inherits whatever wizard is open.
  const projectId = next.creationVersion != null
    ? (next.workspaceProjectConstraint ?? null)
    : (next.projectId ?? null);
  return {
    importId: next.importId,
    leaf: next.leaf ?? null,
    revision: next.revisionId ?? null,
    projectId,
  };
}

/** Creation retries own a frozen request, even if the first IPC reply is lost. */
export function useImportContinuation(
  projectId: string | null,
  onActivity: () => void,
  client: ImportContinuationClient = api,
) {
  const [draft, renderDraft] = useState<HandoffDraft | null>(null);
  const [continuationError, setContinuationError] = useState<string | null>(null);
  const [continuationBusy, setBusy] = useState(false);
  const [continuationLocked, setLocked] = useState(false);
  const [continuationReviewingScope, setReviewingScope] = useState(false);
  const [continuationScopePending, setScopePending] = useState(false);
  const reviewingScope = useRef(false);
  const scopeAttempt = useRef<AllowedSelection | null>(null);
  const current = useRef<HandoffDraft | null>(null);
  const intent = useRef<ContinuationIntent | null>(null);
  const attempt = useRef<Attempt | null>(null);
  const busy = useRef(false);
  const generation = useRef(0);
  const projectRef = useRef(projectId);
  projectRef.current = projectId;
  const [continuationVisible, setContinuationVisible] = useState(false);
  useEffect(() => () => { generation.current += 1; }, []);

  const adopt = useCallback((next: HandoffDraft | null, keepIntent = false) => {
    current.current = next;
    reviewingScope.current = false;
    scopeAttempt.current = null;
    setReviewingScope(false);
    setScopePending(false);
    renderDraft(next);
    attempt.current = next?.creationRequestId ? { request: next.creationRequestId, text: next.text } : null;
    setLocked(!!attempt.current);
    if (!next) {
      intent.current = null;
      setContinuationVisible(false);
      return;
    }
    if (!keepIntent) intent.current = intentFromDraft(next);
    setContinuationVisible(intent.current?.projectId === (projectRef.current ?? null));
  }, []);

  const invalidateContinuation = useCallback(() => {
    generation.current += 1;
    busy.current = false;
    setBusy(false);
    setContinuationError(null);
    adopt(null);
  }, [adopt]);

  const setDraft = useCallback((next: HandoffDraft | null) => {
    if (next?.draftId === current.current?.draftId && next) {
      if (busy.current || attempt.current || reviewingScope.current) return;
      current.current = next;
      renderDraft(next);
    } else {
      invalidateContinuation();
      adopt(next);
    }
  }, [adopt, invalidateContinuation]);

  // A frozen create/scope outcome stays recoverable. Hiding or changing project
  // is not a discard and must not unlock it or forget the identical retry.
  const frozenRecovery = useCallback(() => !!current.current && !!(
    attempt.current || current.current.creationRequestId || scopeAttempt.current
  ), []);
  // Idle X/Escape/backdrop hides the editor. It does not discard, create, or
  // unlock a frozen request. A different project is another execution target:
  // drop an unlocked presentation, but keep a frozen recovery concealed.
  const hideContinuation = useCallback(() => {
    if (busy.current || !current.current) return;
    if (reviewingScope.current && !scopeAttempt.current) {
      reviewingScope.current = false;
      setReviewingScope(false);
    }
    setContinuationVisible(false);
  }, []);
  const supersedeIntent = useCallback(() => {
    generation.current += 1;
    busy.current = false;
    setBusy(false);
    if (frozenRecovery()) {
      setContinuationVisible(false);
      return;
    }
    setContinuationError(null);
    adopt(null);
  }, [adopt, frozenRecovery]);
  const projectSeen = useRef(projectId);
  useEffect(() => {
    if (projectSeen.current === projectId) return;
    projectSeen.current = projectId;
    // Pending prepare owns its intent before a draft exists. A project change is
    // its own supersession, not a busy-hide, so that request cannot block the next one.
    if (!intent.current || intent.current.projectId === (projectId ?? null)) return;
    supersedeIntent();
  }, [projectId, supersedeIntent]);

  const begin = useCallback(() => {
    onActivity();
    busy.current = true;
    setBusy(true);
    setContinuationError(null);
    return ++generation.current;
  }, [onActivity]);
  const end = useCallback((token: number) => {
    if (token !== generation.current) return;
    busy.current = false;
    setBusy(false);
  }, []);

  const prepareContinue = useCallback(async (importId: string, leaf?: string | null, revision?: string | null) => {
    const requested: ContinuationIntent = {
      importId,
      leaf: leaf ?? null,
      revision: revision ?? null,
      projectId: projectId ?? null,
    };
    if (current.current && sameContinuationIntent(intent.current, requested)) {
      if (!busy.current) setContinuationVisible(true);
      return;
    }
    // A running prepare/create/workspace/scope still owns the open surface.
    if (busy.current) return;
    if (frozenRecovery()) {
      setContinuationVisible(false);
      return;
    }
    if (current.current) {
      generation.current += 1;
      setContinuationError(null);
      adopt(null);
    }
    intent.current = requested;
    const token = begin();
    try {
      const next = await client.sessionImportContinuePrepare(importId, { projectId, leaf, revision });
      if (token !== generation.current) return;
      intent.current = requested;
      adopt(next, true);
    } catch (cause) {
      if (token === generation.current) setContinuationError(message(cause));
    } finally { end(token); }
  }, [adopt, begin, client, end, frozenRecovery, projectId]);

  const selectContinuationWorkspace = useCallback(async (workspaceId: string | null) => {
    const previous = current.current;
    if (!previous || busy.current || attempt.current || reviewingScope.current) return;
    const token = begin();
    try {
      const next = await client.sessionImportContinueWorkspace(previous.draftId, previous.draftHash, workspaceId);
      if (token === generation.current) adopt({ ...next, text: previous.text }, true);
    } catch (cause) {
      if (token === generation.current) setContinuationError(message(cause));
    } finally { end(token); }
  }, [adopt, begin, client, end]);

  const beginContinuationScopeReview = useCallback(() => {
    if (!current.current?.selection || busy.current || attempt.current) return;
    reviewingScope.current = true;
    setReviewingScope(true);
  }, []);
  const cancelContinuationScopeReview = useCallback(() => {
    if (busy.current || scopeAttempt.current) return;
    reviewingScope.current = false;
    setReviewingScope(false);
    setContinuationError(null);
  }, []);
  const selectContinuationScope = useCallback(async (selection: AllowedSelection) => {
    const previous = current.current;
    if (!previous?.selection || busy.current || attempt.current) return false;
    reviewingScope.current = true;
    setReviewingScope(true);
    // Unknown replacement outcomes retry the same scope, not a newly edited one.
    const frozen = scopeAttempt.current ?? structuredClone(selection);
    scopeAttempt.current = frozen;
    setScopePending(true);
    const token = begin();
    try {
      const next = await client.sessionImportContinueSelection(previous.draftId, previous.draftHash, frozen);
      if (token !== generation.current) return false;
      adopt(next, true); // Regenerate the text; old prose may contain excluded history.
      return true;
    } catch (cause) {
      if (token !== generation.current) return false;
      setContinuationError(message(cause));
      try {
        const restored = await client.sessionImportContinueRestore(previous.draftId, previous.draftHash);
        if (token === generation.current && restored.draftId === previous.draftId && restored.draftHash === previous.draftHash) {
          if (restored.creationRequestId) {
            const frozenScope = scopeAttempt.current;
            adopt(restored, true);
            if (frozenScope) {
              scopeAttempt.current = frozenScope;
              setScopePending(true);
            }
          } else { scopeAttempt.current = null; setScopePending(false); }
        }
      } catch { /* Unknown outcomes remain frozen for an identical retry. */ }
      return false;
    } finally { end(token); }
  }, [adopt, begin, client, end]);

  const discardContinuation = useCallback(async () => {
    const previous = current.current;
    if (!previous || busy.current || scopeAttempt.current) return false;
    const token = begin();
    try {
      await client.sessionImportContinueDiscard(previous.draftId, previous.draftHash);
      if (token !== generation.current) return false;
      adopt(null);
      return true;
    } catch (cause) {
      if (token === generation.current) setContinuationError(message(cause));
      return false;
    } finally { end(token); }
  }, [adopt, begin, client, end]);

  const createContinue = useCallback(async () => {
    const previous = current.current;
    if (!previous || busy.current || reviewingScope.current || !previous.text.trim()) return null;
    const token = begin();
    const frozen = attempt.current ?? {
      request: previous.creationRequestId ?? `continuation:${previous.draftId}`,
      text: previous.text,
    };
    attempt.current = frozen;
    setLocked(true);
    try {
      const result = await client.sessionImportContinueCreate(previous.draftId, previous.draftHash, frozen.request, frozen.text);
      if (token !== generation.current) return null;
      adopt(null);
      return result.appSessionId;
    } catch (cause) {
      if (token !== generation.current) return null;
      setContinuationError(message(cause));
      try {
        const restored = await client.sessionImportContinueRestore(previous.draftId, previous.draftHash);
        if (token === generation.current
          && restored.draftId === previous.draftId
          && restored.draftHash === previous.draftHash) {
          // Only this owned operation keeps the requested identity. A different
          // restore must not inherit the edited prose or thaw another target.
          adopt(restored.creationRequestId ? restored : { ...restored, text: frozen.text }, true);
        }
      } catch {
        // An unknown result keeps the original request/text frozen for retry.
      }
      return null;
    } finally { end(token); }
  }, [adopt, begin, client, end]);

  return { draft, setDraft, continuationError, setContinuationError, continuationBusy,
    continuationLocked, continuationVisible, prepareContinue, selectContinuationWorkspace,
    discardContinuation, hideContinuation, createContinue, invalidateContinuation,
    continuationReviewingScope, continuationScopePending, beginContinuationScopeReview,
    cancelContinuationScopeReview, selectContinuationScope };
}
