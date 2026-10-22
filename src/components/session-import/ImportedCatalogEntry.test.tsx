/** @vitest-environment jsdom */
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { createT } from "@/i18n";
import { ImportedCatalogEntry } from "./ImportedCatalogEntry";

const mocks = vi.hoisted(() => ({ entries: vi.fn(), groups: vi.fn(), listen: vi.fn() }));
vi.mock("@/lib/api/sessionMigration", () => ({
  sessionImportEntries: mocks.entries,
  sessionImportExportMarkdown: vi.fn(),
}));
vi.mock("@/lib/api/sessionWorkspaceAssociation", () => ({ importedWorkspaceGroups: mocks.groups }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("@/lib/nativeWebviewCover", () => ({ acquireNativeWebviewCover: () => () => {} }));

const row = {
  importId: "imp:00000000-0000-4000-8000-000000000001",
  title: "Imported example",
  alias: null,
  hidden: false,
  provider: "codex",
  nativeSessionId: "n1",
  createdAt: "2026-09-20T00:00:00Z",
};

beforeEach(() => {
  sessionStorage.clear();
  mocks.listen.mockResolvedValue(() => {});
  mocks.groups.mockResolvedValue({ items: [], nextCursor: null, selected: null });
  mocks.entries.mockResolvedValue({ items: [row], nextCursor: null, total: 4, providers: ["codex"] });
});
afterEach(() => { cleanup(); vi.resetAllMocks(); });

test("settings shows a compact count until the catalog is opened", async () => {
  const user = userEvent.setup();
  render(<ImportedCatalogEntry tr={createT("en")} />);
  expect(await screen.findByText("4 imported records")).toBeTruthy();
  expect(screen.queryByRole("button", { name: /Imported example/ })).toBeNull();
  await user.click(screen.getByRole("button", { name: "Open catalog" }));
  expect(await screen.findByRole("button", { name: /Imported example/ })).toBeTruthy();
  expect(screen.getByRole("searchbox")).toBeTruthy();
});
