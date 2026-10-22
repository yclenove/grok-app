import { beforeEach, expect, test, vi } from "vitest";
import { sessionImportSourceAssociate, sessionImportSourceLocations, sessionImportSourceRegister } from "./sessionMigration";
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("./host", () => ({ invoke }));
beforeEach(() => vi.resetAllMocks());

test("location lookup forwards the full installation/provider/host and page claim", async () => {
  const page = { locations: [], nextCursor: null, totalCount: 0 };
  invoke.mockResolvedValue(page);
  expect(await sessionImportSourceLocations("source-id", "cursor", {
    host: "local", cursor: "opaque-cursor", limit: 50,
  })).toBe(page);
  expect(invoke).toHaveBeenCalledExactlyOnceWith("session_import_source_locations", {
    sourceId: "source-id", provider: "cursor", host: "local", cursor: "opaque-cursor", limit: 50,
  });
});

test("failed location lookup propagates without fallback or registration", async () => {
  invoke.mockRejectedValue(new Error("SOURCE_NOT_FOUND: missing location"));
  await expect(sessionImportSourceLocations("source-id", "cursor")).rejects.toThrow("SOURCE_NOT_FOUND");
  expect(invoke).toHaveBeenCalledTimes(1);
});

test("relocation does not strip location or host claims before Host validation", async () => {
  const source = {
    provider: "cursor", path: "synthetic-path", sourceId: "source-id", locationId: "location-id", host: "local",
  };
  invoke.mockRejectedValue(new Error("MAPPING_CONFLICT: not a relocation"));
  await expect(sessionImportSourceRegister(source)).rejects.toThrow("MAPPING_CONFLICT");
  expect(invoke).toHaveBeenCalledExactlyOnceWith("session_import_source_register", { source });
});

test("explicit association preserves owner, host and expected primary root", async () => {
  const request = { sourceId: "source-id", provider: "cursor" as const, host: "local", path: "associated-root", expectedPrimaryRoot: "original-root" };
  const location = { locationId: "location-id", kind: "associated" };
  invoke.mockResolvedValue(location);
  expect(await sessionImportSourceAssociate(request)).toBe(location);
  expect(invoke).toHaveBeenCalledExactlyOnceWith("session_import_source_associate", { request });
});

test("association conflicts propagate without an implicit relocation fallback", async () => {
  invoke.mockRejectedValue(new Error("MAPPING_CONFLICT: another installation owns this root"));
  await expect(sessionImportSourceAssociate({ sourceId: "source-id", provider: "cursor", host: "local", path: "associated-root", expectedPrimaryRoot: "original-root" })).rejects.toThrow("MAPPING_CONFLICT");
  expect(invoke).toHaveBeenCalledTimes(1);
});
