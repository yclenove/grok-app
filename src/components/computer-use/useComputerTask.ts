import { useCallback, useSyncExternalStore } from "react";
import {
  EMPTY_TASK_SNAPSHOT, getTaskSessionSnapshot, subscribeTaskSession,
} from "@/lib/computer-use/taskSource";

/** Cards and panels share one original Host stream for this App session. */
export function useComputerTask(sessionId: string | null) {
  const subscribe = useCallback((listener: () => void) => subscribeTaskSession(sessionId, listener), [sessionId]);
  const read = useCallback(() => getTaskSessionSnapshot(sessionId), [sessionId]);
  return useSyncExternalStore(subscribe, read, () => EMPTY_TASK_SNAPSHOT);
}
