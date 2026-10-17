/**
 * Side-panel timeline. The admission window matches `gcu-host-protocol` panel:
 * ten events per session per rolling second, newest 200 steps, stop latch.
 * Production replaces host snapshots. `push` is the same throttle for tests.
 */

import type { Outcome } from "@/lib/computer-use/events";

export const TIMELINE_WINDOW_MS = 1000;
export const TIMELINE_MAX_PER_WINDOW = 10;
export const TIMELINE_MAX_STEPS = 200;
export const COMPUTER_USE_SETTINGS_HASH = "#/settings/extensions/computer";

export type EnginePhase =
  | "idle"
  | "running"
  | "refused"
  | "failed"
  | "unknown"
  | "stopped"
  | "restarted"
  | "tripped";

export type ShownPhase = EnginePhase | "waiting" | "handoff";

export type TimelineStep = {
  tool: string;
  target: string;
  outcome: Outcome;
  durationMs: number;
  shotId: string | null;
  atMs: number;
};

export type PanelView = {
  sessionId: string;
  phase: EnginePhase;
  stopped: boolean;
  tripped: boolean;
  restarted: boolean;
  steps: TimelineStep[];
  dropped: number;
  revision: number;
};

export type TimelineNotice =
  | { kind: "snapshot"; view: PanelView }
  | { kind: "engine"; stopped: boolean; tripped: boolean; restarted: boolean; revision: number }
  | { kind: "forgotten"; sessionId: string; revision: number };

export type TimelineInput = {
  sessionId: string;
  tool: string;
  target: string;
  outcome: Outcome;
  durationMs: number;
  shotId?: string | null;
};

export type Admit = "kept" | "throttled" | "rejected";

type Engine = { stopped: boolean; tripped: boolean; restarted: boolean };
type Slot = { steps: TimelineStep[]; admittedAt: number[]; dropped: number };

const EMPTY_ENGINE: Engine = { stopped: false, tripped: false, restarted: false };

function safeKey(value: string): boolean {
  return value.length >= 1 && value.length <= 128 && /^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(value);
}

function outcomeOf(value: string): Outcome {
  if (value === "done" || value === "refused" || value === "failed" || value === "unknown") return value;
  return "unknown";
}

function clip(value: string): string {
  const chars = Array.from(value);
  return chars.length <= 280 ? value : chars.slice(0, 280).join("");
}

function phaseOf(engine: Engine, steps: TimelineStep[]): EnginePhase {
  if (engine.stopped) return "stopped";
  if (engine.tripped) return "tripped";
  if (engine.restarted) return "restarted";
  const last = steps[steps.length - 1];
  if (!last) return "idle";
  return last.outcome === "done" ? "running" : last.outcome;
}

function copyStep(step: TimelineStep): TimelineStep {
  return {
    tool: step.tool,
    target: clip(step.target),
    outcome: outcomeOf(step.outcome),
    durationMs: step.durationMs,
    shotId: step.shotId && safeKey(step.shotId) ? step.shotId : null,
    atMs: step.atMs,
  };
}

/** Session timelines plus the global stop, trip, and restart flags. */
export class TimelineStore {
  private sessions = new Map<string, Slot>();
  private engine: Engine = { ...EMPTY_ENGINE };
  private hold: boolean | null = null;
  private hostRevision = 0;
  private listeners = new Set<() => void>();
  private cache = new Map<string, PanelView>();

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  view(sessionId: string | null | undefined): PanelView {
    const key = sessionId ?? "";
    const hit = this.cache.get(key);
    if (hit) return hit;
    const slot = sessionId ? this.sessions.get(sessionId) : undefined;
    const engine = this.shownEngine();
    const steps = slot ? slot.steps : [];
    const view: PanelView = {
      sessionId: key,
      phase: phaseOf(engine, steps),
      stopped: engine.stopped,
      tripped: engine.tripped,
      restarted: engine.restarted,
      steps,
      dropped: slot?.dropped ?? 0,
      revision: this.hostRevision,
    };
    this.cache.set(key, view);
    return view;
  }

