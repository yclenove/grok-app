// @vitest-environment node
import { beforeAll, describe, expect, it } from "vitest";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
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
});
