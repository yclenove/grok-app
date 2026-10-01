// Fixed, document-bound code. Scroll/resize/visibility changes fence an in-flight capture,
// including a change away and back to the same geometry between the two inspections.
export function inspectDocument() {
  let state = globalThis.__grokComputerUseView;
  if (!state) {
    state = { generation: 0 };
    globalThis.__grokComputerUseView = state;
    const changed = () => { state.generation++; };
    for (const type of ["scroll", "resize", "blur", "pagehide"]) addEventListener(type, changed, { capture: true, passive: true });
    document.addEventListener("visibilitychange", changed);
    visualViewport?.addEventListener("resize", changed);
    visualViewport?.addEventListener("scroll", changed);
  }
  return { url: location.href, title: document.title, visible: document.visibilityState === "visible",
    viewportWidth: innerWidth, viewportHeight: innerHeight, scrollX, scrollY, pixelRatio: devicePixelRatio,
    visualScale: visualViewport?.scale ?? 1, visualX: visualViewport?.offsetLeft ?? 0,
    visualY: visualViewport?.offsetTop ?? 0, viewGeneration: state.generation };
}

export function sameView(a, b) {
  return ["url", "documentId", "viewportWidth", "viewportHeight", "scrollX", "scrollY", "pixelRatio",
    "visualScale", "visualX", "visualY", "viewGeneration"].every(key => a[key] === b[key]);
}
