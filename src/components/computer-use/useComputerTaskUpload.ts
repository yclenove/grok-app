import { useCallback, useSyncExternalStore } from "react";
import { getTaskUploadActions, getTaskUploadSnapshot, subscribeTaskUpload } from "@/lib/computer-use/taskUploadStore";
import { EMPTY_UPLOAD_STATE } from "@/lib/computer-use/taskUpload";

export function useComputerTaskUpload(sessionId: string | null) {
  const subscribe = useCallback((listener: () => void) => subscribeTaskUpload(sessionId, listener), [sessionId]);
  const read = useCallback(() => getTaskUploadSnapshot(sessionId), [sessionId]);
  const state = useSyncExternalStore(subscribe, read, () => EMPTY_UPLOAD_STATE);
  const actions = useCallback(() => getTaskUploadActions(sessionId), [sessionId]);
  return { state, actions };
}
