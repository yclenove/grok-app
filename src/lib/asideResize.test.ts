import { describe, expect, it } from "vitest";
import { asideResizeKey, resolveAsideResize } from "./asideResize";

const windowsFrame = { viewportWidth: 1202, sidebarOccupiedWidth: 268, windowControlsInset: 138 };

describe("aside keyboard resize geometry", () => {
  it("matches native Windows chrome floor and reserves the chat column", () => {
    expect(resolveAsideResize(500, { kind: "minimum" }, windowsFrame)).toBe(458);
    expect(resolveAsideResize(500, { kind: "maximum" }, windowsFrame)).toBe(574);
    expect(resolveAsideResize(500, { kind: "delta", pixels: 1000 }, windowsFrame)).toBe(574);
    expect(resolveAsideResize(500, { kind: "delta", pixels: -1000 }, windowsFrame)).toBe(458);
  });

  it("uses the native frame inset rather than hardcoding a Windows minimum", () => {
    expect(resolveAsideResize(500, { kind: "minimum" }, { ...windowsFrame, windowControlsInset: 0 })).toBe(400);
    expect(resolveAsideResize(500, { kind: "maximum" }, { ...windowsFrame, sidebarOccupiedWidth: 0 })).toBe(842);
  });

  it("fits squeezed windows using the existing layout contract", () => {
    const squeezed = { ...windowsFrame, viewportWidth: 900 };
    expect(resolveAsideResize(500, { kind: "minimum" }, squeezed)).toBe(272);
    expect(resolveAsideResize(500, { kind: "maximum" }, squeezed)).toBe(272);
  });

  it("maps only the documented keys and moves the edge in the arrow direction", () => {
    expect(asideResizeKey("ArrowLeft", false)).toEqual({ kind: "delta", pixels: 10 });
    expect(asideResizeKey("ArrowRight", false)).toEqual({ kind: "delta", pixels: -10 });
    expect(asideResizeKey("ArrowLeft", true)).toEqual({ kind: "delta", pixels: 50 });
    expect(asideResizeKey("ArrowRight", true)).toEqual({ kind: "delta", pixels: -50 });
    expect(asideResizeKey("Home", false)).toEqual({ kind: "minimum" });
    expect(asideResizeKey("End", false)).toEqual({ kind: "maximum" });
    for (const key of ["Tab", "Enter", "Escape", "ArrowUp", "ArrowDown", "a"]) {
      expect(asideResizeKey(key, false)).toBeNull();
    }
  });
});
