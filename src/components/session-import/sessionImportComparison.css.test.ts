import { readFileSync } from "node:fs";
import { expect, test } from "vitest";

const css = readFileSync(new URL("../../styles/session-import-comparison.css", import.meta.url), "utf8");
test("comparison uses bounded shared surfaces, a scroll body and a non-scrolling footer", () => {
  expect(css).toMatch(/\.session-import-comparison \.modal-body\s*\{[^}]*min-height:\s*0;[^}]*overflow:\s*auto;/s);
  expect(css).toMatch(/\.session-import-comparison \.modal-actions\s*\{[^}]*flex:\s*0 0 auto;/s);
  expect(css).toContain("background: var(--glass-surface-solid)");
  expect(css).toMatch(/\.session-import__comparison-versions ul\s*\{[^}]*max-height:\s*240px;[^}]*overflow:\s*auto;/s);
});

test("complete field inspector remains bounded inside the shared scroll body", () => {
  expect(css).toMatch(/\.session-import__comparison-field-text\s*\{[^}]*max-height:\s*280px;[^}]*overflow:\s*auto;[^}]*white-space:\s*pre-wrap;/s);
  expect(css).toMatch(/\.session-import__comparison-inspector\s*\{[^}]*background:\s*var\(--glass-surface-solid\);[^}]*min-width:\s*0;/s);
});
test("wide pairs are minmax-bounded and narrow pairs stack without fixed-width text", () => {
  expect(css).toContain("grid-template-columns: repeat(2, minmax(0, 1fr))");
  expect(css).toMatch(/@container session-import-comparison \(max-width:\s*719px\)[\s\S]*grid-template-columns:\s*minmax\(0, 1fr\)/);
  expect(css).toContain("white-space: pre-wrap");
  expect(css).toContain("overflow-wrap: anywhere");
});
