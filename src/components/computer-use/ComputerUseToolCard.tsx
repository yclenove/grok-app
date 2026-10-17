/**
 * Computer Use tool row in the chat timeline.
 * Thumbnails come from the redacted shot ledger, never from model text.
 */

import { useEffect, useMemo, useState, useSyncExternalStore } from "react";
import {
  IconChevronRight,
  IconClick,
  IconClock,
  IconDeviceDesktop,
  IconEye,
  IconKeyboard,
} from "@/components/icons";
import {
  useImageViewerOptional,
  type ImageViewerApi,
} from "@/components/ImageViewerContext";
import { createT, type Locale, type MessageKey } from "@/i18n";
import { loadComputerUseShot } from "@/lib/api/computerUseShot";
import {
  previewUrl,
  readComputerUseCall,
  sharedShotBook,
  type ComputerUseToolName,
  type Delivery,
  type Effect,
  type Outcome,
  type ShotBook,
  type ShotFault,
  type AdmittedShot,
} from "@/lib/computer-use/events";
import { onMediaEndpointChange } from "@/lib/imageSrc";
import { formatWorkDuration } from "@/lib/formatWorkDuration";
import type { MessageToolSegment } from "@/lib/session";

export type ShotLoader = (
  sessionId: string,
  id: string,
) => Promise<AdmittedShot | ShotFault>;

const OUTCOME_KEY: Record<Outcome, MessageKey> = {
  done: "tool.outcome.done",
  refused: "tool.outcome.refused",
  failed: "tool.outcome.failed",
  unknown: "tool.outcome.unknown",
};

const EFFECT_KEY: Record<Effect, MessageKey> = {
  changed: "tool.effect.changed",
  no_visible_change: "tool.effect.no_visible_change",
  not_checked: "tool.effect.not_checked",
};

const DELIVERY_KEY: Record<Delivery, MessageKey> = {
  accessibility: "tool.delivery.accessibility",
  targeted_event: "tool.delivery.targeted_event",
  foreground: "tool.delivery.foreground",
  clipboard: "tool.delivery.clipboard",
  none: "tool.delivery.none",
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

function ToolIcon({ name }: { name: string }) {
  const props = { size: 15, stroke: 1.5 } as const;
  if (name === "click" || name === "perform_secondary_action") return <IconClick {...props} />;
  if (
    name === "type_text" ||
    name === "set_value" ||
    name === "select_text" ||
    name === "press_key"
  ) {
    return <IconKeyboard {...props} />;
  }
  if (name === "get_app_state" || name === "list_apps" || name === "list_windows") {
    return <IconEye {...props} />;
  }
  return <IconDeviceDesktop {...props} />;
}

export function ComputerUseToolCard({
  tool,
  sessionId = null,
  locale = "en",
  isLast = true,
  viewer: viewerProp,
  loadShot = loadComputerUseShot,
  book = sharedShotBook,
}: {
  tool: MessageToolSegment;
  sessionId?: string | null;
  locale?: Locale;
  isLast?: boolean;
  viewer?: ImageViewerApi;
  loadShot?: ShotLoader;
  book?: ShotBook;
}) {
  const contextViewer = useImageViewerOptional();
  const viewer = viewerProp ?? contextViewer;
  const tr = useMemo(() => createT(locale), [locale]);
  const call = useMemo(() => readComputerUseCall(tool), [tool]);
  const view = useSyncExternalStore(
    book.subscribe,
    () => book.viewFor(sessionId, call?.screenshotId ?? null),
    () => book.viewFor(sessionId, call?.screenshotId ?? null),
  );
  const [open, setOpen] = useState(false);
  const [, setMediaTick] = useState(0);

  useEffect(() => onMediaEndpointChange(() => setMediaTick((tick) => tick + 1)), []);

  useEffect(() => {
    const id = call?.screenshotId;
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
  }, [book, call?.screenshotId, loadShot, sessionId]);

  if (!call) return null;

  const failed = call.outcome === "failed" || call.outcome === "refused";
  const className = [
    "grok-act__step",
    "lobe-timeline-tool",
    "cu-tool",
    call.pending ? "is-running" : "",
    failed ? "is-error" : "",
    call.outcome === "unknown" ? "is-warning" : "",
    isLast ? "is-last" : "",
  ]
    .filter(Boolean)
    .join(" ");
  const outcome = call.outcome ? tr(OUTCOME_KEY[call.outcome]) : "";
  const effect = call.effect ? tr(EFFECT_KEY[call.effect]) : "";
  const badge = call.pending
    ? tr("tool.badgePending")
    : call.delivery
      ? tr("tool.badgeDelivery", {
          outcome,
          effect,
          delivery: tr(DELIVERY_KEY[call.delivery]),
        })
      : tr("tool.badge", { outcome, effect });
  const target =
    call.app && call.element
      ? tr("tool.target", { app: call.app, element: call.element })
      : call.app || call.element;
  const duration =
    call.durationMs == null
      ? ""
      : call.durationMs < 1000
        ? tr("tool.durationMs", { n: call.durationMs })
        : formatWorkDuration(Math.floor(call.durationMs / 1000), locale);
  const shotPath =
    view.kind === "ready" ? view.full || view.thumb : null;
  const preview = shotPath ? previewUrl(shotPath) : null;

  return (
    <div
      className={className}
      role="status"
      data-testid="cu-tool-card"
      data-tool-id={tool.toolCallId}
      data-outcome={call.pending ? "pending" : call.outcome ?? "unknown"}
    >
      <div className="grok-act__icon-col" aria-hidden>
        <span className="grok-act__icon">
          <ToolIcon name={call.tool} />
        </span>
        {!isLast ? <span className="grok-act__rail" /> : null}
      </div>
      <div className="cu-tool__main">
        <div className="cu-tool__row">
          <span className="grok-act__label">{tr(TOOL_KEY[call.tool])}</span>
          <span className="perm-bar__badge cu-tool__badge" data-testid="cu-tool-badge">
            {badge}
          </span>
          {duration ? (
            <span className="cu-tool__duration" data-testid="cu-tool-duration">
              <IconClock size={12} stroke={1.5} />
              {duration}
            </span>
          ) : null}
          {call.raw ? (
            <button
              type="button"
              className="grok-act__step-btn cu-tool__expand"
              data-testid="cu-tool-expand"
              aria-expanded={open}
              aria-label={tr("tool.raw")}
              onClick={() => setOpen((value) => !value)}
            >
              <span className={"grok-act__mini-caret" + (open ? " is-open" : "")} aria-hidden>
                <IconChevronRight size={11} />
              </span>
            </button>
          ) : null}
        </div>
        {target ? (
          <div className="cu-tool__target" data-testid="cu-tool-target">
            {target}
          </div>
        ) : null}
        {preview && shotPath ? (
          <button
            type="button"
            className="cu-tool__thumb"
            data-testid="cu-tool-thumb"
            aria-label={tr("tool.enlarge")}
            onClick={() => viewer.open([{ src: shotPath, alt: tr("tool.enlarge") }])}
          >
            <img src={preview} alt="" />
          </button>
        ) : view.kind === "foreign" ? (
          <p className="cu-tool__note">{tr("tool.shot.foreign")}</p>
        ) : view.kind === "missing" || view.kind === "rejected" ? (
          <p className="cu-tool__note">
            {tr(view.kind === "rejected" ? "tool.noThumb" : "tool.shot.missing")}
          </p>
        ) : null}
        {open && call.raw ? (
          <pre className="cu-tool__raw" data-testid="cu-tool-raw">
            {call.raw}
          </pre>
        ) : null}
      </div>
    </div>
  );
}
