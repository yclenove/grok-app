import { expect, test } from "vitest";
import type { SourceCandidate } from "@/lib/api/sessionMigration";
import { extrasNotOnPage } from "./scanList";

const row = (id: string): SourceCandidate => ({
  candidateId: id,
  provider: "codex",
  nativeSessionId: id,
  title: id,
  locator: id,
  sourceRevision: id,
  completeness: "metadata-preview",
});

test("extras that already appear on the current page are not listed twice", () => {
  const page = [row("a1"), row("a2"), row("e1")];
  const extras = [row("e1"), row("e2")];
  expect(extrasNotOnPage(page, extras).map((c) => c.candidateId)).toEqual(["e2"]);
});

test("an empty extra page stays empty", () => {
  expect(extrasNotOnPage([row("a1")], [])).toEqual([]);
});
