import { useCallback, useLayoutEffect, useRef, useState } from "react";
import {
  sessionImportEntries, sessionImportEntry, type CatalogEntry, type CatalogPage,
  type PrepareBatchItem, type PrepareBatchStatus,
} from "@/lib/api/sessionMigration";
import {
  sessionImportPreparedAssociationInspect, type IdentityAssociationConfirmation,
  type PreparedAssociationReview,
} from "@/lib/api/sessionMigrationIdentityAssociation";

export type ConfirmedHistoryLink = {
  candidateId: string;
  title: string;
  targetTitle: string;
  confirmation: IdentityAssociationConfirmation;
};
type ReviewDialog = {
  member: PrepareBatchItem;
  target: CatalogEntry | null;
  review: PreparedAssociationReview | null;
  ready: boolean;
};
export type AssociationClient = {
  entries: typeof sessionImportEntries;
  entry: typeof sessionImportEntry;
  inspect: typeof sessionImportPreparedAssociationInspect;
};
const liveClient: AssociationClient = {
  entries: (...args) => sessionImportEntries(...args),
  entry: (...args) => sessionImportEntry(...args),
  inspect: (...args) => sessionImportPreparedAssociationInspect(...args),
};

/** Reviews are bound to the actual prepared version. No inferred title matches. */
export function useSessionImportAssociations(
  batch: PrepareBatchStatus | null, active: boolean, isLocked: () => boolean,
  client: AssociationClient = liveClient,
) {
  const scope = batch && ["completed", "completed-with-issues"].includes(batch.state)
    ? JSON.stringify([batch.batchId, batch.stateVersion]) : "";
  const [confirmed, setConfirmed] = useState<ConfirmedHistoryLink[]>([]);
  const [dialog, setDialog] = useState<ReviewDialog | null>(null);
  const [targets, setTargets] = useState<CatalogPage | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const current = useRef({ scope, active, batch, isLocked });
  const links = useRef(confirmed);
  const review = useRef(dialog);
  const page = useRef(targets);
  const generation = useRef(0);
  const flight = useRef<number | null>(null);
  const update = useCallback((next: ReviewDialog | null) => { review.current = next; setDialog(next); }, []);
  const close = useCallback(() => {
    ++generation.current; flight.current = null;
    update(null); page.current = null; setTargets(null); setBusy(false); setError(null);
  }, [update]);
  useLayoutEffect(() => {
    const previous = current.current;
    current.current = { scope, active, batch, isLocked };
    if (previous.scope !== scope) {
      links.current = []; setConfirmed([]); close();
    } else if (previous.active && !active) close();
  }, [scope, active, batch, isLocked, close]);
  useLayoutEffect(() => () => { ++generation.current; flight.current = null; }, []);
  const editable = useCallback(() => {
    const owner = current.current;
    return !!owner.scope && owner.active && !owner.isLocked();
  }, []);
  const loadTargets = useCallback(async (cursor: string | null = null) => {
    if (!editable() || !review.current || flight.current !== null) return;
    const token = ++generation.current;
    flight.current = token; setBusy(true); setError(null);
    try {
      const next = await client.entries(cursor, 20, { provider: "cursor" });
      if (generation.current !== token || !editable()) return;
      // Replace the visible page; never grow a hidden unbounded target list.
      page.current = next; setTargets(next);
    } catch (cause) {
      if (generation.current === token && editable()) setError(String(cause));
    } finally {
      if (flight.current === token) { flight.current = null; setBusy(false); }
    }
  }, [client, editable]);
  const open = useCallback((member: PrepareBatchItem) => {
    if (!editable() || review.current || member.state !== "prepared" || member.provider !== "cursor" || !member.previewId) return;
    close();
    update({ member: { ...member }, target: null, review: null, ready: false });
    void loadTargets();
  }, [close, editable, loadTargets, update]);
  const choose = useCallback(async (importId: string) => {
    const original = review.current;
    const owner = current.current;
    if (!editable() || !original || !owner.batch || flight.current !== null) return;
    update({ ...original, target: null, review: null, ready: false });
    setError(null);
    if (!importId) return;
    const token = ++generation.current;
    flight.current = token; setBusy(true); setError(null);
    try {
      if (links.current.some((link) => link.candidateId !== original.member.candidateId && link.confirmation.importId === importId)) {
        throw new Error("MAPPING_CONFLICT: target is already assigned in this batch");
      }
      const target = await client.entry(importId);
      if (generation.current !== token || !editable()) return;
      if (!target || target.hidden || target.provider !== "cursor" || !target.currentRevision) {
        throw new Error("PLAN_STALE: chosen history is unavailable");
      }
      update({ ...original, target, review: null, ready: false });
      const inspected = await client.inspect({
        version: 1, batchId: owner.batch.batchId, expectedStateVersion: owner.batch.stateVersion,
        candidateId: original.member.candidateId, previewId: original.member.previewId!,
        importId, expectedRevision: target.currentRevision,
      });
      if (generation.current !== token || !editable()) return;
      if (inspected.batchId !== owner.batch.batchId || inspected.stateVersion !== owner.batch.stateVersion ||
          inspected.candidateId !== original.member.candidateId || inspected.review.previewId !== original.member.previewId ||
          inspected.review.importId !== importId || inspected.review.expectedRevision !== target.currentRevision ||
          inspected.review.version !== 1 || !inspected.leafId) throw new Error("PLAN_STALE: review identity changed");
      update({ ...original, target, review: inspected, ready: false });
    } catch (cause) {
      if (generation.current === token && editable()) setError(String(cause));
    } finally {
      if (flight.current === token) { flight.current = null; setBusy(false); }
    }
  }, [client, editable, update]);
  const markReady = useCallback((digest: string, ready: boolean) => {
    const next = review.current;
    if (next?.review?.review.reviewDigest === digest && next.ready !== ready) update({ ...next, ready });
  }, [update]);
  const confirm = useCallback(() => {
    const next = review.current;
    if (!editable() || flight.current !== null || !next?.ready || !next.review || !next.target) return false;
    const inspected = next.review.review;
    if (links.current.some((link) => link.candidateId !== next.member.candidateId && link.confirmation.importId === inspected.importId)) return false;
    const others = links.current.filter((link) => link.candidateId !== next.member.candidateId);
    if (others.length >= 200) { setError("RESOURCE_LIMIT: too many historical associations"); return false; }
    const updated = [...others, { candidateId: next.member.candidateId, title: next.member.title,
      targetTitle: next.target.alias || next.target.title,
      confirmation: { version: 1 as const, importId: inspected.importId, expectedRevision: inspected.expectedRevision,
        previewId: inspected.previewId, reviewDigest: inspected.reviewDigest } }];
    links.current = updated; setConfirmed(updated); close(); return true;
  }, [close, editable]);
  const clear = useCallback((candidateId: string) => {
    if (!editable() || review.current) return;
    links.current = links.current.filter((link) => link.candidateId !== candidateId);
    setConfirmed(links.current);
  }, [editable]);
  const snapshot = useCallback(() => {
    if (!current.current.scope || review.current || flight.current !== null) throw new Error("PLAN_STALE: finish reviewing historical associations first");
    return links.current.map((link) => ({ ...link.confirmation }));
  }, []);
  return { confirmed, dialog, targets, busy, error, open, close, choose, markReady, confirm, clear, snapshot,
    isReviewing: () => !!review.current,
    nextTargets: () => loadTargets(page.current?.nextCursor ?? null),
    firstTargets: () => loadTargets(),
  };
}
