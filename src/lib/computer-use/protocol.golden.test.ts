import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import {
  COMPUTER_USE_ERROR_CODES,
  COMPUTER_USE_HOST_ONLY_TOOLS,
  COMPUTER_USE_MODEL_TOOLS,
  COMPUTER_USE_PROTOCOL_VERSION,
  validateActionRequest,
  validateNavigateUrl,
} from "./protocol";

const golden = join(process.cwd(), "tools/computer-use-protocol/golden");

function readJson(path: string) {
  return JSON.parse(readFileSync(path, "utf8"));
}

describe("computer-use golden catalog", () => {
  const catalog = readJson(join(golden, "catalog.json"));

  it("matches TypeScript constants", () => {
    expect(catalog.protocolVersion).toBe(COMPUTER_USE_PROTOCOL_VERSION);
    expect(catalog.modelTools).toEqual([...COMPUTER_USE_MODEL_TOOLS]);
    expect(catalog.hostOnlyTools).toEqual([...COMPUTER_USE_HOST_ONLY_TOOLS]);
    expect(catalog.errorCodes).toEqual([...COMPUTER_USE_ERROR_CODES]);
    for (const name of catalog.hostOnlyTools) {
      expect(COMPUTER_USE_MODEL_TOOLS).not.toContain(name);
    }
  });

  it("accepts every valid golden action", () => {
    const dir = join(golden, "actions/valid");
    const files = readdirSync(dir).filter((f) => f.endsWith(".json"));
    expect(files.length).toBeGreaterThanOrEqual(7);
    for (const file of files) {
      const body = readJson(join(dir, file));
      expect(validateActionRequest(body), file).toBeNull();
    }
  });

  it("rejects every invalid golden action", () => {
    const dir = join(golden, "actions/invalid");
    const files = readdirSync(dir).filter((f) => f.endsWith(".json"));
    expect(files.length).toBeGreaterThanOrEqual(9);
    for (const file of files) {
      const fixture = readJson(join(dir, file));
      const err = validateActionRequest(fixture.body);
      expect(err, file).toBeTruthy();
      expect(String(err).toLowerCase(), file).toContain(
        String(fixture.errorContains).toLowerCase(),
      );
    }
  });

  it("rejects NaN and Infinity coordinates", () => {
    const base = readJson(join(golden, "actions/valid/click-coord.json"));
    expect(
      validateActionRequest({ ...base, target: { x: Number.NaN, y: 1 } }),
    ).toMatch(/finite|nonnegative/);
    expect(
      validateActionRequest({ ...base, target: { x: Number.POSITIVE_INFINITY, y: 1 } }),
    ).toMatch(/finite|nonnegative/);
  });

  it("rejects illegal navigate URLs from golden", () => {
    const rows = readJson(join(golden, "urls/invalid.json"));
    for (const row of rows) {
      const err = validateNavigateUrl(row.url);
      expect(err, row.url).toBeTruthy();
      expect(String(err).toLowerCase(), row.url).toContain(
        String(row.errorContains).toLowerCase(),
      );
    }
    expect(validateNavigateUrl("https://example.com/a")).toBeNull();
  });
});
