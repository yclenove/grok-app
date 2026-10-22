import { useCallback, useLayoutEffect, useRef, useState } from "react";
import { sessionImportScanGroupMember, type SourceGroupMember } from "@/lib/api/sessionMigrationGroups";
import { comparisonContextKey, copyComparisonContext, validateComparisonContext,
  validateComparisonGroupMember, type ComparisonPreviewContext } from "@/lib/sessionImport/comparisonPreviewState";

type State = { member: SourceGroupMember | null; loading: boolean; error: string | null };
const empty = (): State => ({ member: null, loading: false, error: null });

/** Independent metadata owner: retries never reread A's fixed preview body. */
export function useSessionImportComparisonPrimaryMember(
  context: ComparisonPreviewContext | null, client = sessionImportScanGroupMember,
) {
  const [state, setState] = useState(empty);
  const current = useRef(state);
  const input = useRef(context);
  const active = useRef<ComparisonPreviewContext | null>(null);
  const lifetime = useRef(0);
  const serial = useRef(0);
  const owner = useRef(0);
  const key = comparisonContextKey(context);
  useLayoutEffect(() => { input.current = context; });
  const commit = useCallback((next: State) => { current.current = next; setState(next); }, []);
  useLayoutEffect(() => {
    ++lifetime.current;
    owner.current = 0;
    active.current = input.current ? copyComparisonContext(input.current) : null;
    commit(empty());
    return () => { ++lifetime.current; owner.current = 0; active.current = null; current.current = empty(); };
  }, [key, commit]);

  const load = useCallback(async (): Promise<boolean> => {
    if (!active.current || current.current.loading) return false;
    const request = copyComparisonContext(active.current);
    const generation = lifetime.current;
    const ticket = ++serial.current;
    owner.current = ticket;
    const owns = () => generation === lifetime.current && owner.current === ticket && active.current !== null;
    commit({ member: null, loading: true, error: null });
    try {
      validateComparisonContext(request);
      const member = await client(request.scope.scanId, request.scope.epoch, request.group.groupId, request.primary.candidateId);
      if (!owns()) return false;
      validateComparisonGroupMember(request, member);
      if (member.candidate.candidateId !== request.primary.candidateId ||
        member.candidate.sourceRevision !== request.primary.sourceRevision ||
        member.candidate.nativeSessionId !== request.primary.nativeSessionId) {
        throw new Error("PLAN_STALE: exact primary source metadata changed");
      }
      commit({ member: structuredClone(member), loading: false, error: null });
      return true;
    } catch (cause) {
      if (owns()) commit({ member: null, loading: false, error: String(cause) });
      return false;
    } finally {
      if (owns() && current.current.loading) commit({ ...current.current, loading: false });
    }
  }, [client, commit]);
  return { ...state, load };
}
