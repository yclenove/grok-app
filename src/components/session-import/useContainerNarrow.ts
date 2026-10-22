import { useLayoutEffect, useRef, useState, type RefObject } from "react";

/** Width below this uses one reading area and the compact step menu. */
export const SESSION_IMPORT_NARROW_PX = 720;

/**
 * Measure a mounted pane. A zero width, which jsdom reports, stays wide so
 * tests do not invent a narrow layout the window never produced.
 */
export function useContainerNarrow(active: boolean): {
  ref: RefObject<HTMLDivElement | null>;
  narrow: boolean;
  width: number;
} {
  const ref = useRef<HTMLDivElement>(null);
  const [narrow, setNarrow] = useState(false);
  const [width, setWidth] = useState(0);
  useLayoutEffect(() => {
    const node = ref.current;
    if (!active || !node || typeof ResizeObserver === "undefined") {
      setWidth(0);
      setNarrow(false);
      return;
    }
    const apply = (next: number) => {
      setWidth(next);
      setNarrow(next > 0 && next < SESSION_IMPORT_NARROW_PX);
    };
    apply(node.getBoundingClientRect().width);
    const observer = new ResizeObserver((entries) => {
      apply(entries[0]?.contentRect.width ?? 0);
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, [active]);
  return { ref, narrow, width };
}
