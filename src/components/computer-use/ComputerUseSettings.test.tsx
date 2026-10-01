/** @vitest-environment jsdom */
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import * as api from "@/lib/api/computerUse";
import { createT, type Locale } from "@/i18n";
import { ComputerUseSettings } from "./ComputerUseSettings";

vi.mock("@/lib/api/computerUse", () => ({
  computerStatus: vi.fn(), computerSetEnabled: vi.fn(), computerRuntimeStatus: vi.fn(),
  computerRuntimeRepair: vi.fn(), computerRuntimeRollback: vi.fn(), computerExportBundle: vi.fn(),
  computerClearTraces: vi.fn(), computerClearStaging: vi.fn(), computerClearManagedProfiles: vi.fn(),
}));
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(api.computerStatus).mockResolvedValue({ featureEnabled: false } as api.ComputerStatus);
  vi.mocked(api.computerRuntimeStatus).mockResolvedValue({ issues: [], canRepair: true, canRollback: false });
});
afterEach(cleanup);

it.each<[Locale, string]>([
  ["en", "Clear activity history"], ["zh", "清除操作记录"], ["zh-TW", "清除操作記錄"],
  ["de", "Aktivitätsverlauf löschen"], ["es", "Borrar historial de actividad"],
  ["fil", "Burahin ang kasaysayan ng aktibidad"], ["fr", "Effacer l’historique d’activité"],
  ["id", "Hapus riwayat aktivitas"], ["it", "Cancella cronologia attività"],
  ["ja", "操作履歴を消去"], ["ko", "활동 기록 지우기"], ["pt-BR", "Limpar histórico de atividades"],
  ["ru", "Очистить историю действий"], ["ta", "செயல்பாட்டு வரலாற்றை அழி"],
  ["uk", "Очистити історію дій"],
])("uses clear localized activity-history copy in %s without deleting on cancel", async (locale, label) => {
  const tr = createT(locale);
  render(<ComputerUseSettings locale={locale} />);
  await waitFor(() => expect(screen.getByRole("switch")).toBeEnabled());
  fireEvent.click(screen.getByText(tr("cu.settings.maintenance")));
  fireEvent.click(screen.getByRole("button", { name: label }));
  const dialog = screen.getByRole("dialog", { name: label });
  expect(dialog).toHaveTextContent(tr("cu.settings.clearTracesHint"));
  expect(api.computerClearTraces).not.toHaveBeenCalled();
  fireEvent.click(within(dialog).getByRole("button", { name: tr("common.cancel") }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(api.computerClearTraces).not.toHaveBeenCalled();
  expect(api.computerClearStaging).not.toHaveBeenCalled();
  expect(api.computerClearManagedProfiles).not.toHaveBeenCalled();
});

it("does not allow toggling until the actual feature flag has loaded", async () => {
  let finish!: (value: api.ComputerStatus) => void;
  vi.mocked(api.computerStatus).mockImplementation(() => new Promise((resolve) => { finish = resolve; }));
  render(<ComputerUseSettings locale="en" />);
  expect(screen.getByRole("switch")).toBeDisabled();
  await act(async () => { finish({ featureEnabled: true } as api.ComputerStatus); });
  await waitFor(() => expect(screen.getByRole("switch")).toBeEnabled());
  expect(screen.getByRole("switch")).toHaveAttribute("aria-checked", "true");
});

it("keeps maintenance collapsed and reports the actual exported path", async () => {
  vi.mocked(api.computerExportBundle).mockResolvedValue("C:/fixture/support.zip");
  render(<ComputerUseSettings locale="en" />);
  await waitFor(() => expect(screen.getByRole("switch")).toBeEnabled());
  const maintenance = screen.getByText("Maintenance");
  expect(maintenance.closest("details")).not.toHaveAttribute("open");
  fireEvent.click(maintenance);
  fireEvent.click(screen.getByRole("button", { name: "Export Computer Use support bundle" }));
  expect(await screen.findByText("Support bundle saved to C:/fixture/support.zip")).toBeInTheDocument();
});

it("uses an operation-specific confirmation and never clears data before confirmation", async () => {
  render(<ComputerUseSettings locale="en" />);
  await waitFor(() => expect(screen.getByRole("switch")).toBeEnabled());
  fireEvent.click(screen.getByText("Maintenance"));
  fireEvent.click(screen.getByRole("button", { name: "Clear managed profiles and pairing" }));
  expect(screen.getByRole("dialog")).toHaveTextContent("You will need to sign in and pair again.");
  expect(api.computerClearManagedProfiles).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
  await waitFor(() => expect(api.computerClearManagedProfiles).toHaveBeenCalledTimes(1));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
});

it("shows failed loading without claiming a healthy runtime or enabling the switch", async () => {
  vi.mocked(api.computerStatus).mockRejectedValue(new Error("host unavailable"));
  render(<ComputerUseSettings locale="en" />);
  await screen.findByRole("alert");
  expect(screen.getByRole("switch")).toBeDisabled();
  expect(screen.queryByText("App-owned runtime is ready. System Node is not used.")).toBeNull();
  expect(screen.getByRole("button", { name: "Retry" })).toBeEnabled();
});

it("summarizes repeated runtime failures while retaining every component in collapsed diagnostics", async () => {
  const issues = ["js-runtime", "browser-worker", "chromium", "playwright-runtime"].map((component) => ({
    code: "missing_file", component, action: "repair",
  }));
  issues.push({ code: "hash_mismatch", component: "mcp-server", action: "repair" });
  vi.mocked(api.computerRuntimeStatus).mockResolvedValue({ issues, canRepair: true, canRollback: false });
  render(<ComputerUseSettings locale="en" />);
  await waitFor(() => expect(screen.getByRole("switch")).toBeEnabled());
  expect(screen.getAllByText("A required Computer Use file is missing. Repair the App runtime.")).toHaveLength(1);
  expect(screen.getByText("A Computer Use file failed its integrity check. Repair the App runtime.")).toBeInTheDocument();
  const diagnostics = screen.getByText("Diagnostics").closest("details");
  expect(diagnostics).not.toHaveAttribute("open");
  for (const issue of issues) expect(diagnostics).toHaveTextContent(issue.component);
  expect(api.computerRuntimeRepair).not.toHaveBeenCalled();
  expect(api.computerSetEnabled).not.toHaveBeenCalled();
});

it("keeps unknown runtime failures visible without showing a healthy state", async () => {
  vi.mocked(api.computerRuntimeStatus).mockResolvedValue({
    issues: [{ code: "future_failure", component: "fixture-component", action: "repair" }],
    canRepair: false, canRollback: false,
  });
  render(<ComputerUseSettings locale="en" />);
  await waitFor(() => expect(screen.getByRole("switch")).toBeEnabled());
  expect(screen.getByText("Could not run that action.")).toBeInTheDocument();
  const diagnostics = screen.getByText("Diagnostics").closest("details");
  expect(diagnostics).toHaveTextContent("future_failure");
  expect(diagnostics).toHaveTextContent("fixture-component");
  expect(screen.queryByText("App-owned runtime is ready. System Node is not used.")).toBeNull();
  expect(screen.getByRole("button", { name: "Repair runtime" })).toBeDisabled();
});
