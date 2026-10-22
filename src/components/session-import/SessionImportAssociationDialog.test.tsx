/** @vitest-environment jsdom */
import "@testing-library/jest-dom/vitest";
import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import { createT } from "@/i18n";
import { useSessionImportAssociations, type AssociationClient } from "@/hooks/useSessionImportAssociations";
import { SessionImportAssociationDialog } from "./SessionImportAssociationDialog";
import type { PrepareBatchStatus } from "@/lib/api/sessionMigration";

const mocks = vi.hoisted(() => ({ source: vi.fn(), reload: vi.fn() }));
vi.mock("@/lib/nativeWebviewCover", () => ({
  acquireNativeWebviewCover: () => () => {}, rectOverlapsNativeWebviewHost: () => false,
}));
vi.mock("@/lib/api/sessionMigration", () => ({
  sessionImportPreviewEvents: mocks.source,
}));
vi.mock("@/hooks/useImportedReader", () => ({ useImportedReader: () => ({
  scope: { revision: "old-rev", leaf: "leaf" }, busy: false, error: null,
  events: [{ eventId: "same-id", kind: "message", role: "user", text: "OLD SYNTHETIC HISTORY" }],
  previousCursor: null, nextCursor: null, reload: mocks.reload,
}) }));
afterEach(() => { cleanup(); vi.clearAllMocks(); });
const tr = createT("en");
const batch: PrepareBatchStatus = { version: 1, batchId: "batch", scanId: "scan", epoch: 1, selectionId: "selection",
  selectionStateVersion: 1, state: "completed", stateVersion: 4, total: 1, prepared: 1, failed: 0, skipped: 0,
  active: 0, cancelRequested: false, createdAt: "2026-10-15", updatedAt: "2026-10-15" };
const target = { importId: "imp:old", installationId: "registered", provider: "cursor", nativeSessionId: "old",
  title: "Synthetic old", currentRevision: "old-rev", hidden: false, createdAt: "2026-10-15" };
const client: AssociationClient = {
  entries: async () => ({ items: [target], total: 1, nextCursor: null }),
  entry: async () => target,
  inspect: async (request) => ({ batchId: request.batchId, stateVersion: request.expectedStateVersion,
    candidateId: request.candidateId, leafId: "leaf", review: { version: 1, importId: request.importId,
      expectedRevision: request.expectedRevision, previewId: request.previewId, sourceId: "registered",
      oldNativeSessionId: "old", currentNativeSessionId: "new", oldNormalizerVersion: "1.0.0",
      currentNormalizerVersion: "1.0.6", selectionHash: "selection", reviewDigest: "a".repeat(64) } }),
};
function Harness() {
  const ui = useSessionImportAssociations(batch, true, () => false, client);
  return <>
    <button onClick={() => ui.open({ candidateId: "candidate", title: "Synthetic source", provider: "cursor",
      sourceRevision: "source-rev", state: "prepared", attempt: 1, previewId: "preview" })}>Open synthetic review</button>
    <output>{ui.confirmed.length}</output>
    <SessionImportAssociationDialog tr={tr} ui={ui} />
  </>;
}

test("link confirmation waits for both read-only panes; duplicate event IDs stay scoped", async () => {
  let resolve!: (page: { events: unknown[]; nextCursor: null }) => void;
  mocks.source.mockReturnValueOnce(new Promise((done) => { resolve = done; }));
  const user = userEvent.setup(); render(<Harness />);
  await user.click(screen.getByText("Open synthetic review"));
  const confirm = screen.getByRole("button", { name: tr("sessionImport.historyLink.confirm") });
  expect(confirm).toBeDisabled();
  await waitFor(() => expect(screen.getByRole("button", { name: tr("sessionImport.historyLink.target") })).toBeEnabled());
  await user.click(screen.getByRole("button", { name: tr("sessionImport.historyLink.target") }));
  await user.click(within(screen.getByRole("option", { name: /Synthetic old/ })).getByRole("button"));
  await screen.findByText("OLD SYNTHETIC HISTORY");
  expect(confirm).toBeDisabled();
  await act(async () => resolve({ events: [{ eventId: "same-id", kind: "message", role: "user", text: "NEW SYNTHETIC HISTORY" }], nextCursor: null }));
  await screen.findByText("NEW SYNTHETIC HISTORY");
  await waitFor(() => expect(confirm).toBeEnabled());
  expect(mocks.source).toHaveBeenCalledWith("preview", null, 20, "leaf");
  expect(document.getElementById("association-target-same-id")).not.toBeNull();
  expect(document.getElementById("association-source-same-id")).not.toBeNull();
  await user.click(confirm);
  expect(screen.queryByRole("dialog")).toBeNull(); expect(screen.getByRole("status")).toHaveTextContent("1");
});

test("a failed source read cannot enable confirmation; cancel leaves no link", async () => {
  mocks.source.mockRejectedValueOnce(new Error("SNAPSHOT_UNAVAILABLE: synthetic read failure"));
  const user = userEvent.setup(); render(<Harness />);
  await user.click(screen.getByText("Open synthetic review"));
  await waitFor(() => expect(screen.getByRole("button", { name: tr("sessionImport.historyLink.target") })).toBeEnabled());
  await user.click(screen.getByRole("button", { name: tr("sessionImport.historyLink.target") }));
  await user.click(within(screen.getByRole("option", { name: /Synthetic old/ })).getByRole("button"));
  await screen.findByRole("alert");
  expect(screen.getByRole("button", { name: tr("sessionImport.historyLink.confirm") })).toBeDisabled();
  await user.click(screen.getByRole("button", { name: tr("common.cancel") }));
  expect(screen.queryByRole("dialog")).toBeNull(); expect(screen.getByRole("status")).toHaveTextContent("0");
});
