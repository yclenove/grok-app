import { useCallback, useLayoutEffect, useRef, useState } from "react";
import { sessionImportPreview, type SourceCandidate } from "@/lib/api/sessionMigration";
import type { SourceGroupMember } from "@/lib/api/sessionMigrationGroups";
import {
  comparisonContextKey, comparisonPairOf, comparisonPreviewSummary, copyComparisonContext,
  emptyComparisonPreviews, validateComparisonContext, validateComparisonMember,
  type ComparisonPreviewContext, type ComparisonPreviewSlot, type ComparisonSideName,
} from "@/lib/sessionImport/comparisonPreviewState";

export type ComparisonPreviewClient = { preview: typeof sessionImportPreview };
const liveClient: ComparisonPreviewClient = { preview: sessionImportPreview };

/** Separate read-only preview owners; neither comparison nor branch choice edits import selection. */
export function useSessionImportComparisonPreviews(
  context: ComparisonPreviewContext | null, client: ComparisonPreviewClient = liveClient,
) {
  const [state, setState] = useState(emptyComparisonPreviews);
  const current = useRef(state);
  const active = useRef<ComparisonPreviewContext | null>(null);
  const lifetime = useRef(0);
  const serial = useRef(0);
  const owners = useRef({ left: 0, right: 0 });
  const key = comparisonContextKey(context);
  const input = useRef(context);
  useLayoutEffect(() => { input.current = context; });
  const commit = useCallback((next: typeof state) => { current.current = next; setState(next); }, []);
  const close = useCallback(() => {
    ++lifetime.current;
    owners.current = { left: 0, right: 0 };
    commit(emptyComparisonPreviews());
    // Release UI refs only. Native fixed snapshots follow their existing lease
    // expiry/verified cleanup; a late native success is not deleted by path here.
  }, [commit]);
  useLayoutEffect(() => {
    close();
    active.current = input.current ? copyComparisonContext(input.current) : null;
    return () => {
      ++lifetime.current;
      owners.current = { left: 0, right: 0 };
      active.current = null;
      current.current = emptyComparisonPreviews();
    };
  }, [key, close]);

  const load = useCallback(async (side: ComparisonSideName, candidate: SourceCandidate, member: SourceGroupMember | null): Promise<boolean> => {
    const snapshot = current.current;
    if (!snapshot.open || !snapshot.context || snapshot[side]?.loading) return false;
    const generation = lifetime.current;
    const owner = ++serial.current;
    owners.current[side] = owner;
    const owns = () => generation === lifetime.current && owners.current[side] === owner && current.current.open;
    const slot: ComparisonPreviewSlot = { candidate: { ...candidate }, member: member ? structuredClone(member) : null,
      previewId: null, summary: null, adapter: null, leafId: null, loading: true, error: null };
    commit({ ...snapshot, [side]: slot, error: null });
    try {
      const response = await client.preview(slot.candidate.candidateId, slot.candidate.sourceRevision);
      if (!owns()) return false;
      const summary = comparisonPreviewSummary(response);
      const other = current.current[side === "left" ? "right" : "left"];
      if (other?.previewId === response.previewId) throw new Error("PLAN_STALE: comparison previews share one snapshot identity");
      commit({ ...current.current, [side]: { ...slot, previewId: response.previewId, adapter: response.adapter,
        summary, leafId: summary.leaf, loading: false } });
      return true;
    } catch (cause) {
      if (owns()) commit({ ...current.current, [side]: { ...slot, loading: false, error: String(cause) } });
      return false;
    } finally {
      if (owns()) commit({ ...current.current, [side]: { ...current.current[side]!, loading: false } });
    }
  }, [client, commit]);

  const start = useCallback(async () => {
    const context = active.current;
    if (!context || current.current.left?.loading) return false;
    close();
    const pinned = copyComparisonContext(context);
    try { validateComparisonContext(pinned); } catch (cause) {
      commit({ ...emptyComparisonPreviews(), open: true, error: String(cause) });
      return false;
    }
    commit({ ...emptyComparisonPreviews(), open: true, context: pinned });
    return load("left", pinned.primary, null);
  }, [close, commit, load]);
  const chooseRight = useCallback(async (member: SourceGroupMember) => {
    const snapshot = current.current;
    if (!snapshot.open || !snapshot.context || snapshot.right?.loading) return false;
    try { validateComparisonMember(snapshot.context, member); } catch (cause) {
      commit({ ...snapshot, error: String(cause) });
      return false;
    }
    return load("right", member.candidate, member);
  }, [commit, load]);
  const retry = useCallback((side: ComparisonSideName) => {
    const slot = current.current[side];
    if (!slot?.error || slot.loading) return Promise.resolve(false);
    return load(side, slot.candidate, slot.member);
  }, [load]);
  const chooseLeaf = useCallback((side: ComparisonSideName, leafId: string) => {
    const slot = current.current[side];
    if (!slot || slot.loading || slot.error || !slot.summary?.leaves.includes(leafId)) return false;
    commit({ ...current.current, [side]: { ...slot, leafId } });
    return true;
  }, [commit]);
  const getPair = useCallback(() => comparisonPairOf(current.current), []);
  return { ...state, pair: comparisonPairOf(state), getPair, start, close, chooseRight, retry, chooseLeaf };
}
