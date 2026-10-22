/**
 * @vitest-environment jsdom
 */
import { createRequire } from "node:module";
import { describe, expect, it } from "vitest";

// Use Mermaid's installed production sanitizer, not a test-only dependency.
const appRequire = createRequire(import.meta.url);
const mermaidRequire = createRequire(appRequire.resolve("mermaid"));
const createDOMPurify = mermaidRequire("dompurify");

describe("Mermaid production DOMPurify security", () => {
  it("preserves SVG labels while removing executable markup and URLs", () => {
    const purify = createDOMPurify(window);
    const clean = purify.sanitize(
      '<svg xmlns="http://www.w3.org/2000/svg"><text>diagram</text>' +
        '<script>alert(1)</script><g onload="alert(2)"></g>' +
        '<a xlink:href="javascript:alert(3)"><text>link</text></a></svg>',
    );
    const container = document.createElement("div");
    container.innerHTML = clean;
    expect(container.querySelector("svg text")?.textContent).toBe("diagram");
    expect(container.querySelector("script, [onload], [xlink\\:href]")).toBeNull();
    expect(container.textContent).toContain("link");
  });

  // GHSA-cg2g-xh64-v4q7: removing elements can join text that previously
  // contained no complete template delimiter in any individual text node.
  it.each([false, true])(
    "removes split template expressions with RETURN_DOM=%s",
    (returnDom) => {
      const purify = createDOMPurify(window);
      const result = purify.sanitize(
        '<p>{<script></script>{7*7}<script></script>}</p>',
        { SAFE_FOR_TEMPLATES: true, RETURN_DOM: returnDom },
      );
      const container = document.createElement("div");
      if (returnDom) container.appendChild(result);
      else container.innerHTML = result;
      expect(container.querySelector("script")).toBeNull();
      expect(container.textContent).not.toContain("{{");
      expect(container.textContent).not.toContain("}}");
    },
  );

  // GHSA-mrv2-3pqr-j5jr: a hook detaching an ancestor must not leave later
  // siblings armed when sanitizing the supplied DOM tree in place.
  it.each(["uponSanitizeElement", "afterSanitizeAttributes"])(
    "sanitizes later siblings when %s removes a parent in place",
    (hook) => {
      const purify = createDOMPurify(window);
      const root = document.createElement("div");
      root.innerHTML =
        '<div><span id="trigger">remove parent</span></div>' +
        '<img id="tail" src="x" onerror="alert(1)">' +
        '<a href="javascript:alert(2)">safe label</a><script>alert(3)</script>';
      purify.addHook(hook, (node: Node) => {
        if (node instanceof Element && node.id === "trigger") {
          node.parentElement?.remove();
        }
      });
      expect(purify.sanitize(root, { IN_PLACE: true })).toBe(root);
      expect(root.querySelector("#trigger")).toBeNull();
      expect(root.querySelector("#tail")).not.toBeNull();
      expect(root.querySelector("script, [onerror], [href]")).toBeNull();
      expect(root.querySelector("a")?.textContent).toBe("safe label");
    },
  );
});
