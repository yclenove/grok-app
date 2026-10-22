import { useEffect, useRef, useState } from "react";
import {
  resolveMarkdownPaintSource,
  resolveStreamMarkdownParseMs,
} from "@/lib/streamRenderPolicy";

/** One pending paint deadline; incoming chunks replace its payload, not its clock. */
export function useStreamingMarkdownSource(source: string, streaming: boolean): string {
  const [painted, setPainted] = useState(source);
  const latestRef = useRef(source);
  const publishedRef = useRef(source);
  const timerRef = useRef<number | null>(null);
  const interval = resolveStreamMarkdownParseMs(source.length, streaming);

  useEffect(() => {
    latestRef.current = source;
    if (!streaming || interval <= 0) {
      if (timerRef.current !== null) {
        window.clearTimeout(timerRef.current);
        timerRef.current = null;
      }
      publishedRef.current = source;
      setPainted(source);
      return;
    }
    if (source === publishedRef.current || timerRef.current !== null) return;
    timerRef.current = window.setTimeout(() => {
      timerRef.current = null;
      publishedRef.current = latestRef.current;
      setPainted(latestRef.current);
    }, interval);
    // Do not cancel on each source change: that debounces indefinitely when
    // stream chunks arrive more frequently than the markdown parse interval.
  }, [source, streaming, interval]);

  useEffect(() => () => {
    if (timerRef.current !== null) {
      window.clearTimeout(timerRef.current);
      timerRef.current = null;
    }
  }, []);

  // Settle must render the unmodified final source in the same frame.
  return resolveMarkdownPaintSource(streaming, source, painted);
}
