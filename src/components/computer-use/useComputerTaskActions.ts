import { useCallback, useSyncExternalStore } from "react";
import {
  getTaskActions, getTaskActionSnapshot, subscribeTaskActions,
} from "@/lib/computer-use/taskActionStore";
import { EMPTY_TASK_ACTION_STATE } from "@/lib/computer-use/taskActions";

export function useComputerTaskActions(sessionId: string | null) {
  const subscribe = useCallback((listener: () => void) => subscribeTaskActions(sessionId, listener), [sessionId]);
  const read = useCallback(() => getTaskActionSnapshot(sessionId), [sessionId]);
  const state = useSyncExternalStore(subscribe, read, () => EMPTY_TASK_ACTION_STATE);
  const actions = useCallback(() => getTaskActions(sessionId), [sessionId]);
  return { state, actions };
}
