import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import * as ts from "typescript";
import { describe, expect, it } from "vitest";

const read = (path: string) => readFileSync(resolve(__dirname, path), "utf8").replace(/\r\n/g, "\n");

function nodes<T extends ts.Node>(root: ts.Node, matches: (node: ts.Node) => node is T): T[] {
  const result: T[] = [];
  function visit(node: ts.Node) {
    if (matches(node)) result.push(node);
    ts.forEachChild(node, visit);
  }
  visit(root);
  return result;
}

function parse(path: string) {
  return ts.createSourceFile(path, read(path), ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
}

function attribute(element: ts.JsxElement, name: string) {
  return element.openingElement.attributes.properties.find(
    (property): property is ts.JsxAttribute => ts.isJsxAttribute(property) && property.name.getText() === name,
  )?.initializer;
}

function selectedTab(condition: ts.Expression) {
  return ts.isBinaryExpression(condition)
    && ts.isIdentifier(condition.left) && condition.left.text === "tab"
    && condition.operatorToken.kind === ts.SyntaxKind.EqualsEqualsEqualsToken
    && ts.isStringLiteral(condition.right)
    ? condition.right.text : undefined;
}

function extensionsSurface() {
  const candidates = nodes(parse("../components/ExtensionsPanel.tsx"), ts.isJsxElement)
    .flatMap((element) => {
      const value = attribute(element, "className");
      const expression = value && ts.isJsxExpression(value) ? value.expression : undefined;
      return expression && ts.isConditionalExpression(expression)
        && ts.isStringLiteral(expression.whenFalse)
        && expression.whenFalse.text.split(/\s+/).includes("ext-panel__surface")
        ? [{ element, expression }] : [];
    });
  expect(candidates).toHaveLength(1);
  return candidates[0];
}

describe("extensions settings surface guard", () => {
  it("keeps every non-Computer extension tab inside the shared opaque settings card", () => {
    const { element, expression } = extensionsSurface();
    expect(selectedTab(expression.condition)).toBe("computer");
    expect(ts.isStringLiteral(expression.whenFalse) && expression.whenFalse.text.split(/\s+/))
      .toEqual(expect.arrayContaining(["settings-card", "ext-panel__surface"]));
    const tabs = nodes(element, ts.isBinaryExpression).map(selectedTab);
    for (const tab of ["plugins", "skills", "mcp", "hooks", "agents", "rules", "commands"]) {
      expect(tabs).toContain(tab);
    }
  });

  it("keeps Computer Use on its own shared opaque card without nesting another card", () => {
    const { element, expression } = extensionsSurface();
    expect(ts.isStringLiteral(expression.whenTrue) && expression.whenTrue.text.split(/\s+/))
      .toEqual(["ext-panel__computer-surface"]);
    const computerBranches = nodes(element, ts.isConditionalExpression)
      .filter((branch) => selectedTab(branch.condition) === "computer"
        && nodes(branch.whenTrue, ts.isJsxSelfClosingElement)
          .some((child) => child.tagName.getText() === "ComputerUseSettings"));
    expect(computerBranches).toHaveLength(1);

    const cards = nodes(parse("../components/computer-use/ComputerUseSettings.tsx"), ts.isJsxElement)
      .filter((child) => {
        const value = attribute(child, "className");
        return value && ts.isStringLiteral(value) && value.text.split(/\s+/).includes("settings-card");
      });
    expect(cards).toHaveLength(1);
    const classes = attribute(cards[0], "className");
    expect(classes && ts.isStringLiteral(classes) && classes.text.split(/\s+/))
      .toEqual(expect.arrayContaining(["settings-card", "cu-settings"]));
    const anchor = attribute(cards[0], "id");
    expect(anchor && ts.isStringLiteral(anchor) && anchor.text).toBe("settings-anchor-ext-computer");
  });

  it("keeps local-path plugin install on the plugins Discover + control", () => {
    const component = read("../components/ExtensionsPanel.tsx");
    const modals = read("../components/ExtensionsPanelPluginsModals.tsx");
    // Anchor stays on the panel's Discover surface; the card + wiring moved
    // with the plugins modal farm (WP: giant-component decomposition).
    expect(component).toContain("settings-anchor-ext-plugins-install");
    expect(component).toContain("openPathInstall");
    expect(component).toContain("pathInstallOpen");
    expect(component).toContain("pickDirectory");
    expect(modals).toContain("PluginPathInstallCard");
    expect(modals).toContain("openPathInstall");
  });

  it("reuses the shared card material and keeps its modifier layout-only", () => {
    const extensionCss = read("../styles/extensions-ref.part1.css");
    const computerCss = read("../styles/computer-use.css");
    const settingsCss = read("../styles/settings.part1.css");

    expect(settingsCss).toMatch(
      /\.settings-card\s*\{[^}]*background:\s*var\(--bg-card\);/s,
    );
    expect(extensionCss).toMatch(
      /\.ext-panel__surface\s*\{[^}]*padding:\s*16px;[^}]*min-width:\s*0;/s,
    );
    expect(extensionCss).not.toMatch(
      /\.ext-panel__(?:surface|computer-surface)\s*\{[^}]*(?:background(?:-color)?|opacity):/s,
    );
    expect(computerCss).not.toMatch(
      /\.cu-settings\s*\{[^}]*(?:background(?:-color)?|opacity):/s,
    );
  });
});
