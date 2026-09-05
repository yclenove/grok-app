import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const modalCss = readFileSync(
  resolve(__dirname, "../styles/settings.part3.css"),
  "utf8",
);
const sourceCss = readFileSync(
  resolve(__dirname, "../styles/settings.part4.css"),
  "utf8",
);

describe("wallpaper source layout guard", () => {
  it("keeps a compact source switcher above the full-width workspace", () => {
    expect(modalCss).toMatch(
      /\.modal\.glass-modal\.wallpaper-source-modal\s*\{[^}]*width:\s*min\(1440px,/s,
    );
    expect(sourceCss).toMatch(
      /\.wallpaper-source-layout\s*\{[^}]*display:\s*flex[^}]*flex-direction:\s*column/s,
    );
    expect(modalCss).toMatch(
      /\.wallpaper-source-tabs\s*\{[^}]*display:\s*grid[^}]*grid-template-columns:\s*minmax\(max-content,\s*4fr\)\s+minmax\(max-content,\s*1fr\)\s+minmax\(max-content,\s*2fr\)[^}]*overflow-x:\s*auto/s,
    );
    expect(modalCss).toMatch(
      /\.wallpaper-source-tabs__group\s*\{[^}]*display:\s*grid[^}]*grid-auto-columns:\s*minmax\(max-content,\s*1fr\)[^}]*min-width:\s*0[^}]*background:/s,
    );
    expect(sourceCss).toMatch(
      /@media \(max-width: 1200px\)[\s\S]*\.wallpaper-source-tabs\s*\{[^}]*display:\s*flex[^}]*justify-content:\s*flex-start[\s\S]*\.wallpaper-source-tabs__group\s*\{[^}]*display:\s*inline-flex[^}]*min-width:\s*max-content/s,
    );
    expect(sourceCss).toMatch(
      /@media \(min-width: 1320px\)[\s\S]*\.wallpaper-source-modal \.wallpaper-masonry--stable\s*\{[^}]*grid-template-columns:\s*repeat\(4,/s,
    );
    expect(sourceCss).toMatch(
      /\.wallpaper-masonry--stable \.wallpaper-masonry__media\s*\{[^}]*aspect-ratio:\s*auto/s,
    );
    expect(sourceCss).toMatch(
      /\.wallpaper-attribution--licensed \.wallpaper-attribution__author\s*\{[^}]*flex:\s*1 1 0/s,
    );
    expect(sourceCss).toMatch(
      /\.wallpaper-attribution--licensed \.wallpaper-attribution__license\s*\{[^}]*max-width:\s*52%/s,
    );
    expect(sourceCss).toMatch(
      /\.wallpaper-attribution:not\(\.wallpaper-attribution--licensed\)\s*\.wallpaper-attribution__source\s*\{[^}]*flex:\s*1 1 auto/s,
    );
    expect(sourceCss).toMatch(
      /\.wallpaper-imagine-source\s*\{[^}]*display:\s*grid[^}]*grid-template-columns:\s*56px minmax\(0, 1fr\) 28px/s,
    );
    expect(sourceCss).toMatch(
      /\.wallpaper-imagine-source__thumb\s*\{[^}]*width:\s*56px[^}]*height:\s*36px[^}]*overflow:\s*hidden/s,
    );
    expect(sourceCss).toMatch(
      /\.wallpaper-imagine-source__thumb img\s*\{[^}]*width:\s*100%[^}]*height:\s*100%[^}]*object-fit:\s*cover/s,
    );
    expect(sourceCss).toMatch(
      /\.wallpaper-masonry-scroll:focus-visible\s*\{[^}]*outline:\s*2px solid var\(--border-focus\)/s,
    );
    expect(sourceCss).not.toMatch(
      /\.wallpaper-masonry-scroll:focus-visible\s*\{[^}]*outline:\s*none/s,
    );
  });

  it("keeps every source label visible in the scrollable strip", () => {
    expect(modalCss).not.toMatch(
      /\.wallpaper-source-tabs__label\s*\{[^}]*(?:clip:\s*rect|width:\s*1px|position:\s*absolute)/s,
    );
  });
});
