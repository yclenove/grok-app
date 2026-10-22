import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const css = readFileSync(new URL("../styles/chat.part6.css", import.meta.url), "utf8");

describe("Select keyboard focus styling", () => {
  it("keeps the closed trigger's keyboard focus visible with the shared theme token", () => {
    const rule = css.match(/\.c-select__trigger:focus-visible\s*\{([^}]+)\}/)?.[1];
    expect(rule).toContain("outline: 2px solid var(--border-focus");
    expect(rule).toContain("outline-offset: 2px");
  });

  it("draws an inset focus ring inside the portaled scrolling menu, including empty menus", () => {
    const rule = css.match(/\.c-select__menu:focus-visible,\s*\.c-select__option:focus-visible\s*\{([^}]+)\}/)?.[1];
    expect(rule).toContain("outline: 2px solid var(--border-focus");
    expect(rule).toContain("outline-offset: -2px");
  });
});
