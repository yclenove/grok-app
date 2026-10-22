import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  sessionImportPreview,
  sessionImportPreviewEvents,
} from "@/lib/api/sessionMigration";
import { peekSummaryFromAdapter } from "@/lib/sessionImport/peekSummary";
import {
  allowedSelectionOf,
  applySelection,
  beginOperation,
  beginRequest,
  beginRevisionReload,
  emptyPreviewSlot,
  isCurrentRequest,
  migratePendingToPreview,
  sameOperationBusy,
  slotNeedsReload,
  type BusyOperation,
  type ContentRequestOwner,
  type PreviewContentSlot,
} from "@/lib/sessionImport/contentStates";
import type { ContentSelectionAction } from "@/lib/sessionImport/contentSelection";
import { copyFixedPreviewTransfer, fixedPreviewContentSlot, fixedScopeKey, fixedTransferIdentity, FIXED_PREVIEW_PAGE_SIZE,
  type FixedPreviewTransferRequest, type FixedPreviewAuthority } from "@/lib/sessionImport/fixedPreviewTransfer";
import type { ImportGroupScope } from "@/lib/sessionImport/groupSelectionState";

export type SelectedPreviewSeed = {
  candidateId: string;
  sourceRevision: string | null;
  title: string;
};

type Store = {
  pendingByCandidate: Record<string, PreviewContentSlot>;
  slotsByPreviewId: Record<string, PreviewContentSlot>;
  retainedCandidateIds: string[];
  scanGeneration: number;
  scope: ImportGroupScope | null;
};

export type SessionImportPreviewClient = {
  preview: typeof sessionImportPreview;
  events: typeof sessionImportPreviewEvents;
};

const livePreviewClient: SessionImportPreviewClient = {
  preview: sessionImportPreview,
  events: sessionImportPreviewEvents,
};

function bagFromStore(store: Store): Record<string, PreviewContentSlot> {
  const bag: Record<string, PreviewContentSlot> = { ...store.pendingByCandidate };
  for (const slot of Object.values(store.slotsByPreviewId)) {
    bag[slot.candidateId] = slot;
  }
  return bag;
}

function findByCandidate(store: Store, candidateId: string): PreviewContentSlot | undefined {
  return store.pendingByCandidate[candidateId]
    ?? Object.values(store.slotsByPreviewId).find((slot) => slot.candidateId === candidateId);
}

function writeSlot(store: Store, slot: PreviewContentSlot): Store {
  if (slot.previewId) {
    const pending = { ...store.pendingByCandidate };
    delete pending[slot.candidateId];
    const slots = { ...store.slotsByPreviewId };
    for (const [id, existing] of Object.entries(slots)) {
      if (existing.candidateId === slot.candidateId && id !== slot.previewId) delete slots[id];
    }
    slots[slot.previewId] = slot;
    return {
      ...store,
      pendingByCandidate: pending,
      slotsByPreviewId: slots,
    };
  }
  const slots = { ...store.slotsByPreviewId };
  for (const [id, existing] of Object.entries(slots)) {
    if (existing.candidateId === slot.candidateId) delete slots[id];
  }
  return {
    ...store,
    slotsByPreviewId: slots,
    pendingByCandidate: { ...store.pendingByCandidate, [slot.candidateId]: slot },
  };
}

