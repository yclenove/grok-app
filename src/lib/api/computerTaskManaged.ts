/** An explicit task gesture creates an owned browser; it does not select a tab
 * from the user's existing browser or accept model-supplied native identities. */
import { invoke, isDesktopHost } from "./host";
import type { ComputerTaskKey } from "./computerTask";
import type { ComputerTaskApproval } from "./computerTaskPicker";

export async function computerTaskManagedApprove(sessionId: string, key: ComputerTaskKey): Promise<ComputerTaskApproval> {
  if (!isDesktopHost()) throw new Error("Computer Use requires the local desktop host");
  const { connectionId, taskId, requestId, revision } = key;
  return invoke("computer_use_task_managed_approve", { sessionId, key: { connectionId, taskId, requestId, revision } });
}
