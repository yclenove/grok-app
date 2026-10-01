// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createT } from "@/i18n";
import { emptySideWorkbenchState } from "@/lib/sideWorkbench";
import { emptySessionPlan } from "@/lib/planSession";
import { WorkbenchResourcesAside, type WorkbenchResourcesAsideProps } from "./WorkbenchResourcesAside";

vi.mock("@/components/side-workbench/SideWorkbench", () => ({ SideWorkbench: () => null }));
vi.mock("@/hooks/usePaneUnreadDot", () => ({ usePaneUnreadDot: () => false }));
vi.mock("@/components/PaneToggleButton", () => ({ PaneToggleButton: () => null }));
afterEach(cleanup);

function props(overrides: Partial<WorkbenchResourcesAsideProps> = {}): WorkbenchResourcesAsideProps {
  return {
    tr: createT("en"), locale: "en",
    layout: { asideCollapsed: false, asideWidth: 458 },
    phoneLayout: false, sidePaneCoversMain: false, asideOverlay: false,
    resizingAside: false, asideOpenW: 458, asidePaint: 458,
    asideResize: { value: 458, min: 458, max: 574, begin: vi.fn(), change: vi.fn() },
    effectiveProjectPath: null, projectName: "",
    sideIsGitProject: false, sideWorkbench: emptySideWorkbenchState(),
    setSideWorkbench: vi.fn(), sideDockComposer: false,
    onToggleSideDockComposer: vi.fn(), sessionChanges: [], sessionId: null,
    plan: emptySessionPlan(),
    planFocusKey: null, composerMode: "chat", planEnabled: false,
    planUserClosed: false, planHistoryNonEmpty: false,
    onApprovePlan: vi.fn(), onRequestPlanChanges: vi.fn(),
    onDismissPlan: vi.fn(), onOpenPlanHistory: vi.fn(),
    resourceOpenTarget: null, onOpenRequestConsumed: vi.fn(),
    closeActiveSideRequest: null, onCloseActiveRequestConsumed: vi.fn(),
    onToggleSide: vi.fn(), onExpandedChange: vi.fn(), skillInfos: [],
    skillsLoading: false, skillsLoadError: null, onSelectSkill: vi.fn(),
    ...overrides,
  };
}

describe("workbench side-pane resize accessibility", () => {
  it("exposes a keyboard-focusable separator for the Computer pane", () => {
    render(<WorkbenchResourcesAside {...props()} />);
    const handle = screen.getByRole("separator");
    expect(handle.getAttribute("tabindex")).toBe("0");
    expect(handle.getAttribute("aria-controls")).toBe("workbench-aside");
    expect(handle.getAttribute("aria-valuemin")).toBe("458");
    expect(handle.getAttribute("aria-valuemax")).toBe("574");
    expect(handle.getAttribute("aria-valuenow")).toBe("458");
  });

  it("routes keyboard changes to the shared layout controller", () => {
    const options = props();
    render(<WorkbenchResourcesAside {...options} />);
    const handle = screen.getByRole("separator");
    fireEvent.keyDown(handle, { key: "ArrowLeft" });
    fireEvent.keyDown(handle, { key: "ArrowRight", shiftKey: true });
    fireEvent.keyDown(handle, { key: "Home" });
    fireEvent.keyDown(handle, { key: "End" });
    expect(vi.mocked(options.asideResize.change).mock.calls).toEqual([
      [{ kind: "delta", pixels: 10 }], [{ kind: "delta", pixels: -50 }],
      [{ kind: "minimum" }], [{ kind: "maximum" }],
    ]);
    expect(options.asideResize.begin).not.toHaveBeenCalled();
  });

  it.each([
    { phoneLayout: true }, { sidePaneCoversMain: true },
    { asideOverlay: true }, { layout: { asideCollapsed: true, asideWidth: 458 } },
  ])("does not leave an invisible tab stop in %j", (overrides) => {
    render(<WorkbenchResourcesAside {...props(overrides)} />);
    expect(screen.queryByRole("separator", { hidden: true })).toBeNull();
  });
});
