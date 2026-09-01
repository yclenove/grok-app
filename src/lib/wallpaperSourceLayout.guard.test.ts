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
  it("uses a labeled source rail beside the desktop workspace", () => {
    expect(modalCss).toMatch(
      /\.modal\.glass-modal\.wallpaper-source-modal\s*\{[^}]*width:\s*min\(1080px,/s,
    );
    expect(sourceCss).toMatch(
      /\.wallpaper-source-layout\s*\{[^}]*grid-template-columns:\s*156px minmax\(0, 1fr\)/s,
    );
    expect(modalCss).toMatch(
      /\.wallpaper-source-tabs\s*\{[^}]*flex-direction:\s*column[^}]*border-right:/s,
    );
  });

  it("keeps source labels visible in a horizontally scrollable narrow layout", () => {
    expect(sourceCss).toMatch(
      /@media \(max-width: 720px\)[\s\S]*?\.wallpaper-source-tabs\s*\{[^}]*flex-direction:\s*row[^}]*overflow-x:\s*auto/s,
    );
    expect(sourceCss).not.toMatch(
      /\.wallpaper-source-tabs__label\s*\{[^}]*(?:clip:\s*rect|width:\s*1px|position:\s*absolute)/s,
    );
  });
});
