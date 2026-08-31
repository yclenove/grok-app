import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const sourceRoot = resolve(__dirname, "..");
const provider = readFileSync(
  resolve(sourceRoot, "providers/ThemeProvider.tsx"),
  "utf8",
);
const context = readFileSync(
  resolve(sourceRoot, "providers/ThemeShellContext.ts"),
  "utf8",
);
const consumers = [
  "app/AppWorkbench.tsx",
  "hooks/useAppearanceEditorModel.ts",
  "providers/SkinShareProvider.tsx",
  "components/settings/AppearanceOpacityCard.tsx",
  "components/settings/AppearanceChromeCard.tsx",
  "components/settings/SkinPresetsCard.tsx",
] as const;

describe("ThemeProvider HMR boundary", () => {
  it("keeps the context identity outside the component module", () => {
    expect(provider).toContain('from "@/providers/ThemeShellContext"');
    expect(provider).not.toContain("createContext(");
    expect(provider).not.toContain("export function useThemeShell");
    expect(context).toContain("export const ThemeShellContext = createContext");
    expect(context).toContain("export function useThemeShell");
  });

  it.each(consumers)("imports the stable context from %s", (relativePath) => {
    const consumer = readFileSync(resolve(sourceRoot, relativePath), "utf8");
    expect(consumer).toContain(
      'import { useThemeShell } from "@/providers/ThemeShellContext";',
    );
    expect(consumer).not.toContain(
      'import { useThemeShell } from "@/providers/ThemeProvider";',
    );
  });
});
