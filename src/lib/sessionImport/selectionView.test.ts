import { expect, test, vi } from "vitest";
import { groupCandidate, groupChoice, groupDeferred, groupDescriptor, groupMetadata, groupScope } from "@/test/sessionImportGroupFixture";
import type { GroupSelectionMembership } from "@/lib/api/sessionMigrationGroups";
import { defaultSelection } from "./selection";
import { selectedOverrides, type ImportSelectionView, type OverrideMembershipClient, type PreviewGroupBinding } from "./selectionView";

test.each(["fixed-groups", "independent-candidates"] as const)("a closed %s override owner does not request the next membership page", async (domain) => {
  const slots = Array.from({ length: 51 }, (_, index) => ({ candidateId: groupCandidate(String(index)).candidateId,
    previewId: `preview-${index}`, leafId: "leaf", selection: defaultSelection() }));
  const bindings = new Map<string, PreviewGroupBinding>(slots.map((slot, index) => [slot.candidateId,
    { scope: groupScope, group: groupMetadata(String(index)) }]));
  const pending = groupDeferred<GroupSelectionMembership>();
  const candidatePending = groupDeferred<{ selectionId: string; stateVersion: number; selectedIds: string[] }>();
  const client: OverrideMembershipClient = { groups: vi.fn().mockReturnValue(pending.promise),
    candidates: vi.fn().mockReturnValue(candidatePending.promise) };
  const view: ImportSelectionView = domain === "fixed-groups"
    ? { domain, scope: groupScope, descriptor: groupDescriptor(), ready: true }
    : { domain, scope: groupScope, descriptor: { version: 1, scanId: groupScope.scanId, epoch: 1,
      selectionId: "synthetic-selector", mode: "explicit", selectedCount: 0, stateVersion: 1 }, ready: true };
  let current = true;
  const reading = selectedOverrides(view, slots, bindings, client, () => current);
  const observed = expect(reading).rejects.toThrow("PLAN_STALE");
  current = false;
  pending.resolve({ selectionId: "synthetic-selector", stateVersion: 1,
    choices: slots.slice(0, 50).map((_slot, index) => ({ ...groupChoice(String(index)), groupSeq: index + 1 })) });
  candidatePending.resolve({ selectionId: "synthetic-selector", stateVersion: 1, selectedIds: [] });
  await observed;
  expect(domain === "fixed-groups" ? client.groups : client.candidates).toHaveBeenCalledTimes(1);
});
