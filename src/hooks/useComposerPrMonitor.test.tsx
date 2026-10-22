/** @vitest-environment jsdom */
import "@/test/jsdomStubs";
import { renderHook } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { createT } from "@/i18n";
import type { Project } from "@/lib/app/sidebarModels";
import type { UsePrMonitorResult } from "@/hooks/usePrMonitor";

const usePrMonitor = vi.hoisted(() => vi.fn());
const desktop = vi.hoisted(() => vi.fn(() => true));
vi.mock("@/hooks/usePrMonitor", () => ({ usePrMonitor }));
vi.mock("@/lib/api", () => ({ isDesktopHost: desktop }));
import { useComposerPrMonitor } from "./useComposerPrMonitor";

const project: Project = { id: "p", name: "Synthetic", path: "/synthetic", trusted: true, pathOk: true };

beforeEach(() => {
  desktop.mockReturnValue(true);
  usePrMonitor.mockReturnValue({ prs: [], watches: [], busy: [] } satisfies Partial<UsePrMonitorResult>);
  window.location.hash = "";
});

it("binds the repository and chat without requiring a matching worktree branch", () => {
  const showToast = vi.fn();
  const { result, rerender } = renderHook(({ sessionId }) => useComposerPrMonitor({
    activeProject: project, session: { sessionId }, locale: "zh", showToast, tr: createT("zh"),
  }), { initialProps: { sessionId: "chat-a" } });
  expect(usePrMonitor.mock.lastCall?.[0]).toMatchObject({ enabled: true, projectPath: "/synthetic", sessionId: "chat-a" });
  expect(usePrMonitor.mock.lastCall?.[0]).not.toHaveProperty("branch");
  expect(result.current.prRows).toEqual([]);
  usePrMonitor.mock.lastCall?.[0].notify("failure", "error");
  expect(showToast).toHaveBeenCalledWith("failure", 5000);
  rerender({ sessionId: "chat-b" });
  expect(usePrMonitor.mock.lastCall?.[0].sessionId).toBe("chat-b");
  result.current.openPrHubForNumber(7);
  expect(window.location.hash).toContain("7");
});

it("disables monitoring without a desktop project", () => {
  desktop.mockReturnValue(false);
  renderHook(() => useComposerPrMonitor({ activeProject: null, session: { sessionId: null }, locale: "en", showToast: vi.fn(), tr: createT("en") }));
  expect(usePrMonitor.mock.lastCall?.[0]).toMatchObject({ enabled: false, projectPath: null, sessionId: null });
});
