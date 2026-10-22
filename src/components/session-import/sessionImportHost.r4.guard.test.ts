/** Behavior coverage for R4 lives in SessionImportHost.interact.test.tsx
 *  and useSessionImport.test.tsx (hashchange / restore). This file no longer
 *  treats source-string contains() as product proof.
 */
import { describe, expect, it } from "vitest";
import { importedHistoryHash, parseImportedDeepLink } from "@/lib/sessionImport/open";

describe("imported history hash is the copyable route", () => {
  it("round-trips revision and leaf", () => {
    const detail = {
      importId: "imp:00000000-0000-4000-8000-000000000001",
      revision: "rev-a",
      leaf: "a3",
    };
    expect(parseImportedDeepLink(importedHistoryHash(detail))).toEqual(detail);
  });
});
