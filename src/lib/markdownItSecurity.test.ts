import { execFileSync } from "node:child_process";
import { createRequire } from "node:module";
import { describe, expect, it } from "vitest";

// Exercise both installed production dependency paths, not a separately added
// test-only parser. The application explicitly enables linkify in its editor.
const appRequire = createRequire(import.meta.url);
const tiptapRequire = createRequire(appRequire.resolve("tiptap-markdown"));
const prosemirrorRequire = createRequire(
  tiptapRequire.resolve("prosemirror-markdown"),
);
const parsers = [
  ["tiptap-markdown", tiptapRequire.resolve("markdown-it")],
  ["prosemirror-markdown", prosemirrorRequire.resolve("markdown-it")],
] as const;

// GHSA-253c-mchw-3w2r: many soft-broken emails and many unregistered schemes
// used to take quadratic time with linkify enabled. A separate process gives
// the watchdog a chance to terminate a regression's synchronous event loop.
const renderPayload = `
  const MarkdownIt = require(process.argv[1]);
  const kind = process.argv[2];
  const input = kind === 'emails'
    ? 'a@b.co\\n'.repeat(40000)
    : 'a://'.repeat(80000);
  const html = new MarkdownIt({ html: false, linkify: true }).render(input);
  process.stdout.write(JSON.stringify({
    links: (html.match(/<a /g) || []).length,
    emails: (html.match(/href="mailto:a@b.co"/g) || []).length,
    schemes: html.split('a://').length - 1,
  }));
`;

// GHSA-gfr2-rv44-r23c also affects default options. One reference definition
// with many title lines must preserve its entire title without quadratic work.
const renderMultilineReference = `
  const MarkdownIt = require(process.argv[1]);
  const title = '\\n' + 'line\\n'.repeat(200000);
  const input = '[ref]: /target "' + title + '"\\n\\n[link][ref]\\n';
  const expected = '<p><a href="/target" title="' + title + '">link</a></p>\\n';
  const html = new MarkdownIt().render(input);
  process.stdout.write(JSON.stringify({ matches: html === expected }));
`;

describe.each(parsers)("%s production parser security", (_name, parser) => {
  it(
    "renders a multiline reference within an isolated process budget",
    () => {
      const output = execFileSync(
        process.execPath,
        ["-e", renderMultilineReference, parser],
        {
          encoding: "utf8",
          timeout: 10_000,
          maxBuffer: 1024 * 1024,
          windowsHide: true,
        },
      );
      expect(JSON.parse(output)).toEqual({ matches: true });
    },
    15_000,
  );

  it.each(["emails", "schemes"])(
    "renders the %s payload within an isolated process budget",
    (kind) => {
      const output = execFileSync(
        process.execPath,
        ["-e", renderPayload, parser, kind],
        {
          encoding: "utf8",
          timeout: 10_000,
          maxBuffer: 1024 * 1024,
          windowsHide: true,
        },
      );
      expect(JSON.parse(output)).toEqual(
        kind === "emails"
          ? { links: 40_000, emails: 40_000, schemes: 0 }
          : { links: 0, emails: 0, schemes: 80_000 },
      );
    },
    15_000,
  );
});
