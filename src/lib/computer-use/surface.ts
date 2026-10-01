/** Product Computer Use surfaces. Wire strings match Host/MCP. */

import {
  openSideTab,
  type OpenSideTabResult,
  type SideTab,
  type SideWorkbenchState,
} from "@/lib/sideWorkbench";

export const COMPUTER_SURFACES = [
  "desktop",
  "managed-browser",
  "existing-tabs",
  "app-webview",
] as const;

export type ComputerSurface = (typeof COMPUTER_SURFACES)[number];

export function isComputerSurface(value: string | undefined | null): value is ComputerSurface {
  return !!value && (COMPUTER_SURFACES as readonly string[]).includes(value);
}

export function surfaceFromSlash(action: string | undefined): ComputerSurface | null {
  if (action === "computer-use") return "desktop";
  if (action === "computer-use-browser") return "managed-browser";
  return null;
}

export function surfaceLabelKey(
  surface: ComputerSurface,
):
  | "cu.surface.desktop"
  | "cu.surface.managedBrowser"
  | "cu.surface.existingTabs"
  | "cu.surface.appWebview" {
  switch (surface) {
    case "desktop":
      return "cu.surface.desktop";
    case "managed-browser":
      return "cu.surface.managedBrowser";
    case "existing-tabs":
      return "cu.surface.existingTabs";
    case "app-webview":
      return "cu.surface.appWebview";
  }
}

export function computerTabSurface(tab: SideTab | null | undefined): ComputerSurface | null {
  if (!tab || tab.kind !== "computer") return null;
  return isComputerSurface(tab.surface) ? tab.surface : "desktop";
}

export type ApplyComputerPanelResult = OpenSideTabResult & {
  surfaceChanged: boolean;
  previousSurface?: ComputerSurface;
};

export function applyComputerPanelOpen(
  state: SideWorkbenchState,
  requestedSurface: ComputerSurface | "current",
): ApplyComputerPanelResult {
  const current = state.tabs.find((t) => t.kind === "computer");
  const previous = current && current.kind === "computer" ? current.surface : undefined;
  const surface = requestedSurface === "current" ? computerTabSurface(current) ?? "desktop" : requestedSurface;
  const next = openSideTab(state, "computer", { surface });
  return {
    ...next,
    surfaceChanged: previous != null && previous !== surface,
    previousSurface: previous && isComputerSurface(previous) ? previous : undefined,
  };
}
