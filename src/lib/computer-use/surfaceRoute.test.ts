import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { emptySideWorkbenchState, type SideWorkbenchState } from "@/lib/sideWorkbench";
import {
  openComputerUseFromSlash,
  requestComputerPanel,
  subscribeComputerPanel,
} from "./panelStore";
import {
  applyComputerPanelOpen,
  computerTabSurface,
  surfaceFromSlash,
} from "./surface";

const here = dirname(fileURLToPath(import.meta.url));

function computerSurface(state: SideWorkbenchState) {
  const tab = state.tabs.find((t) => t.kind === "computer");
  return computerTabSurface(tab ?? null);
}

describe("product surface routing", () => {
  it("maps slash actions to distinct surfaces", () => {
    expect(surfaceFromSlash("computer-use")).toBe("desktop");
    expect(surfaceFromSlash("computer-use-browser")).toBe("managed-browser");
    expect(surfaceFromSlash("yolo")).toBeNull();
  });

  it("slash subscribe keeps mode on the computer tab and does not collapse to desktop", () => {
    let state = emptySideWorkbenchState();
    const stop = subscribeComputerPanel((mode) => {
      state = applyComputerPanelOpen(state, mode);
    });
    expect(openComputerUseFromSlash("computer-use")).toBe(true);
    expect(computerSurface(state)).toBe("desktop");
    expect(openComputerUseFromSlash("computer-use-browser")).toBe(true);
    expect(computerSurface(state)).toBe("managed-browser");
    const browserTab = state.tabs.find((t) => t.kind === "computer");
    expect(browserTab).toMatchObject({ kind: "computer", surface: "managed-browser" });
    stop();
  });

  it("alternating slashes 20 times never lose managed-browser or flash desktop", () => {
    let state = emptySideWorkbenchState();
    const stop = subscribeComputerPanel((mode) => {
      state = applyComputerPanelOpen(state, mode);
    });
    const seen: string[] = [];
    for (let i = 0; i < 20; i += 1) {
      const action = i % 2 === 0 ? "computer-use" : "computer-use-browser";
      openComputerUseFromSlash(action);
      const surface = computerSurface(state);
      seen.push(surface ?? "missing");
      expect(surface).toBe(i % 2 === 0 ? "desktop" : "managed-browser");
    }
    expect(seen).toHaveLength(20);
    expect(new Set(seen)).toEqual(new Set(["desktop", "managed-browser"]));
    stop();
  });

  it("opening a task workspace preserves its existing browser surface", () => {
    let state: SideWorkbenchState = applyComputerPanelOpen(emptySideWorkbenchState(), "existing-tabs");
    let surfaceChanged = false;
    const stop = subscribeComputerPanel((mode) => {
      const next = applyComputerPanelOpen(state, mode);
      state = next;
      surfaceChanged = next.surfaceChanged;
    });
    requestComputerPanel();
    expect(computerSurface(state)).toBe("existing-tabs");
    expect(surfaceChanged).toBe(false);
    stop();
  });

  it("WorkbenchResourcesAside subscription forwards slash mode into applyComputerPanelOpen", () => {
    const aside = readFileSync(
      join(here, "../../app/WorkbenchResourcesAside.tsx"),
      "utf8",
    );
    expect(aside).toContain("applyComputerPanelOpen");
    expect(aside).toContain("subscribeComputerPanel(");
    expect(aside).not.toMatch(/subscribeComputerPanel\(\(\s*\)\s*=>/);
  });

  it("ComputerPanel is given the tab surface", () => {
    const panelHost = readFileSync(
      join(here, "../../components/side-workbench/SideWorkbench.tsx"),
      "utf8",
    );
    expect(panelHost).toMatch(/surface=\{/);
    expect(panelHost).toContain("ComputerPanel");
  });
});
