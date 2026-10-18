/**
 * Computer side panel: timeline, status, Stop, and explicit user Resume.
 * Stop and Resume go through the overlay control commands.
 */

import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { IconClock, IconDeviceDesktop } from "@/components/icons";
import { useImageViewerOptional, type ImageViewerApi } from "@/components/ImageViewerContext";
import { createT, type Locale, type MessageKey } from "@/i18n";
import { loadComputerUseShot } from "@/lib/api/computerUseShot";
import { overlayControl, type ControlAction } from "@/lib/computer-use/overlay";
import {
  previewUrl,
  sanitizeLabel,
  sharedShotBook,
  type AdmittedShot,
  type ComputerUseToolName,
  type Outcome,
  type ShotBook,
  type ShotFault,
} from "@/lib/computer-use/events";
import { formatWorkDuration } from "@/lib/formatWorkDuration";
import { onMediaEndpointChange } from "@/lib/imageSrc";
import {
  COMPUTER_USE_SETTINGS_HASH,
  sharedTimeline,
  shownPhase,
  type ShownPhase,
  type TimelineStep,
  type TimelineStore,
} from "@/lib/computer-use/store";
import { useComputerUseEvents } from "@/hooks/useComputerUseEvents";
import { ComputerUseBrowser } from "@/components/computer-use/ComputerUseBrowser";

type ShotLoader = (sessionId: string, id: string) => Promise<AdmittedShot | ShotFault>;

const OUTCOME_KEY: Record<Outcome, MessageKey> = {
  done: "tool.outcome.done",
  refused: "tool.outcome.refused",
  failed: "tool.outcome.failed",
  unknown: "tool.outcome.unknown",
};

const TOOL_KEY: Record<ComputerUseToolName | "unknown", MessageKey> = {
  list_apps: "tool.name.list_apps",
  list_windows: "tool.name.list_windows",
  launch_app: "tool.name.launch_app",
  get_app_state: "tool.name.get_app_state",
  click: "tool.name.click",
  perform_secondary_action: "tool.name.perform_secondary_action",
  set_value: "tool.name.set_value",
  select_text: "tool.name.select_text",
  type_text: "tool.name.type_text",
  press_key: "tool.name.press_key",
  scroll: "tool.name.scroll",
  drag: "tool.name.drag",
  wait_for: "tool.name.wait_for",
  computer_batch: "tool.name.computer_batch",
  computer_script: "tool.name.computer_script",
  capabilities: "tool.name.capabilities",
  unknown: "tool.name.unknown",
};

function toolKey(name: string): MessageKey {
  return name in TOOL_KEY ? TOOL_KEY[name as ComputerUseToolName | "unknown"] : TOOL_KEY.unknown;
}

function statusKey(phase: ShownPhase): MessageKey {
  switch (phase) {
    case "stopped":
      return "computerUse.overlay.stopped";
    case "tripped":
      return "computerUse.panel.status.tripped";
    case "waiting":
      return "computerUse.panel.status.waiting";
    case "handoff":
      return "computerUse.panel.status.handoff";
    case "restarted":
      return "computerUse.panel.status.restarted";
    case "failed":
      return "computerUse.panel.status.failed";
    case "refused":
      return "computerUse.panel.status.refused";
    case "unknown":
      return "computerUse.panel.status.unknown";
    case "running":
      return "computerUse.panel.status.running";
    default:
      return "computerUse.panel.status.idle";
  }
}

