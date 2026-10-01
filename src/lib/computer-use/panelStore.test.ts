import { describe, expect, it } from "vitest";
import {
  openComputerUseFromSlash,
  subscribeComputerPanel,
  type ComputerPanelMode,
} from "./panelStore";

describe("openComputerUseFromSlash", () => {
  it("opens the Computer panel and never returns a chat command", () => {
    const opened: ComputerPanelMode[] = [];
    const stop = subscribeComputerPanel((mode) => {
      opened.push(mode);
    });
    expect(openComputerUseFromSlash("computer-use")).toBe(true);
    expect(openComputerUseFromSlash("computer-use-browser")).toBe(true);
    expect(openComputerUseFromSlash("yolo")).toBe(false);
    expect(openComputerUseFromSlash(undefined)).toBe(false);
    expect(opened).toEqual(["desktop", "managed-browser"]);
    stop();
  });
});
