import { vi } from "vitest";
import type { ScanState, PrepareBatchStatus } from "@/lib/api/sessionMigration";
import type { SourceComparisonRequest } from "@/lib/api/sessionMigrationComparison";
import { groupClient, groupMetadata, groupScope } from "./sessionImportGroupFixture";
import { previewComparisonMember } from "./sessionImportComparisonPreviewFixture";
import { fixedTransferPage, fixedTransferRequest } from "./sessionImportFixedPreviewFixture";
import { comparisonPage } from "./sessionImportComparisonFixture";

const ipc = vi.hoisted(() => ({ invoke: vi.fn(), events: vi.fn(), membership: vi.fn(), choose: vi.fn() }));
vi.mock("@/lib/api/host", async (original) => ({ ...await original<typeof import("@/lib/api/host")>(), invoke: ipc.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock("@/lib/nativeWebviewCover", () => ({ acquireNativeWebviewCover: () => () => {}, rectOverlapsNativeWebviewHost: () => false }));

export function adoptionUiFixture() {
  for (const mock of Object.values(ipc)) mock.mockReset();
  const host = groupClient();
  const scan: ScanState = { scanId: groupScope.scanId, epoch: 1, state: "completed", stateVersion: 1,
    candidates: [], discoveredCount: 3, complete: true, issueCount: 0, extraCount: 0,
    selection: { version: 1, selectionId: "candidate-selector", scanId: groupScope.scanId, epoch: 1,
      mode: "explicit", selectedCount: 0, stateVersion: 1 } };
  const batch: PrepareBatchStatus = { version: 1, batchId: "synthetic-ui-batch", scanId: groupScope.scanId,
    epoch: 1, selectionId: "synthetic-selector", selectionStateVersion: 3, state: "completed", stateVersion: 1,
    total: 2, prepared: 2, failed: 0, skipped: 0, active: 0, cancelRequested: false,
    createdAt: "2026-10-13T00:00:00Z", updatedAt: "2026-10-13T00:00:00Z" };
  ipc.events.mockResolvedValue(fixedTransferPage());
  ipc.membership.mockImplementation(host.client.membership);
  ipc.choose.mockImplementation(async (owner, groupId, candidateId) => {
    const prior = host.choices.get(groupId)!;
    if (prior.choiceVersion && prior.candidate.candidateId === candidateId) return {
      selection: await host.client.get(groupScope.scanId, 1), groupId, previousCandidateId: candidateId,
      candidateId, selected: prior.selected };
    const ack = await host.client.choose(owner, groupId, candidateId);
    host.choices.set(groupId, { ...host.choices.get(groupId)!, choiceVersion: ack.selection.stateVersion });
    return ack;
  });
  ipc.invoke.mockImplementation(async (command: string, args: Record<string, unknown> = {}) => {
    switch (command) {
      case "session_import_recovery": return { expired: false };
      case "session_import_entries": return { items: [], nextCursor: null, total: 0, providers: [] };
      case "session_import_list_roots": case "session_import_sources": case "projects_list": return [];
      case "session_import_scan_start": case "session_import_scan_get": case "session_import_scan_cancel": return scan;
      case "session_import_group_selection_get": return host.client.get(groupScope.scanId, 1);
      case "session_import_scan_groups": return host.client.groups(groupScope.scanId, 1);
      case "session_import_group_selection_membership": return ipc.membership(groupScope.scanId, 1,
        String(args.selectionId), args.groupIds);
      case "session_import_group_selection_set": return host.client.set(args, args.groupIds as string[], Boolean(args.selected));
      case "session_import_group_selection_freeze": return host.client.freeze(args);
      case "session_import_group_selection_clear": return host.client.clear(args);
      case "session_import_group_selection_choose": return ipc.choose(args, String(args.groupId), String(args.candidateId));
      case "session_import_scan_group_members": return { version: 1, groupId: groupMetadata("a").groupId,
        members: [previewComparisonMember("a"), previewComparisonMember("alternate")], nextCursor: null,
        memberCount: 2, complete: true, stateVersion: 1 };
      case "session_import_scan_group_member": return previewComparisonMember(String(args.candidateId).split(":").at(-1));
      case "session_import_preview": return fixedTransferRequest(String(args.candidateId).split(":").at(-1)).preview;
      case "session_import_preview_events": return ipc.events(args);
      case "session_import_source_comparison": {
        const request = args.request as SourceComparisonRequest;
        const page = comparisonPage(request);
        const side = (name: "left" | "right") => ({ ...fixedTransferRequest(request[name].candidateId.split(":").at(-1)).comparison, ...request[name] });
        return { ...page, left: side("left"), right: side("right"), rows: page.rows.slice(0, 2),
          totalPositions: 2, complete: true, nextCursor: null };
      }
      case "session_import_prepare_start": return batch;
      case "session_import_prepare_get": case "session_import_prepare_cancel": return batch;
      case "session_import_prepare_items": return { items: [], nextCursor: null, total: 2 };
      default: throw new Error(`Unexpected synthetic adoption IPC: ${command}`);
    }
  });
  return { ipc, host, scan, batch, calls: (command: string) => ipc.invoke.mock.calls.filter(([name]) => name === command) };
}
