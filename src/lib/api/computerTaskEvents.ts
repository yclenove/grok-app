/** Main desktop WebView Channel; metadata only, no mirror broadcast. */
import { invoke, isDesktopHost } from "./host";
import type { ComputerTaskView } from "./computerTask";

export type ComputerTaskSubscription = {
  subscriptionId: string;
  sessionId: string;
  connectionId: string;
  hostInstanceId: string;
};

export type ComputerTaskEvent = ComputerTaskSubscription & { sequence: string } & (
  | { kind: "view" | "heartbeat"; view: ComputerTaskView }
  | { kind: "retired" }
);

export async function computerTaskSubscribe(
  sessionId: string,
  onEvent: (event: ComputerTaskEvent) => void,
): Promise<ComputerTaskSubscription> {
  if (!isDesktopHost()) throw new Error("Computer Use requires the local desktop host");
  const { Channel } = await import("@tauri-apps/api/core");
  const events = new Channel<ComputerTaskEvent>((event) => {
    try { onEvent(event); } finally {
      // Flow control only. This acknowledges receipt, never task consent.
      void invoke<void>("computer_use_task_acknowledge", {
        sessionId, subscriptionId: event.subscriptionId, sequence: event.sequence,
      }).catch(() => {});
    }
  });
  return invoke<ComputerTaskSubscription>("computer_use_task_subscribe", { sessionId, events });
}

export async function computerTaskUnsubscribe(sessionId: string, subscriptionId: string): Promise<boolean> {
  if (!isDesktopHost()) throw new Error("Computer Use requires the local desktop host");
  return invoke<boolean>("computer_use_task_unsubscribe", { sessionId, subscriptionId });
}
