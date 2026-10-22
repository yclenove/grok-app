import { expect, test } from "vitest";
import {
  choiceFromCustomFile,
  choiceFromCustomFolder,
  choiceFromDefaultRoot,
  choiceFromRegistered,
  choiceFromRelocate,
  choiceFromLocations,
  sameSourceChoice,
  scanSourcesForChoice,
} from "./sourceChoice";

const registered = {
  sourceId: "src-1",
  provider: "codex" as const,
  rootPath: "C:\\Users\\me\\.codex",
  profile: "default",
};

test("no choice scans every default root", () => {
  expect(scanSourcesForChoice(null)).toBeUndefined();
});

test("a default root is one path without a source id", () => {
  const choice = choiceFromDefaultRoot("codex", "C:\\Users\\me\\.codex");
  expect(scanSourcesForChoice(choice)).toEqual([
    { provider: "codex", path: "C:\\Users\\me\\.codex" },
  ]);
  expect(choice.kind).toBe("default");
});

test("custom folder and single file stay distinct kinds", () => {
  const folder = choiceFromCustomFolder("codex", "D:\\history");
  const file = choiceFromCustomFile("D:\\one.json");
  expect(folder.kind).toBe("custom-folder");
  expect(file.kind).toBe("custom-file");
  expect(scanSourcesForChoice(folder)?.[0]).toEqual({
    provider: "codex",
    path: "D:\\history",
  });
  expect(scanSourcesForChoice(file)?.[0].path).toBe("D:\\one.json");
  expect(sameSourceChoice(folder, file)).toBe(false);
});

test("registered sources carry sourceId and are not treated as a new folder", () => {
  const choice = choiceFromRegistered(registered);
  expect(choice.kind).toBe("registered");
  expect(scanSourcesForChoice(choice)).toEqual([
    { provider: "codex", path: registered.rootPath, sourceId: "src-1" },
  ]);
  expect(sameSourceChoice(choice, choiceFromCustomFolder("codex", registered.rootPath))).toBe(
    false,
  );
});

test("relocate keeps the registered id on the new path", () => {
  const moved = choiceFromRelocate(registered, "E:\\codex-copy");
  expect(moved.kind).toBe("relocate");
  expect(scanSourcesForChoice(moved)).toEqual([
    { provider: "codex", path: "E:\\codex-copy", sourceId: "src-1" },
  ]);
  expect(sameSourceChoice(moved, choiceFromCustomFolder("codex", "E:\\codex-copy"))).toBe(false);
});

test("selected installation roots use the existing locationId/host wire contract without relocation", () => {
  const source = { ...registered, provider: "cursor" as const };
  const primary = { version: 1 as const, locationId: `primary:${source.sourceId}`, sourceId: source.sourceId,
    provider: source.provider, host: "local", rootPath: source.rootPath, kind: "primary" as const };
  const associated = { ...primary, locationId: "00000000-0000-0000-0000-000000000001",
    rootPath: "D:/synthetic/associated", kind: "associated" as const };
  const choice = choiceFromLocations(source, [primary, associated]);
  expect(scanSourcesForChoice(choice)).toEqual([primary, associated].map((location) => ({
    provider: "cursor", sourceId: source.sourceId, path: location.rootPath, locationId: location.locationId, host: "local",
  })));
  expect(choice.locations?.[0]).not.toBe(primary);
  expect(sameSourceChoice(choice, choiceFromRegistered(source))).toBe(false);
  expect(() => choiceFromLocations(source, [{ ...associated, sourceId: "foreign" }])).toThrow("SOURCE_CHANGED");
});

test.each(["codex", "claude-code", "cursor", "generic-file"] as const)(
  "custom folder preserves the selected %s provider",
  (provider) => {
    expect(choiceFromCustomFolder(provider, "D:\\history")).toEqual({
      kind: "custom-folder",
      provider,
      path: "D:\\history",
    });
  },
);