export function ComputerUsePanel({
  sessionId,
  locale = "en",
  store = sharedTimeline,
  book = sharedShotBook,
  loadShot = loadComputerUseShot,
  onControl = overlayControl,
  viewer: viewerProp,
  waiting: waitingProp,
  waitingTarget: waitingTargetProp,
  handoff: handoffProp,
}: {
  sessionId: string | null;
  locale?: Locale | string;
  store?: TimelineStore;
  book?: ShotBook;
  loadShot?: ShotLoader;
  onControl?: (action: ControlAction, event: { isTrusted: boolean }) => Promise<void> | void;
  viewer?: ImageViewerApi;
  waiting?: boolean;
  waitingTarget?: string;
  handoff?: boolean;
}) {
  const live = useComputerUseEvents(sessionId, store, book);
  const waiting = waitingProp ?? live.waiting;
  const handoff = handoffProp ?? live.handoff;
  const waitingTarget = waitingTargetProp ?? live.waitingTarget;
  const tr = useMemo(() => createT(locale as Locale), [locale]);
  const contextViewer = useImageViewerOptional();
  const viewer = viewerProp ?? contextViewer;
  const view = live.view;
  const phase = shownPhase(view, waiting, handoff);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  const submitting = useRef(false);
  const last = view.steps[view.steps.length - 1];
  const target = sanitizeLabel(phase === "waiting" ? waitingTarget || last?.target || "" : last?.target || "");
  const empty = phase === "idle" && view.steps.length === 0;
  const stopped = phase === "stopped";

  const submit = async (action: ControlAction, event: { isTrusted: boolean }) => {
    if (submitting.current) return;
    submitting.current = true;
    setBusy(true);
    setFailed(false);
    try {
      await onControl(action, event);
      if (action === "stop") store.userStop();
      else store.userResume();
    } catch {
      setFailed(true);
    } finally {
      submitting.current = false;
      setBusy(false);
    }
  };

  return (
    <section className="sw-computer" data-testid="cu-panel" data-phase={phase} aria-label={tr("computerUse.panel.title")}>
      <header className="sw-computer__head">
        <div className="sw-computer__title-row">
          <IconDeviceDesktop size={16} />
          <h2 className="sw-computer__title">{tr("computerUse.panel.title")}</h2>
          <div className="sw-computer__actions">
            <button
              type="button"
              className={stopped ? "btn" : "btn btn--solid btn--danger"}
              data-testid={stopped ? "cu-panel-resume" : "cu-panel-stop"}
              disabled={busy}
              aria-busy={busy}
              onClick={(event) => void submit(stopped ? "resume" : "stop", event.nativeEvent)}
            >
              {tr(stopped ? "computerUse.overlay.resume" : "computerUse.overlay.stop")}
            </button>
          </div>
        </div>
        <p className="sw-computer__status" data-testid="cu-panel-status" role="status" aria-live="polite">
          {tr(statusKey(phase), { target })}
        </p>
        {failed || live.loadFailed ? (
          <p className="sw-computer__alert" role="alert">{tr("computerUse.overlay.failed")}</p>
        ) : null}
      </header>
      {empty ? (
        <div className="rp__empty-state" data-testid="cu-panel-empty">
          <div className="rp__empty-title">{tr("computerUse.panel.emptyTitle")}</div>
          <p className="rp__empty-desc">{tr("computerUse.panel.emptyBody")}</p>
          <button
            type="button"
            className="btn"
            data-testid="cu-panel-settings"
            onClick={() => {
              window.location.hash = COMPUTER_USE_SETTINGS_HASH;
            }}
          >
            {tr("computerUse.panel.openSettings")}
          </button>
        </div>
      ) : (
        <ol className="sw-computer__list" aria-label={tr("computerUse.panel.timeline")}>
          {view.steps.map((step, index) => (
            <StepRow
              key={`${step.atMs}-${index}`}
              step={step}
              sessionId={sessionId}
              locale={locale as Locale}
              book={book}
              loadShot={loadShot}
              viewer={viewer}
            />
          ))}
          {view.dropped > 0 ? (
            <li className="sw-computer__dropped" data-testid="cu-panel-dropped">
              {tr("computerUse.panel.dropped", { n: view.dropped })}
            </li>
          ) : null}
        </ol>
      )}
      <ComputerUseBrowser locale={locale as Locale} />
    </section>
  );
}

function StepRow({
  step,
  sessionId,
  locale,
  book,
  loadShot,
  viewer,
}: {
  step: TimelineStep;
  sessionId: string | null;
  locale: Locale;
  book: ShotBook;
  loadShot: ShotLoader;
  viewer: ImageViewerApi | null;
}) {
  const tr = useMemo(() => createT(locale), [locale]);
  const shot = useSyncExternalStore(
    book.subscribe,
    () => book.viewFor(sessionId, step.shotId),
    () => book.viewFor(sessionId, step.shotId),
  );
  const [, setMediaTick] = useState(0);
  useEffect(() => onMediaEndpointChange(() => setMediaTick((tick) => tick + 1)), []);
  useEffect(() => {
    const id = step.shotId;
    if (!sessionId || !id) return;
    const current = book.resolve(sessionId, id);
    if (current === "foreign") return;
    if (typeof current !== "string" && current.full) return;
    let cancel = false;
    void loadShot(sessionId, id).then((result) => {
      if (cancel) return;
      if (typeof result === "string") {
        book.noteFault(sessionId, id, result);
        return;
      }
      if (result.sessionId === sessionId && result.id === id) book.admit(result);
    });
    return () => {
      cancel = true;
    };
  }, [book, loadShot, sessionId, step.shotId]);

  const target = sanitizeLabel(step.target);
  const duration = step.durationMs < 1000
    ? tr("tool.durationMs", { n: step.durationMs })
    : formatWorkDuration(Math.floor(step.durationMs / 1000), locale);
  const shotPath = shot.kind === "ready" ? shot.full || shot.thumb : null;
  const preview = shotPath ? previewUrl(shotPath) : null;

  return (
    <li className="sw-computer__step" data-testid="cu-panel-step" data-outcome={step.outcome}>
      <div className="sw-computer__step-main">
        <span className="sw-computer__tool">{tr(toolKey(step.tool))}</span>
        <span className="perm-bar__badge">{tr(OUTCOME_KEY[step.outcome])}</span>
        <span className="sw-computer__duration">
          <IconClock size={12} stroke={1.5} />
          {duration}
        </span>
        {target ? <div className="sw-computer__target">{target}</div> : null}
        {preview && shotPath ? (
          <button
            type="button"
            className="sw-computer__thumb"
            data-testid="cu-panel-thumb"
            aria-label={tr("tool.enlarge")}
            onClick={() => viewer?.open([{ src: shotPath, alt: tr("tool.enlarge") }])}
          >
            <img src={preview} alt="" />
          </button>
        ) : shot.kind === "foreign" ? (
          <p className="sw-computer__note">{tr("tool.shot.foreign")}</p>
        ) : null}
      </div>
    </li>
  );
}