export function useSessionImportContentStates(
  onError?: (message: string) => void,
  client: SessionImportPreviewClient = livePreviewClient,
) {
  const [store, setStore] = useState<Store>({
    pendingByCandidate: {},
    slotsByPreviewId: {},
    retainedCandidateIds: [],
    scanGeneration: 0,
    scope: null,
  });
  const [activeCandidateId, setActiveCandidateIdState] = useState<string | null>(null);
  const [activePreviewId, setActivePreviewId] = useState<string | null>(null);
  const storeRef = useRef(store);
  const activeCandidateIdRef = useRef<string | null>(null);
  const requestSerial = useRef(0);
  const lifetime = useRef(0);
  const fixedFlight = useRef<number | null>(null);
  const fixedTarget = useRef<string | null>(null);
  const fixedPrimary = useRef<string | null>(null);
  const fixedCommit = useRef<{ lifetime: number; identity: string; slot: PreviewContentSlot } | null>(null);
  const fixedError = useRef<string | null>(null);
  const [fixedTransfer, setFixedTransfer] = useState({ busy: false, error: null as string | null });
  const cancelFixedTransfer = useCallback(() => {
    fixedFlight.current = null;
    fixedTarget.current = null;
    fixedPrimary.current = null;
    fixedCommit.current = null;
    fixedError.current = null;
    setFixedTransfer({ busy: false, error: null });
  }, []);

  // Slots can be removed and recreated. Their local counters alone cannot
  // distinguish an old response from a new request for the same candidate.
  const ownRequest = useCallback((started: ReturnType<typeof beginRequest>) => {
    const requestSeq = ++requestSerial.current;
    return {
      slot: { ...started.slot, requestSeq },
      owner: { ...started.owner, requestSeq },
    };
  }, []);

  useEffect(() => () => {
    ++lifetime.current;
    fixedFlight.current = null;
    fixedTarget.current = null;
    fixedPrimary.current = null;
    fixedCommit.current = null;
    fixedError.current = null;
    storeRef.current = {
      pendingByCandidate: {},
      slotsByPreviewId: {},
      retainedCandidateIds: [],
      scanGeneration: storeRef.current.scanGeneration,
      scope: null,
    };
    activeCandidateIdRef.current = null;
  }, []);

  const commitStore = useCallback((next: Store) => {
    storeRef.current = next;
    setStore(next);
  }, []);

  const setActiveCandidateId = useCallback((id: string | null) => {
    if (fixedFlight.current !== null) return;
    activeCandidateIdRef.current = id;
    setActiveCandidateIdState(id);
  }, []);

  const adoptPreviewId = useCallback((candidateId: string, previewId: string) => {
    const active = activeCandidateIdRef.current;
    if (active === candidateId) {
      setActivePreviewId(previewId);
    }
  }, []);

  const byCandidate = useMemo(() => bagFromStore(store), [store]);

  const commitIfCurrent = useCallback(
    (owner: ContentRequestOwner, update: (slot: PreviewContentSlot) => PreviewContentSlot) => {
      const current = findByCandidate(storeRef.current, owner.candidateId);
      if (!isCurrentRequest(current, owner) || !current) return;
      const nextSlot = update(current);
      const nextStore = writeSlot(storeRef.current, nextSlot);
      commitStore(nextStore);
      if (nextSlot.previewId) adoptPreviewId(owner.candidateId, nextSlot.previewId);
    },
    [adoptPreviewId, commitStore],
  );

  const loadPreview = useCallback(
    async (seed: SelectedPreviewSeed, opts?: { force?: boolean }) => {
      if (fixedFlight.current !== null) return;
      const existing = findByCandidate(storeRef.current, seed.candidateId);
      if (
        !opts?.force &&
        existing &&
        !slotNeedsReload(existing, seed.sourceRevision, storeRef.current.scanGeneration) &&
        existing.previewId
      ) {
        adoptPreviewId(seed.candidateId, existing.previewId);
        return;
      }
      if (
        sameOperationBusy(existing, "preview") &&
        !opts?.force &&
        !slotNeedsReload(existing, seed.sourceRevision, storeRef.current.scanGeneration)
      ) {
        return;
      }
      const started = ownRequest(
        existing
          ? slotNeedsReload(existing, seed.sourceRevision, storeRef.current.scanGeneration) || opts?.force
            ? beginRevisionReload(existing, seed.sourceRevision, seed.title)
            : beginRequest({ ...existing, title: seed.title })
          : beginRequest(
              emptyPreviewSlot(
                seed.candidateId,
                seed.sourceRevision,
                seed.title,
                storeRef.current.scanGeneration,
              ),
            ),
      );
      commitStore(writeSlot(storeRef.current, started.slot));
      const owner = started.owner;
      try {
        const preview = await client.preview(seed.candidateId, seed.sourceRevision);
        const live = findByCandidate(storeRef.current, seed.candidateId);
        if (!isCurrentRequest(live, owner)) return;
        const withPreview = migratePendingToPreview(live!, preview.previewId);
        commitStore(writeSlot(storeRef.current, withPreview));
        adoptPreviewId(seed.candidateId, preview.previewId);
        const summary = peekSummaryFromAdapter(preview.adapter);
        const page = await client.events(
          preview.previewId,
          null,
          20,
          summary.leaf,
        );
        if (!isCurrentRequest(findByCandidate(storeRef.current, seed.candidateId), {
          ...owner,
          previewId: preview.previewId,
        })) {
          return;
        }
        const leaf = summary.leaf;
        const leaves = summary.leaves.length ? summary.leaves : leaf ? [leaf] : [];
        commitIfCurrent(
          { ...owner, previewId: preview.previewId },
          (slot) =>
            applySelection(
              {
                ...slot,
                previewId: preview.previewId,
                snapshotId: seed.sourceRevision,
                summary,
                leafId: leaf ?? slot.leafId,
                busy: false,
                busyOperation: "idle",
                error: null,
              },
              {
                type: "reset",
                previewId: preview.previewId,
                leaf,
                leaves,
                events: page.events,
                nextCursor: page.nextCursor == null ? null : String(page.nextCursor),
                defaultSelection: (summary.defaultSelection ?? null) as never,
                archiveCount: summary.eventCount,
              },
            ),
        );
      } catch (e) {
        if (!isCurrentRequest(findByCandidate(storeRef.current, seed.candidateId), owner)) return;
        const message = String(e);
        commitIfCurrent(owner, (slot) => ({
          ...slot,
          busy: false,
          busyOperation: "idle",
          error: message,
        }));
        onError?.(message);
      } finally {
        const slot = findByCandidate(storeRef.current, seed.candidateId);
        if (isCurrentRequest(slot, owner) && slot?.busyOperation === "preview") {
          commitIfCurrent(owner, (s) => ({ ...s, busy: false, busyOperation: "idle" }));
        }
      }
    },
    [adoptPreviewId, client, commitIfCurrent, commitStore, onError, ownRequest],
  );

  const ensureSelected = useCallback(
    (seeds: SelectedPreviewSeed[], scanGeneration?: number, scope?: ImportGroupScope | null) => {
      const current = storeRef.current;
      const nextScope = scope === undefined ? current.scope : scope ? { ...scope } : null;
      if ((scanGeneration != null && scanGeneration !== current.scanGeneration) || fixedScopeKey(nextScope) !== fixedScopeKey(current.scope)) {
        ++lifetime.current;
        cancelFixedTransfer();
        commitStore({
          pendingByCandidate: {},
          slotsByPreviewId: {},
          retainedCandidateIds: seeds.map((s) => s.candidateId),
          scanGeneration: scanGeneration ?? nextScope?.generation ?? current.scanGeneration,
          scope: nextScope,
        });
        setActiveCandidateId(null);
        setActivePreviewId(null);
      } else {
        const retained = Array.from(
          new Set([...storeRef.current.retainedCandidateIds, ...seeds.map((s) => s.candidateId)]),
        );
        const prev = storeRef.current.retainedCandidateIds;
        const same =
          retained.length === prev.length && retained.every((id, i) => id === prev[i]);
        if (!same) {
          commitStore({ ...storeRef.current, retainedCandidateIds: retained });
        }
      }
    },
    [cancelFixedTransfer, commitStore, setActiveCandidateId],
  );

  const open = useCallback(
    async (seed: SelectedPreviewSeed) => {
      if (fixedFlight.current !== null) return;
      setActiveCandidateId(seed.candidateId);
      const existing = findByCandidate(storeRef.current, seed.candidateId);
      if (existing?.previewId) setActivePreviewId(existing.previewId);
      else setActivePreviewId(null);
      await loadPreview(seed);
    },
    [loadPreview, setActiveCandidateId],
  );

  const runKeyed = useCallback(
    async (
      candidateId: string,
      operation: BusyOperation,
      run: (slot: PreviewContentSlot, owner: ContentRequestOwner) => Promise<void>,
    ) => {
      const slot = findByCandidate(storeRef.current, candidateId);
      if (fixedFlight.current !== null) return;
      if (!slot?.previewId) return;
      if (sameOperationBusy(slot, operation)) return;
      const started = ownRequest(beginOperation(slot, operation));
      commitStore(writeSlot(storeRef.current, started.slot));
      const owner = started.owner;
      try {
        await run(started.slot, owner);
      } finally {
        const current = findByCandidate(storeRef.current, candidateId);
        if (isCurrentRequest(current, owner) && current?.busyOperation === operation) {
          commitIfCurrent(owner, (s) => ({ ...s, busy: false, busyOperation: "idle" }));
        }
      }
    },
    [commitIfCurrent, commitStore, ownRequest],
  );

  const loadMore = useCallback(
    async (candidateId: string) => {
      const slot = findByCandidate(storeRef.current, candidateId);
      if (!slot?.previewId || slot.selection.nextCursor == null) return;
      if (sameOperationBusy(slot, "page")) return;
      await runKeyed(candidateId, "page", async (live, owner) => {
        const leaf = live.selection.leaf;
        const cursor = live.selection.nextCursor;
        const gen = live.selection.generation;
        try {
          const page = await client.events(live.previewId!, cursor, 20, leaf);
          if (!isCurrentRequest(findByCandidate(storeRef.current, candidateId), owner)) return;
          commitIfCurrent(owner, (s) =>
            applySelection(s, {
              type: "appendPage",
              previewId: s.previewId ?? live.previewId!,
              generation: gen,
              events: page.events,
              nextCursor: page.nextCursor == null ? null : String(page.nextCursor),
            }),
          );
        } catch (e) {
          if (!isCurrentRequest(findByCandidate(storeRef.current, candidateId), owner)) return;
          const message = String(e);
          commitIfCurrent(owner, (s) => ({ ...s, error: message }));
          onError?.(message);
        }
      });
    },
    [client, commitIfCurrent, onError, runKeyed],
  );

  const confirmLeaf = useCallback(
    async (candidateId: string) => {
      const slot = findByCandidate(storeRef.current, candidateId);
      const leaf = slot?.selection.pendingLeaf;
      if (!slot?.previewId || !leaf) return;
      if (sameOperationBusy(slot, "branch")) return;
      await runKeyed(candidateId, "branch", async (live, owner) => {
        try {
          const page = await client.events(live.previewId!, null, 20, leaf);
          if (!isCurrentRequest(findByCandidate(storeRef.current, candidateId), owner)) return;
          commitIfCurrent(owner, (s) =>
            applySelection(s, {
              type: "confirmLeaf",
              leaf,
              events: page.events,
              nextCursor: page.nextCursor == null ? null : String(page.nextCursor),
            }),
          );
        } catch (e) {
          if (!isCurrentRequest(findByCandidate(storeRef.current, candidateId), owner)) return;
          const message = String(e);
          commitIfCurrent(owner, (s) => ({ ...s, error: message }));
          onError?.(message);
        }
      });
    },
    [client, commitIfCurrent, onError, runKeyed],
  );

  const dispatchSelection = useCallback((candidateId: string, action: ContentSelectionAction) => {
    if (fixedFlight.current !== null) return;
    const slot = findByCandidate(storeRef.current, candidateId);
    if (!slot) return;
    commitStore(writeSlot(storeRef.current, applySelection(slot, action)));
  }, [commitStore]);

  const clearError = useCallback((candidateId: string) => {
    if (fixedFlight.current !== null) return;
    const slot = findByCandidate(storeRef.current, candidateId);
    if (!slot) return;
    commitStore(writeSlot(storeRef.current, { ...slot, error: null }));
  }, [commitStore]);

  const invalidateCandidate = useCallback((candidateId: string) => {
    if (fixedTarget.current === candidateId || fixedPrimary.current === candidateId) cancelFixedTransfer();
    const current = storeRef.current;
    const pending = { ...current.pendingByCandidate };
    delete pending[candidateId];
    const slots = Object.fromEntries(
      Object.entries(current.slotsByPreviewId).filter(([, slot]) => slot.candidateId !== candidateId),
    );
    commitStore({
      ...current,
      pendingByCandidate: pending,
      slotsByPreviewId: slots,
      retainedCandidateIds: current.retainedCandidateIds.filter((id) => id !== candidateId),
    });
    if (activeCandidateIdRef.current === candidateId) {
      setActiveCandidateId(null);
      setActivePreviewId(null);
    }
  }, [cancelFixedTransfer, commitStore, setActiveCandidateId]);

  const adoptFixedPreview = useCallback(async (
    input: FixedPreviewTransferRequest, authorize: FixedPreviewAuthority, options?: { discardEdits?: boolean },
  ): Promise<boolean> => {
    if (fixedFlight.current !== null) return false;
    fixedError.current = null;
    setFixedTransfer({ busy: false, error: null });
    const discardEdits = options?.discardEdits === true;
    let request: FixedPreviewTransferRequest;
    try { request = copyFixedPreviewTransfer(input); }
    catch (cause) { fixedError.current = String(cause); setFixedTransfer({ busy: false, error: fixedError.current }); return false; }
    const scope = storeRef.current.scope;
    if (!scope || fixedScopeKey(scope) !== fixedScopeKey(request.context.scope) ||
      storeRef.current.scanGeneration !== request.context.scope.generation) return false;
    const candidateId = request.member.candidate.candidateId;
    const original = findByCandidate(storeRef.current, candidateId);
    const priorId = request.context.primary.candidateId;
    const prior = findByCandidate(storeRef.current, priorId);
    if ((original?.busy && (!original.summary || original.busyOperation === "branch")) ||
      prior?.busyOperation === "branch" || original?.selection.pendingLeaf || prior?.selection.pendingLeaf) return false;
    const edits = () => !!findByCandidate(storeRef.current, candidateId)?.dirty || !!findByCandidate(storeRef.current, priorId)?.dirty;
    if (edits() && !discardEdits) return false;
    const ticket = ++requestSerial.current;
    const generation = lifetime.current;
    fixedFlight.current = ticket;
    fixedCommit.current = null;
    fixedError.current = null;
    fixedTarget.current = candidateId;
    fixedPrimary.current = priorId;
    // Preserve the old page/edits, but expire a competing page request permanently.
    const guarded = original ? { ...original, requestSeq: ticket, busy: false, busyOperation: "idle" as const } : undefined;
    if (guarded) commitStore(writeSlot(storeRef.current, guarded));
    const owns = () => generation === lifetime.current && fixedFlight.current === ticket &&
      fixedScopeKey(storeRef.current.scope) === fixedScopeKey(request.context.scope) &&
      findByCandidate(storeRef.current, candidateId) === guarded;
    setFixedTransfer({ busy: true, error: null });
    try {
      const page = await client.events(request.preview.previewId, null, FIXED_PREVIEW_PAGE_SIZE, request.comparison.leafId);
      if (!owns()) return false;
      const slot = fixedPreviewContentSlot(request, page, ticket);
      if (edits() && !discardEdits) return false;
      // A detached identity is passed to the domain's real CAS authority. No
      // stage body or unrestricted commit function escapes the content owner.
      if (await authorize(structuredClone(request), owns) !== true || !owns()) return false;
      if (edits() && !discardEdits) return false;
      const next = writeSlot(storeRef.current, slot);
      fixedFlight.current = null;
      fixedTarget.current = null;
      fixedPrimary.current = null;
      setFixedTransfer({ busy: false, error: null });
      commitStore({ ...next, retainedCandidateIds: [...new Set([...next.retainedCandidateIds, candidateId])] });
      fixedCommit.current = { lifetime: generation, identity: fixedTransferIdentity(request), slot };
      setActiveCandidateId(candidateId);
      setActivePreviewId(slot.previewId);
      return true;
    } catch (cause) {
      if (owns()) {
        fixedError.current = String(cause);
        setFixedTransfer({ busy: true, error: fixedError.current });
      }
      return false;
    } finally {
      if (owns()) {
        fixedFlight.current = null;
        fixedTarget.current = null;
        fixedPrimary.current = null;
        setFixedTransfer((state) => ({ ...state, busy: false }));
      }
    }
  }, [client, commitStore, setActiveCandidateId]);

  const isFixedPreviewCurrent = useCallback((request: FixedPreviewTransferRequest) => {
    const receipt = fixedCommit.current;
    return !!receipt && receipt.lifetime === lifetime.current && fixedFlight.current === null &&
      receipt.identity === fixedTransferIdentity(request) && fixedScopeKey(storeRef.current.scope) === fixedScopeKey(request.context.scope) &&
      findByCandidate(storeRef.current, request.member.candidate.candidateId) === receipt.slot;
  }, []);
  const fixedPreviewError = useCallback(() => fixedError.current, []);

  const closeWizard = useCallback(() => {
    ++lifetime.current;
    cancelFixedTransfer();
    commitStore({
      pendingByCandidate: {},
      slotsByPreviewId: {},
      retainedCandidateIds: [],
      scanGeneration: storeRef.current.scanGeneration,
      scope: null,
    });
    setActiveCandidateId(null);
    setActivePreviewId(null);
  }, [cancelFixedTransfer, commitStore, setActiveCandidateId]);

  const active = activePreviewId
    ? store.slotsByPreviewId[activePreviewId] ?? (activeCandidateId ? byCandidate[activeCandidateId] ?? null : null)
    : activeCandidateId
      ? byCandidate[activeCandidateId] ?? null
      : null;

  return {
    byCandidate,
    pendingByCandidate: store.pendingByCandidate,
    slotsByPreviewId: store.slotsByPreviewId,
    activeCandidateId,
    activePreviewId,
    active,
    setActiveCandidateId,
    ensureSelected,
    open,
    loadMore,
    confirmLeaf,
    dispatchSelection,
    clearError,
    invalidateCandidate,
    closeWizard,
    allowedSelectionOf,
    fixedTransfer,
    adoptFixedPreview,
    fixedPreviewTransfer: { run: adoptFixedPreview, isCurrent: isFixedPreviewCurrent, getError: fixedPreviewError },
    cancelFixedTransfer,
  };
}
