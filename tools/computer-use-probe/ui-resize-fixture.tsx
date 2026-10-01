// Probe-only shell. Real resize hook/handle/panel; not the full App workbench.
import { createT, type Locale } from "../../src/i18n";
import { AsideResizeHandle } from "../../src/components/AsideResizeHandle";
import { ComputerPanel } from "../../src/components/computer-use/ComputerPanel";
import { useWorkbenchLayout } from "../../src/hooks/useWorkbenchLayout";
import { paneSplitSizeStyle } from "../../src/lib/paneSplitMotion";

export function ComputerResizeFixture({ locale }: { locale: Locale }) {
  const controller = useWorkbenchLayout();
  const { layout, asideResize, resizingAside } = controller;
  return (
    <div className="workbench">
      <nav className="sidebar" style={paneSplitSizeStyle(layout.sidebarWidth, "x")}>
        <span>Probe-only navigation</span>
      </nav>
      <main style={{ flex: 1, minWidth: 360, padding: 20 }}>
        {layout.asideCollapsed ? (
          <button type="button" data-testid="open-pane" onClick={controller.openAsidePane}>
            Open pane
          </button>
        ) : (
          <button type="button" data-testid="close-pane" onClick={controller.closeAsidePane}>
            Close pane
          </button>
        )}
      </main>
      {!layout.asideCollapsed && (
        <aside id="workbench-aside" className={`aside${resizingAside ? " is-resizing" : ""}`}
          style={paneSplitSizeStyle(layout.asideWidth, "x")}>
          <AsideResizeHandle control={asideResize} busy={resizingAside}
            label={createT(locale)("resources.resizeFilesPane")} />
          <button type="button" data-testid="after-separator">Probe focus destination</button>
          <ComputerPanel locale={locale} sessionId="fixture-chat" runId={null} />
        </aside>
      )}
    </div>
  );
}
