// @vitest-environment jsdom
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { LAYOUT_STORAGE_KEY } from "@/lib/layout";
import { useWorkbenchLayout } from "./useWorkbenchLayout";

const windowFit = vi.hoisted(() => ({ ensure: vi.fn(async () => {}) }));

vi.mock("@/lib/api", () => ({ isDesktopHost: () => false }));
vi.mock("@/lib/mirrorTransport", () => ({ isMirrorClient: () => false }));
vi.mock("@/lib/appPlatform", () => ({ detectAppPlatform: () => "windows", usesCustomWindowChrome: () => true }));
vi.mock("@/lib/windowFit", async (importOriginal) => ({
  ...await importOriginal<typeof import("@/lib/windowFit")>(),
  ensureWindowFitsLayout: windowFit.ensure,
  isWindowFitSuppressed: () => false,
}));
beforeEach(() => {
  windowFit.ensure.mockReset();
  windowFit.ensure.mockResolvedValue(undefined);
  localStorage.clear();
  vi.stubGlobal("innerWidth", 1202);
});
afterEach(() => { cleanup(); vi.unstubAllGlobals(); localStorage.clear(); document.body.innerHTML = ""; });

describe("shared workbench resize controller", () => {
  it.each(["keyboard", "pointer"])("a late open-pane fit cannot replace the user's %s width", async (input) => {
    let finishFit!: () => void;
    windowFit.ensure.mockReturnValueOnce(new Promise<void>((resolve) => { finishFit = resolve; }));
    const { result } = renderHook(() => useWorkbenchLayout());
    act(() => result.current.openAsidePane());
    expect(result.current.layout.asideCollapsed).toBe(false);
    if (input === "keyboard") {
      act(() => result.current.asideResize.change({ kind: "maximum" }));
    } else {
      act(() => result.current.asideResize.begin());
      act(() => window.dispatchEvent(new MouseEvent("pointermove", { clientX: 628 })));
      act(() => window.dispatchEvent(new Event("pointerup")));
    }
    expect(result.current.layout.asideWidth).toBe(574);
    await act(async () => { finishFit(); });
    expect(result.current.layout.asideWidth).toBe(574);
    expect(JSON.parse(localStorage.getItem(LAYOUT_STORAGE_KEY)!).asideWidth).toBe(574);
  });

  it("accumulates rapid keys, persists only geometry and never opens a closed pane", async () => {
    const { result } = renderHook(() => useWorkbenchLayout());
    act(() => result.current.asideResize.change({ kind: "maximum" }));
    expect(result.current.layout.asideCollapsed).toBe(true);
    act(() => result.current.openAsidePane());
    await waitFor(() => expect(result.current.layout.asideCollapsed).toBe(false));
    act(() => result.current.asideResize.change({ kind: "minimum" }));
    expect(result.current.asideResize.value).toBe(458);
    act(() => {
      result.current.asideResize.change({ kind: "delta", pixels: 10 });
      result.current.asideResize.change({ kind: "delta", pixels: 10 });
    });
    expect(result.current.layout.asideWidth).toBe(478);
    expect(JSON.parse(localStorage.getItem(LAYOUT_STORAGE_KEY)!).asideWidth).toBe(478);
    act(() => result.current.asideResize.change({ kind: "maximum" }));
    expect(result.current.layout.asideWidth).toBe(574);
  });

  it("a late sidebar fit cannot replace the newer side-pane width", async () => {
    const { result } = renderHook(() => useWorkbenchLayout());
    act(() => result.current.closeSidebarPane());
    await act(async () => result.current.openAsidePane());
    act(() => result.current.asideResize.change({ kind: "minimum" }));
    act(() => result.current.asideResize.change({ kind: "delta", pixels: 42 }));
    expect(result.current.layout.asideWidth).toBe(500);
    let finishFit!: () => void;
    windowFit.ensure.mockReturnValueOnce(new Promise<void>((resolve) => { finishFit = resolve; }));
    act(() => result.current.openSidebarPane());
    act(() => result.current.asideResize.change({ kind: "maximum" }));
    expect(result.current.layout.asideWidth).toBe(574);
    await act(async () => { finishFit(); });
    expect(result.current.layout.asideWidth).toBe(574);
    expect(result.current.layout.sidebarCollapsed).toBe(false);
  });

  it.each(["pointercancel", "blur"])("ends drag ownership on %s and keeps the last bounded width", async (event) => {
    document.body.innerHTML = '<div class="workbench"><aside class="aside"><div class="aside-resizer"></div></aside></div>';
    const { result } = renderHook(() => useWorkbenchLayout());
    act(() => result.current.openAsidePane());
    await waitFor(() => expect(result.current.layout.asideCollapsed).toBe(false));
    act(() => result.current.asideResize.begin());
    expect(document.body.style.cursor).toBe("col-resize");
    act(() => window.dispatchEvent(new MouseEvent("pointermove", { clientX: 640 })));
    expect(document.querySelector(".aside-resizer")!.getAttribute("aria-valuenow")).toBe("562");
    act(() => window.dispatchEvent(new Event(event)));
    expect(result.current.resizingAside).toBe(false);
    expect(result.current.layout.asideWidth).toBe(562);
    expect(document.body.style.cursor).toBe("");
    expect(document.body.style.userSelect).toBe("");
    act(() => window.dispatchEvent(new MouseEvent("pointermove", { clientX: 700 })));
    expect(result.current.layout.asideWidth).toBe(562);
  });

  it("closing a pane during drag cannot be undone by a late pointer-up", async () => {
    const { result } = renderHook(() => useWorkbenchLayout());
    act(() => result.current.openAsidePane());
    await waitFor(() => expect(result.current.layout.asideCollapsed).toBe(false));
    act(() => result.current.asideResize.begin());
    act(() => result.current.closeAsidePane());
    expect(result.current.layout.asideCollapsed).toBe(true);
    act(() => window.dispatchEvent(new Event("pointerup")));
    expect(result.current.layout.asideCollapsed).toBe(true);
    expect(result.current.resizingAside).toBe(false);
    expect(document.body.style.cursor).toBe("");
  });

  it("restores input styling when the workbench unmounts during drag", async () => {
    const { result, unmount } = renderHook(() => useWorkbenchLayout());
    act(() => result.current.openAsidePane());
    await waitFor(() => expect(result.current.layout.asideCollapsed).toBe(false));
    act(() => result.current.asideResize.begin());
    expect(document.body.style.userSelect).toBe("none");
    unmount();
    expect(document.body.style.cursor).toBe("");
    expect(document.body.style.userSelect).toBe("");
  });
});
