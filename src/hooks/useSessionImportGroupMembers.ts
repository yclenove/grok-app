import { useCallback, useEffect, useRef, useState } from "react";
import { sessionImportScanGroupMembers, type SourceGroup } from "@/lib/api/sessionMigrationGroups";
import { emptyImportGroupMembers, validateGroupMemberPage, type ImportGroupMemberState } from "@/lib/sessionImport/groupMemberState";
import { GROUP_UI_PAGE_SIZE, validateGroupPage, validateGroupScope, type ImportGroupScope } from "@/lib/sessionImport/groupSelectionState";

export type SessionImportGroupMembersClient = { members: typeof sessionImportScanGroupMembers };
const liveClient: SessionImportGroupMembersClient = { members: sessionImportScanGroupMembers };
type Options = { onError?: (message: string) => void };

/** Lazy metadata only. Open/close and each page own their replies independently. */
export function useSessionImportGroupMembers(client: SessionImportGroupMembersClient = liveClient, options: Options = {}) {
  const [state, setState] = useState<ImportGroupMemberState>(emptyImportGroupMembers);
  const current = useRef(state);
  const lifetime = useRef(0);
  const requestSerial = useRef(0);
  const cursors = useRef<Array<string | undefined>>([undefined]);
  const lastAttempt = useRef(0);
  const commit = useCallback((next: ImportGroupMemberState) => { current.current = next; setState(next); }, []);
  const close = useCallback(() => {
    ++lifetime.current;
    ++requestSerial.current;
    cursors.current = [undefined];
    lastAttempt.current = 0;
    commit(emptyImportGroupMembers());
  }, [commit]);
  useEffect(() => () => {
    ++lifetime.current;
    ++requestSerial.current;
    current.current = emptyImportGroupMembers();
  }, []);
  const report = useCallback((message: string) => {
    commit({ ...current.current, error: message });
    options.onError?.(message);
  }, [commit, options.onError]);

  const readPage = useCallback(async (index: number): Promise<boolean> => {
    const snapshot = current.current;
    const { scope, group } = snapshot;
    if (!scope || !group || snapshot.loading) return false;
    const generation = lifetime.current;
    const request = ++requestSerial.current;
    const owns = () => lifetime.current === generation && requestSerial.current === request;
    lastAttempt.current = index;
    commit({ ...snapshot, loading: true, error: null });
    try {
      const cursor = cursors.current[index];
      const page = await client.members(scope.scanId, scope.epoch, group.groupId, {
        limit: GROUP_UI_PAGE_SIZE, ...(cursor ? { cursor } : {}),
      });
      if (!owns()) return false;
      validateGroupMemberPage(scope, group, page, snapshot);
      commit({ ...current.current, members: page.members, nextCursor: page.nextCursor,
        pageIndex: index, memberCount: page.memberCount, complete: page.complete,
        metadataVersion: page.stateVersion, ready: true, loading: false, error: null });
      return true;
    } catch (cause) {
      if (owns()) report(String(cause));
      return false;
    } finally {
      if (owns()) commit({ ...current.current, loading: false });
    }
  }, [client, commit, report]);

  const open = useCallback(async (scope: ImportGroupScope, group: SourceGroup) => {
    close();
    try {
      validateGroupScope(scope);
      validateGroupPage(scope, { version: 1, groups: [group], nextCursor: null,
        totalCount: 1, complete: true, stateVersion: 0 });
    } catch (cause) { report(String(cause)); return false; }
    commit({ ...emptyImportGroupMembers(), scope: { ...scope }, group: { ...group, firstMember: { ...group.firstMember } } });
    return readPage(0);
  }, [close, commit, readPage, report]);
  const retry = useCallback(() => readPage(lastAttempt.current), [readPage]);
  const nextPage = useCallback(async () => {
    const snapshot = current.current;
    if (!snapshot.ready || snapshot.loading || !snapshot.nextCursor || !snapshot.members.length) return false;
    const index = snapshot.pageIndex + 1;
    cursors.current[index] = snapshot.nextCursor;
    return readPage(index);
  }, [readPage]);
  const previousPage = useCallback(async () => {
    const snapshot = current.current;
    if (!snapshot.ready || snapshot.loading || snapshot.pageIndex === 0) return false;
    return readPage(snapshot.pageIndex - 1);
  }, [readPage]);
  return { ...state, open, close, retry, nextPage, previousPage };
}
