/** @vitest-environment jsdom */
import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { MarkdownBody } from "./MarkdownBody";

describe("MarkdownBody inert imported content", () => {
  it("formats headings, GFM tables and code without changing the source text", () => {
    const html = renderToStaticMarkup(
      <MarkdownBody inert>{"## 中文标题\n\n**重点**\n\n| a | b |\n| - | - |\n| 1 | 2 |\n\n```ts\nconst n = 1;\n```"}</MarkdownBody>,
    );
    expect(html).toContain("<h2>中文标题</h2>");
    expect(html).toContain("<strong>重点</strong>");
    expect(html).toContain("<table>");
    expect(html).toContain('class="language-ts"');
    expect(html).toContain("const n = 1;");
  });

  it("never renders navigable links, remote or local media, raw HTML, or Mermaid actions", () => {
    const source = [
      "[remote](https://example.invalid/tracker) [local](/private/secret.png)",
      "[command](grok://run) <https://example.invalid/auto>",
      "![remote image](https://example.invalid/image.png)",
      "![local image](/private/secret.png)",
      "![inline image](data:image/png;base64,AAAA)",
      "`/private/secret.png`",
      '<img src="https://example.invalid/raw" onerror="alert(1)">',
      "<script>alert(1)</script>",
      "```mermaid\ngraph TD; A-->B\n```",
    ].join("\n\n");
    const html = renderToStaticMarkup(
      <MarkdownBody inert imagePathMap={{ "/private/secret.png": "/private/secret.png" }}>{source}</MarkdownBody>,
    );
    expect(html).not.toMatch(/<(?:a|img|video|iframe|script|button|svg)\b/);
    const doc = new DOMParser().parseFromString(html, "text/html");
    expect(doc.querySelectorAll("[href], [src], [onerror]")).toHaveLength(0);
    expect(html).toContain("remote image");
    expect(html).toContain("local image");
    expect(html).toContain("graph TD; A--&gt;B");
  });
});
