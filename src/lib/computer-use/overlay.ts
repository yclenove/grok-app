/** Local overlay transport. No engine endpoint, token, media or approval access. */
import { invoke } from "@tauri-apps/api/core";

export type CursorKind = "pointer" | "caret" | "drag" | "scroll";
export type OverlayFrame = {
  phase: "hidden" | "running" | "foreground" | "stopped";
  message: string;
  connected: boolean;
  cursor: { x: number; y: number; kind: CursorKind } | null;
  revision: number;
  response_ms: number;
  stop_reason?: "hotkey" | "panel" | "internal_error";
};
export type ControlAction = "stop" | "resume";
export const emptyOverlay: OverlayFrame = {
  phase: "hidden", message: "", connected: false, cursor: null, revision: 0, response_ms: 350,
};

/** Analytic critically damped cursor, matching route A's 0.35 second response. */
export function sampleCursor(
  from: { x: number; y: number }, to: { x: number; y: number }, elapsedMs: number, reduced = false, responseMs = 350,
): { x: number; y: number } {
  if (reduced) return to;
  const t = Math.max(0, elapsedMs) / 1000;
  const response = Number.isFinite(responseMs) ? Math.min(2000, Math.max(50, responseMs)) / 1000 : 0.35;
  const decay = (1 + 8 / response * t) * Math.exp(-8 / response * t);
  return { x: to.x + (from.x - to.x) * decay, y: to.y + (from.y - to.y) * decay };
}

/** Only browser-delivered user gestures can reach the trusted Resume command. */
export async function overlayControl(action: ControlAction, event: { isTrusted: boolean }): Promise<void> {
  if (!event.isTrusted) throw new Error("untrusted");
  await invoke(action === "stop" ? "cu_stop" : "cu_resume");
}

/** Poll a bounded snapshot; overlay has no global event-listen/emit capability.
 * At most one invocation is outstanding. Disposal drops every late response.
 */
export function watchOverlay(onFrame: (frame: OverlayFrame) => void): () => void {
  let disposed = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const poll = async () => {
    try {
      const frame = await invoke<OverlayFrame>("cu_overlay_snapshot");
      if (!disposed) onFrame(frame);
    } catch {
      if (!disposed) onFrame({ ...emptyOverlay, phase: "stopped" });
    } finally {
      if (!disposed) timer = setTimeout(() => void poll(), 50);
    }
  };
  void poll();
  return () => { disposed = true; clearTimeout(timer); };
}
