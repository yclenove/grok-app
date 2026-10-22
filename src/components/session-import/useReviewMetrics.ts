import { useEffect, useState } from "react";

type ReviewState = {
  open: boolean;
  surface: string;
  locale: string;
  theme: string;
  reader: string;
  layout: string;
  measureWidth: number;
  localeReady: boolean;
};

function bounds(node: Element | null | undefined) {
  if (!node) return null;
  const rect = node.getBoundingClientRect();
  return {
    width: rect.width, height: rect.height,
    top: rect.top, left: rect.left, right: rect.right, bottom: rect.bottom,
  };
}

/** Measure real rendered geometry; this is review evidence, never native proof. */
export function useReviewMetrics(state: ReviewState): string {
  const [metrics, setMetrics] = useState("");
  const { open, surface, locale, theme, reader, layout, measureWidth, localeReady } = state;

  useEffect(() => {
    let timer: number | undefined;
    let resize: ResizeObserver | null = null;
    let content: MutationObserver | null = null;
    let observed: Array<Element | null | undefined> = [];
    const publish = () => {
      const current = document.querySelector(".session-import-review-frame");
      const currentBody = current?.querySelector(".modal-body");
      const currentFooter = current?.querySelector(".session-import__footer, .import-reader-actions")
        ?? current?.querySelector(".modal-actions");
      const targets = [current, currentBody, currentFooter];
      // An asynchronously mounted composition may appear after this effect.
      // Bind observers when its portal arrives, and rebind on replacement.
      if (targets.some((node, index) => node !== observed[index])) {
        observed = targets;
        resize?.disconnect(); content?.disconnect();
        for (const node of targets) if (node) resize?.observe(node);
        if (current) content?.observe(current, {
          subtree: true, childList: true, characterData: true,
          attributes: true, attributeFilter: ["class", "style", "hidden", "inert", "open", "aria-busy", "data-layout", "data-measured-width", "data-reviewing"],
        });
      }
      const measurement = current?.querySelector<HTMLElement>("[data-review-surface]");
      const handoffGrid = current?.querySelector(".session-import__handoff-grid");
      const columns = handoffGrid ? window.getComputedStyle(handoffGrid).gridTemplateColumns.trim() : "";
      const handoffLayout = columns && columns !== "none" ? (columns.split(/\s+/).length > 1 ? "wide" : "narrow") : null;
      setMetrics(JSON.stringify({
        viewport: {
          width: window.innerWidth, height: window.innerHeight,
          devicePixelRatio: window.devicePixelRatio,
        },
        modal: bounds(current), body: bounds(currentBody),
        measureWidth: measurement ? Number(measurement.dataset.measuredWidth ?? measurement.getBoundingClientRect().width)
          : currentBody?.getBoundingClientRect().width ?? measureWidth,
        layout: measurement?.dataset.layout ?? handoffLayout ?? layout, surface, locale, theme, reader, localeReady,
        observedSearchState: current?.querySelector("[data-import-search-state]")?.getAttribute("data-import-search-state")
          ?? (current?.querySelector(".session-import__hits") ? "hits" : surface === "reader" ? "idle" : null),
        bodyOverflow: currentBody ? currentBody.scrollHeight > currentBody.clientHeight + 1 : null,
        bodyClientHeight: currentBody?.clientHeight ?? null,
        bodyScrollHeight: currentBody?.scrollHeight ?? null,
        bodyClientWidth: currentBody?.clientWidth ?? null,
        bodyScrollWidth: currentBody?.scrollWidth ?? null,
        footer: bounds(currentFooter),
        footerInBody: currentFooter ? !!currentFooter.closest(".modal-body") : null,
        firstCandidate: bounds(current?.querySelector("[data-import-candidate]")),
        preview: bounds(current?.querySelector(".session-import__preview-pane")),
        dialogs: document.querySelectorAll("[role='dialog']").length,
        focusedInFrame: !!current?.contains(document.activeElement),
      }));
    };
    const schedule = () => {
      if (timer !== undefined) return;
      timer = window.setTimeout(() => { timer = undefined; publish(); }, 0);
    };
    window.addEventListener("resize", schedule);
    document.addEventListener("focusin", schedule);
    resize = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(schedule);
    content = typeof MutationObserver === "undefined" ? null : new MutationObserver(schedule);
    // Direct children catch portaled dialogs. The metrics node lives outside
    // the measured modal, preventing its JSON updates from retriggering us.
    const portals = typeof MutationObserver === "undefined" ? null : new MutationObserver(schedule);
    portals?.observe(document.body, { childList: true });
    publish();
    return () => {
      if (timer !== undefined) window.clearTimeout(timer);
      window.removeEventListener("resize", schedule);
      document.removeEventListener("focusin", schedule);
      resize?.disconnect();
      content?.disconnect();
      portals?.disconnect();
    };
  }, [open, surface, locale, theme, reader, layout, measureWidth, localeReady]);

  return metrics;
}
