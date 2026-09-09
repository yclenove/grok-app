import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));

function stripComments(css: string): string {
  return css.replace(/\/\*[\s\S]*?\*\//g, "");
}

describe("app-shell overflow clip", () => {
  it("uses overflow:clip on .app-shell so wallpaper bleed cannot scroll the frame", () => {
    const css = stripComments(
      readFileSync(join(here, "sidebar.part1.css"), "utf8"),
    );
    const shell = css.match(/\.app-shell\s*\{[^}]*\}/);
    expect(shell?.[0]).toMatch(/overflow:\s*clip/);
    expect(shell?.[0]).not.toMatch(/overflow:\s*hidden/);
  });

  it("does not globally replace html/body/#root overflow with clip", () => {
    const css = stripComments(
      readFileSync(join(here, "sidebar.part1.css"), "utf8"),
    );
    const root = css.match(/html,\s*body,\s*#root\s*\{[^}]*\}/);
    expect(root?.[0]).toMatch(/overflow:\s*hidden/);
    expect(root?.[0]).not.toMatch(/overflow:\s*clip/);
  });

  it("keeps wallpaper frost bleed as a negative inset (not a root transform)", () => {
    const skins = stripComments(readFileSync(join(here, "skins.css"), "utf8"));
    expect(skins).toMatch(
      /html\[data-wallpaper="1"\] \.app-wallpaper-media\s*\{[^}]*inset:\s*calc\(-2 \* var\(--wallpaper-sidebar-blur/s,
    );
    expect(skins).not.toMatch(/html\s*\{[^}]*transform:/s);
  });
});