  /** Host snapshot. An older revision does not overwrite a newer notice. */
  replace(view: PanelView): void {
    if (!safeKey(view.sessionId) || view.revision < this.hostRevision) return;
    this.hostRevision = view.revision;
    this.engine = { stopped: view.stopped, tripped: view.tripped, restarted: view.restarted };
    this.sessions.set(view.sessionId, {
      steps: view.steps.map(copyStep),
      admittedAt: [],
      dropped: view.dropped,
    });
    this.catchUp(view.stopped);
    this.emit();
  }

  apply(notice: TimelineNotice): void {
    const revision = notice.kind === "snapshot" ? notice.view.revision : notice.revision;
    if (revision < this.hostRevision) return;
    this.hostRevision = revision;
    if (notice.kind === "snapshot") {
      this.engine = {
        stopped: notice.view.stopped,
        tripped: notice.view.tripped,
        restarted: notice.view.restarted,
      };
      if (safeKey(notice.view.sessionId)) {
        this.sessions.set(notice.view.sessionId, {
          steps: notice.view.steps.map(copyStep),
          admittedAt: [],
          dropped: notice.view.dropped,
        });
      }
      this.catchUp(notice.view.stopped);
    } else if (notice.kind === "engine") {
      this.engine = { stopped: notice.stopped, tripped: notice.tripped, restarted: notice.restarted };
      this.catchUp(notice.stopped);
    } else if (safeKey(notice.sessionId)) {
      this.sessions.delete(notice.sessionId);
    }
    this.emit();
  }

  /** Test and optimistic admission. The host snapshot path uses `replace`. */
  push(event: TimelineInput, nowMs: number): Admit {
    if (!safeKey(event.sessionId) || !Number.isFinite(nowMs)) return "rejected";
    if (event.outcome !== "done" && event.outcome !== "refused" && event.outcome !== "failed" && event.outcome !== "unknown") {
      return "rejected";
    }
    const slot = this.sessions.get(event.sessionId) ?? { steps: [], admittedAt: [], dropped: 0 };
    while (slot.admittedAt.length > 0 && nowMs - slot.admittedAt[0]! >= TIMELINE_WINDOW_MS) {
      slot.admittedAt.shift();
    }
    if (slot.admittedAt.length >= TIMELINE_MAX_PER_WINDOW) {
      slot.dropped += 1;
      this.sessions.set(event.sessionId, slot);
      this.emit();
      return "throttled";
    }
    slot.admittedAt.push(nowMs);
    slot.steps.push(copyStep({
      tool: event.tool,
      target: event.target,
      outcome: event.outcome,
      durationMs: event.durationMs,
      shotId: event.shotId ?? null,
      atMs: nowMs,
    }));
    while (slot.steps.length > TIMELINE_MAX_STEPS) slot.steps.shift();
    this.sessions.set(event.sessionId, slot);
    if (this.engine.restarted) this.engine = { ...this.engine, restarted: false };
    this.emit();
    return "kept";
  }

  userStop(): void {
    this.hold = true;
    this.engine = { ...this.engine, stopped: true };
    this.emit();
  }

  /** False when Stop is not latched. Does not clear a latch the user did not set. */
  userResume(): boolean {
    if (!this.shownEngine().stopped) return false;
    this.hold = false;
    this.engine = { ...this.engine, stopped: false };
    this.emit();
    return true;
  }

  dropSession(sessionId: string): void {
    if (!safeKey(sessionId)) return;
    this.sessions.delete(sessionId);
    this.emit();
  }

  private shownEngine(): Engine {
    if (this.hold === null) return this.engine;
    return { ...this.engine, stopped: this.hold };
  }

  private catchUp(hostStopped: boolean): void {
    if (this.hold !== null && this.hold === hostStopped) this.hold = null;
  }

  private emit(): void {
    this.cache.clear();
    for (const listener of this.listeners) listener();
  }
}

export const sharedTimeline = new TimelineStore();

/** Chat approval and handoff sit between the breaker and the restart banner. */
export function shownPhase(view: PanelView, waiting: boolean, handoff: boolean): ShownPhase {
  if (view.phase === "stopped" || view.phase === "tripped") return view.phase;
  if (waiting) return "waiting";
  if (handoff) return "handoff";
  return view.phase;
}
