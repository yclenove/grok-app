// @vitest-environment node
import { beforeAll, describe, expect, it } from "vitest";
import { execFileSync } from "node:child_process";
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";

const root = new URL("../", import.meta.url);
const read = (path: string) => readFileSync(new URL(path, root), "utf8");

describe("store material and packaged permission contract", () => {
  beforeAll(() => {
    execFileSync(process.execPath, [fileURLToPath(new URL("build.mjs", root))], { stdio: "pipe" });
  });

  it("built_manifest_permissions_match_store_copy_and_source", () => {
    const built = JSON.parse(read("dist/manifest.json"));
    const source = JSON.parse(read("manifest.json"));
    const copy = read("store/permissions.md");
    const documented = [...copy.matchAll(/^\| `([^`]+)` \| (.+) \|$/gm)].map(match => match[1]);
    expect(built).toEqual(source);
    expect(documented.length).toBe(new Set(documented).size);
    expect([...built.permissions].sort()).toEqual(documented.sort());
    for (const field of ["host_permissions", "optional_host_permissions", "optional_permissions", "content_scripts"]) {
      expect(built[field] ?? []).toEqual([]);
    }
  });

  it("test_fixtures_never_enter_extension_package", () => {
    expect(readdirSync(new URL("dist/", root)).sort()).toEqual([
      "_locales", "background.js", "content.js", "manifest.json",
    ]);
    const files = readdirSync(new URL("dist/", root), { recursive: true, encoding: "utf8" });
    expect(files.some(path => /(?:tests|fixtures|\.test\.)/.test(path))).toBe(false);
    for (const entry of ["background", "content"]) {
      const bundle = read(`dist/${entry}.js`);
      expect(bundle).not.toContain("fixture-relay");
      expect(bundle).not.toContain("page_relays_marker_unique_wrong_window");
      expect(bundle).not.toContain("privateActionShim");
    }
  });
});
