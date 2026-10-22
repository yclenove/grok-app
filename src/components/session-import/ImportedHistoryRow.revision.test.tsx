/** @vitest-environment jsdom */
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { createT } from "@/i18n";
import type { CatalogEntry } from "@/lib/api/sessionMigration";
import { ImportedHistoryRow } from "./ImportedHistoryRow";
const { open } = vi.hoisted(() => ({ open: vi.fn() }));
vi.mock("@/lib/sessionImport/open", () => ({ openImportedHistory: open }));
afterEach(() => { cleanup(); vi.clearAllMocks(); });
const entry: CatalogEntry = { importId: "imp:snapshot", installationId: "installation", provider: "codex", nativeSessionId: "native", title: "Snapshot", hidden: false, createdAt: "2026-09-27T00:00:00Z" };
test.each(["revision-a", null, undefined])("read and continue carry the displayed revision (%s)", (revision) => {
  render(<ImportedHistoryRow entry={{ ...entry, currentRevision: revision }} providerLabel="Codex" exporting={false} exportBusy={false} onExport={vi.fn()} tr={createT("en")} />);
  fireEvent.click(screen.getByRole("button", { name: /Snapshot/ }));
  expect(open).toHaveBeenLastCalledWith({ importId: entry.importId, ...(revision ? { revision } : {}) });
  fireEvent.click(screen.getByRole("button", { name: "Continue in Grok" }));
  expect(open).toHaveBeenLastCalledWith({ importId: entry.importId, ...(revision ? { revision } : {}), continue: true });
});
