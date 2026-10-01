/** @vitest-environment jsdom */
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import * as computer from "@/lib/api/computerUse";
import * as api from "@/lib/api";
import { ExtensionsPanel } from "./ExtensionsPanel";

vi.mock("@/lib/api", () => ({
  isTauri: vi.fn(), skillsList: vi.fn(), inspectMcp: vi.fn(), pluginsList: vi.fn(),
}));
vi.mock("@/lib/pluginsListCache", () => ({
  loadPluginsListCached: (load: () => Promise<unknown>) => load(),
  invalidatePluginsListCache: vi.fn(), patchPluginsListEnabled: vi.fn(),
}));
vi.mock("@/lib/api/computerUse", () => ({
  computerStatus: vi.fn(), computerSetEnabled: vi.fn(), computerRuntimeStatus: vi.fn(),
  computerRuntimeRepair: vi.fn(), computerRuntimeRollback: vi.fn(), computerExportBundle: vi.fn(),
  computerClearTraces: vi.fn(), computerClearStaging: vi.fn(), computerClearManagedProfiles: vi.fn(),
}));

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(api.isTauri).mockReturnValue(false);
  vi.mocked(api.skillsList).mockResolvedValue({ skills: [], skillRoots: [] });
  vi.mocked(api.inspectMcp).mockResolvedValue({ servers: [] });
  vi.mocked(api.pluginsList).mockResolvedValue({ plugins: [] });
  vi.mocked(computer.computerStatus).mockResolvedValue({ featureEnabled: false } as computer.ComputerStatus);
  vi.mocked(computer.computerRuntimeStatus).mockResolvedValue({ issues: [], canRepair: false, canRollback: false });
});

it("does not inspect unrelated CLI extensions when Computer Use opens", async () => {
  vi.mocked(api.isTauri).mockReturnValue(true);
  render(<ExtensionsPanel locale="en" activeTab="computer" cliFound={false} />);
  await waitFor(() => expect(screen.getByRole("switch")).toBeEnabled());
  expect(api.skillsList).not.toHaveBeenCalled();
  expect(api.inspectMcp).not.toHaveBeenCalled();
  expect(api.pluginsList).not.toHaveBeenCalled();
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  expect(computer.computerSetEnabled).not.toHaveBeenCalled();
});

it("scopes legacy inspection errors to their tabs and still reloads them on return", async () => {
  vi.mocked(api.isTauri).mockReturnValue(true);
  vi.mocked(api.inspectMcp).mockRejectedValue(new Error("legacy MCP inspection failed"));
  const { rerender } = render(<ExtensionsPanel locale="en" activeTab="mcp" />);
  await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("legacy MCP inspection failed"));
  rerender(<ExtensionsPanel locale="en" activeTab="computer" />);
  await waitFor(() => expect(screen.getByRole("switch")).toBeEnabled());
  expect(screen.queryByText(/legacy MCP inspection failed/)).not.toBeInTheDocument();
  expect(api.inspectMcp).toHaveBeenCalledTimes(1);
  rerender(<ExtensionsPanel locale="en" activeTab="mcp" />);
  await waitFor(() => expect(api.inspectMcp).toHaveBeenCalledTimes(2));
  expect(screen.getByRole("alert")).toHaveTextContent("legacy MCP inspection failed");
});

it("does not leak an in-flight legacy failure into Computer Use after a tab switch", async () => {
  vi.mocked(api.isTauri).mockReturnValue(true);
  let rejectInspection!: (reason: Error) => void;
  vi.mocked(api.inspectMcp).mockReturnValueOnce(new Promise((_, reject) => { rejectInspection = reject; }));
  const { rerender } = render(<ExtensionsPanel locale="en" activeTab="mcp" />);
  expect(api.inspectMcp).toHaveBeenCalledTimes(1);
  rerender(<ExtensionsPanel locale="en" activeTab="computer" />);
  await waitFor(() => expect(screen.getByRole("switch")).toBeEnabled());
  await act(async () => { rejectInspection(new Error("late unrelated MCP failure")); });
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  expect(api.inspectMcp).toHaveBeenCalledTimes(1);
  expect(screen.getByRole("switch")).toHaveAttribute("aria-checked", "false");
});
afterEach(cleanup);

it("renders one settings surface in the actual Computer Use extensions shell", async () => {
  const { container } = render(<ExtensionsPanel locale="en" activeTab="computer" />);
  await waitFor(() => expect(screen.getByRole("switch")).toBeEnabled());
  const card = container.querySelector("#settings-anchor-ext-computer");
  expect(card).toHaveClass("settings-card", "cu-settings");
  expect(card?.parentElement?.closest(".settings-card")).toBeNull();
  expect(container.querySelectorAll(".settings-card")).toHaveLength(1);
  expect(screen.getByRole("switch")).toHaveAttribute("aria-checked", "false");
  expect(computer.computerSetEnabled).not.toHaveBeenCalled();
});

it("keeps the existing MCP card when switching away from Computer Use", async () => {
  const { container, rerender } = render(<ExtensionsPanel locale="en" activeTab="computer" />);
  await waitFor(() => expect(screen.getByRole("switch")).toBeEnabled());
  rerender(<ExtensionsPanel locale="en" activeTab="mcp" />);
  expect(container.querySelector(".cu-settings")).toBeNull();
  expect(container.querySelector(".settings-card.ext-panel__surface")).toBeInTheDocument();
  rerender(<ExtensionsPanel locale="en" activeTab="computer" />);
  await waitFor(() => expect(screen.getByRole("switch")).toBeEnabled());
  expect(container.querySelectorAll(".settings-card")).toHaveLength(1);
});
