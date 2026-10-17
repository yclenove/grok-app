/** Desktop timeline for the Computer side panel. Mirror clients cannot reach it. */

import type { PanelView, TimelineNotice } from "@/lib/computer-use/store";
import { invoke, isDesktopHost } from "./host";

export async function cuTimeline(sessionId: string): Promise<PanelView> {
  if (!isDesktopHost()) throw new Error("failed");
  return invoke<PanelView>("cu_timeline", { body: { sessionId } });
}

export function listenComputerUseTimeline(
  handler: (notice: TimelineNotice) => void,
): Promise<() => void> {
  return listenDesktop<TimelineNotice>("cu://timeline", handler);
}

async function listenDesktop<T>(event: string, handler: (payload: T) => void): Promise<() => void> {
  if (!isDesktopHost()) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  const unlisten = await listen<T>(event, (emitted) => handler(emitted.payload));
  return unlisten;
}
