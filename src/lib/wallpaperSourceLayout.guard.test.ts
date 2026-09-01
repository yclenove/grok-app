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
  it("keeps grouped source controls above the full-width workspace", () => {
    expect(modalCss).toMatch(
      /\.modal\.glass-modal\.wallpaper-source-modal\s*\{[^}]*width:\s*min\(1080px,/s,
    );
    expect(sourceCss).toMatch(
      /\.wallpaper-source-layout\s*\{[^}]*display:\s*flex[^}]*flex-direction:\s*column/s,
    );
    expect(modalCss).toMatch(
      /\.wallpaper-source-tabs\s*\{[^}]*flex-direction:\s*row[^}]*overflow-x:\s*auto[^}]*border-bottom:/s,
    );
    expect(modalCss).toMatch(
      /\.wallpaper-source-tabs__group\s*\{[^}]*display:\s*inline-flex[^}]*min-width:\s*0/s,
    );
    expect(modalCss).toMatch(
      /\.wallpaper-source-tabs\s*\{[^}]*justify-content:\s*space-between/s,
    );
    expect(sourceCss).toMatch(
      /@media \(max-width: 760px\)[\s\S]*\.wallpaper-source-tabs\s*\{[^}]*flex-wrap:\s*wrap/s,
    );
  });

  it("keeps every source label visible in the scrollable strip", () => {
    expect(sourceCss).not.toMatch(
      /\.wallpaper-source-tabs__label\s*\{[^}]*(?:clip:\s*rect|width:\s*1px|position:\s*absolute)/s,
    );
  });
});
