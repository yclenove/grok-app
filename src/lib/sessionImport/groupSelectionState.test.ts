import { expect, test } from "vitest";
import { groupChoice, groupDescriptor, groupMetadata, groupPage, groupScope } from "@/test/sessionImportGroupFixture";
import { validateGroupMembership, validateGroupPage } from "./groupSelectionState";

test.each(["foreign-candidate", "duplicate-candidate", "native-identity", "host", "identity", "provider-filter", "source-filter"] as const)(
  "rejects $0 metadata rather than exposing a foreign row", (kind) => {
    const page = groupPage();
    const filter = kind === "provider-filter" ? { provider: "claude-code" as const }
      : kind === "source-filter" ? { sourceId: "different-source" } : {};
    if (kind === "foreign-candidate") page.groups[0].firstMember.candidateId = "candidate:other-scan:a";
    if (kind === "duplicate-candidate") page.groups[1].firstMember = page.groups[0].firstMember;
    if (kind === "native-identity") page.groups[0].firstMember.nativeSessionId = "other-session";
    if (kind === "host") page.groups[0].host = "";
    if (kind === "identity") page.groups[0].nativeSessionId = "";
    expect(() => validateGroupPage(groupScope, page, filter)).toThrow("PLAN_STALE");
  },
);

test("typed native identity is a membership boundary, not proof of body equality", () => {
  const choice = groupChoice("a");
  choice.candidate.nativeSessionId = "another-native-session";
  expect(() => validateGroupMembership(groupScope, groupDescriptor(), [groupMetadata("a")], {
    selectionId: "synthetic-selector", stateVersion: 1, choices: [choice],
  })).toThrow("PLAN_STALE");
});

test("artifact metadata does not establish a shared native identity or body equality", () => {
  const group = { ...groupMetadata("a"), proof: "artifact" as const };
  const choice = groupChoice("a");
  choice.candidate.nativeSessionId = "artifact-native-metadata";
  expect(() => validateGroupMembership(groupScope, groupDescriptor(), [group], {
    selectionId: "synthetic-selector", stateVersion: 1, choices: [choice],
  })).not.toThrow();
});

test("duplicate chosen candidates cannot belong to two group rows", () => {
  const choices = [groupChoice("a"), groupChoice("b")];
  choices[1].candidate = choices[0].candidate;
  expect(() => validateGroupMembership(groupScope, groupDescriptor(), groupPage().groups, {
    selectionId: "synthetic-selector", stateVersion: 1, choices,
  })).toThrow("PLAN_STALE");
});

test("an uncaptured zero-version choice must still refer to the first member", () => {
  const choice = groupChoice("a", false, "alternate");
  expect(() => validateGroupMembership(groupScope, groupDescriptor(), [groupMetadata("a")], {
    selectionId: "synthetic-selector", stateVersion: 1, choices: [choice],
  })).toThrow("PLAN_STALE");
});
