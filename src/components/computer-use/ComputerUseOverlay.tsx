/** Nonactivating local overlay entry. Product state stays out of the app shell. */
import { useEffect, useRef, useState, type MouseEvent } from "react";
import { createT, loadLocaleCatalog, resolveLocale, type Locale } from "@/i18n";
import { emptyOverlay, overlayControl, sampleCursor, watchOverlay, type ControlAction, type OverlayFrame } from "@/lib/computer-use/overlay";
import "./computer-use-overlay.css";

function stoppedCopy(reason: OverlayFrame["stop_reason"]): string {
  if (reason === "hotkey") return "computerUse.overlay.stoppedHotkey";
  if (reason === "panel") return "computerUse.overlay.stoppedPanel";
  if (reason === "internal_error") return "computerUse.overlay.stoppedInternal";
  return "computerUse.overlay.stopped";
}

type Props = {
  frame: OverlayFrame;
  locale: Locale;
  controls?: boolean;
  onControl: (action: ControlAction, event: { isTrusted: boolean }) => Promise<void> | void;
};

export function ComputerUseOverlay({ frame, locale, controls = false, onControl }: Props) {
  const tr = createT(locale);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  const submitting = useRef(false);
  const cursor = useRef<HTMLDivElement>(null);
  const position = useRef<{ x: number; y: number } | null>(null);
  const x = frame.cursor?.x;
  const y = frame.cursor?.y;
  const kind = frame.cursor?.kind;
  useEffect(() => {
    if (x === undefined || y === undefined || controls || frame.phase === "stopped" || frame.phase === "hidden") {
      position.current = null;
      return;
    }
    const at = { x, y };
    const from = position.current ?? at;
    const start = performance.now();
    const reduced = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;
    let raf = 0;
    const draw = (now: number) => {
      const p = sampleCursor(from, at, now - start, reduced, frame.response_ms);
      position.current = p;
      if (cursor.current) cursor.current.style.transform = `translate(${p.x}px, ${p.y}px)`;
      if (!reduced && now - start < 1000) raf = requestAnimationFrame(draw);
    };
    draw(start);
    return () => cancelAnimationFrame(raf);
  }, [x, y, kind, controls, frame.phase, frame.response_ms]);

  if (frame.phase === "hidden") return null;
  if (!controls) return frame.cursor && frame.phase !== "stopped" ? (
    <div ref={cursor} data-testid="cu-cursor" data-kind={frame.cursor.kind} className="cu-overlay-cursor" aria-hidden="true">
      {frame.cursor.kind === "caret" ? "I" : frame.cursor.kind === "scroll" ? "↕" : frame.cursor.kind === "drag" ? "✥" : "➤"}
    </div>
  ) : null;
  const stopped = frame.phase === "stopped";
  const submit = async (event: MouseEvent<HTMLButtonElement>) => {
    if (submitting.current) return;
    submitting.current = true;
    setBusy(true);
    setFailed(false);
    try { await onControl(stopped ? "resume" : "stop", event.nativeEvent); }
    catch { setFailed(true); }
    finally { submitting.current = false; setBusy(false); }
  };
  return <div className="cu-overlay-capsule" data-stopped={stopped}>
    <div className="cu-overlay-copy" role="status" aria-live="polite">
      <span>{tr(stopped ? stoppedCopy(frame.stop_reason) : frame.phase === "foreground" ? "computerUse.overlay.foreground" : "computerUse.overlay.running")}</span>
      {failed ? <span role="alert">{tr("computerUse.overlay.failed")}</span> : !frame.connected && stopped ? <span>{tr("computerUse.overlay.unavailable")}</span> : frame.message ? <span dir="auto">{frame.message}</span> : null}
    </div>
    <button type="button" className="btn" disabled={busy || (stopped && !frame.connected)} aria-busy={busy} onPointerDown={(e) => e.preventDefault()} onClick={(e) => void submit(e)}>
      {tr(stopped ? "computerUse.overlay.resume" : "computerUse.overlay.stop")}
    </button>
  </div>;
}

/** Separate document; does not mount App, updater, chat hotkeys or media services. */
export function ComputerUseOverlayApp() {
  const [frame, setFrame] = useState(emptyOverlay);
  const [locale, setLocale] = useState<Locale>(() => resolveLocale(document.documentElement.lang));
  const [, catalogLoaded] = useState(0);
  useEffect(() => watchOverlay(setFrame), []);
  useEffect(() => {
    const sync = () => {
      // The host's boot script provides the persisted preference for each document.
      const boot = (window as Window & { __GROK_BOOT_LOCALE__?: string }).__GROK_BOOT_LOCALE__;
      setLocale(resolveLocale(boot ?? navigator.language));
    };
    sync();
    window.addEventListener("languagechange", sync);
    return () => window.removeEventListener("languagechange", sync);
  }, []);
  useEffect(() => {
    let disposed = false;
    void loadLocaleCatalog(locale).then(() => { if (!disposed) catalogLoaded((v) => v + 1); });
    return () => { disposed = true; };
  }, [locale]);
  return <ComputerUseOverlay frame={frame} locale={locale} controls={window.location.hash === "#/cu-overlay-controls"} onControl={overlayControl} />;
}
