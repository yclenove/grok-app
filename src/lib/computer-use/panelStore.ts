/** Cross-surface Computer panel open requests. Avoids AppWorkbench state. */

import { surfaceFromSlash, type ComputerSurface } from "./surface";

export type ComputerPanelMode = ComputerSurface | "current";

type Listener = (mode: ComputerPanelMode, token: number) => void;

let token = 0;
const listeners = new Set<Listener>();

/** Opening a task card must not silently switch the existing surface. */
export function requestComputerPanel(mode: ComputerPanelMode = "current"): number {
  token += 1;
  for (const fn of listeners) fn(mode, token);
  return token;
}

/** Composer slash must open the panel workflow, never insert a chat command. */
export function openComputerUseFromSlash(action: string | undefined): boolean {
  const surface = surfaceFromSlash(action);
  if (!surface) return false;
  requestComputerPanel(surface);
  return true;
}

export function subscribeComputerPanel(fn: Listener): () => void {
  listeners.add(fn);
  return () => {
    listeners.delete(fn);
  };
}

declare global {
  interface Window {
    __grokOpenComputerUseFromSlash?: typeof openComputerUseFromSlash;
  }
}

if (typeof window !== "undefined") {
  window.__grokOpenComputerUseFromSlash = openComputerUseFromSlash;
}
