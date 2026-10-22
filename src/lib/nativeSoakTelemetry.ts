import type { StreamQueueDiagnostics, ToolQueueDiagnostics } from "./streamCoalesce";

type Observable<T> = { observeDiagnostics: (notify: (value: T) => void) => () => void };
const emptyStream = (): StreamQueueDiagnostics => ({
  receivedChunks: 0, flushedChunks: 0, pendingKeys: 0, pendingTextCodeUnits: 0,
  peakPendingKeys: 0, peakPendingTextCodeUnits: 0,
});
const emptyTools = (): ToolQueueDiagnostics => ({
  receivedEvents: 0, flushedEvents: 0, pendingEvents: 0, peakPendingEvents: 0,
});

/** Owns only the two queues registered by this Host subscription, not generic
 * StreamCoalescer/TimedBatchQueue instances elsewhere. Registration is inert
 * until the native opt-in probe succeeds. Storage is constant-size.
 */
export class NativeSoakQueues {
  private active = false;
  private epoch = 0;
  private stream?: { queue: Observable<StreamQueueDiagnostics> };
  private tools?: { queue: Observable<ToolQueueDiagnostics> };
  private stopStream?: () => void;
  private stopTools?: () => void;
  private streamValue = emptyStream();
  private toolsValue = emptyTools();

  attachStream(queue: Observable<StreamQueueDiagnostics>): () => void {
    this.stopStream?.();
    this.streamValue.pendingKeys = 0;
    this.streamValue.pendingTextCodeUnits = 0;
    const owner = { queue };
    this.stream = owner;
    this.watchStream();
    return () => {
      if (this.stream !== owner) return;
      this.stopStream?.();
      this.stopStream = undefined;
      this.stream = undefined;
      this.streamValue.pendingKeys = 0;
      this.streamValue.pendingTextCodeUnits = 0;
    };
  }

  attachTools(queue: Observable<ToolQueueDiagnostics>): () => void {
    this.stopTools?.();
    this.toolsValue.pendingEvents = 0;
    const owner = { queue };
    this.tools = owner;
    this.watchTools();
    return () => {
      if (this.tools !== owner) return;
      this.stopTools?.();
      this.stopTools = undefined;
      this.tools = undefined;
      this.toolsValue.pendingEvents = 0;
    };
  }

  private watchStream(): void {
    if (!this.active || !this.stream) return;
    let received = 0;
    let flushed = 0;
    this.stopStream = this.stream.queue.observeDiagnostics((value) => {
      this.streamValue.receivedChunks += value.receivedChunks - received;
      this.streamValue.flushedChunks += value.flushedChunks - flushed;
      received = value.receivedChunks;
      flushed = value.flushedChunks;
      this.streamValue.pendingKeys = value.pendingKeys;
      this.streamValue.pendingTextCodeUnits = value.pendingTextCodeUnits;
      this.streamValue.peakPendingKeys = Math.max(this.streamValue.peakPendingKeys, value.peakPendingKeys);
      this.streamValue.peakPendingTextCodeUnits = Math.max(
        this.streamValue.peakPendingTextCodeUnits, value.peakPendingTextCodeUnits,
      );
    });
  }

  private watchTools(): void {
    if (!this.active || !this.tools) return;
    let received = 0;
    let flushed = 0;
    this.stopTools = this.tools.queue.observeDiagnostics((value) => {
      this.toolsValue.receivedEvents += value.receivedEvents - received;
      this.toolsValue.flushedEvents += value.flushedEvents - flushed;
      received = value.receivedEvents;
      flushed = value.flushedEvents;
      this.toolsValue.pendingEvents = value.pendingEvents;
      this.toolsValue.peakPendingEvents = Math.max(this.toolsValue.peakPendingEvents, value.peakPendingEvents);
    });
  }

  start(): () => void {
    this.stop();
    const epoch = ++this.epoch;
    this.streamValue = emptyStream();
    this.toolsValue = emptyTools();
    this.active = true;
    this.watchStream();
    this.watchTools();
    return () => { if (this.epoch === epoch) this.stop(); };
  }

  private stop(): void {
    this.active = false;
    this.stopStream?.();
    this.stopTools?.();
    this.stopStream = undefined;
    this.stopTools = undefined;
  }

  snapshot(): { stream: StreamQueueDiagnostics; tools: ToolQueueDiagnostics } {
    return { stream: { ...this.streamValue }, tools: { ...this.toolsValue } };
  }
}

export type NativeSoakSample = {
  version: 1;
  sequence: number;
  elapsedMs: number;
  intervalMs: number;
  timerLagMs: number;
  frameCount: number;
  maxFrameGapMs: number;
  documentVisible: boolean;
  stream: StreamQueueDiagnostics;
  tools: ToolQueueDiagnostics;
  previousWriteFailures: number;
};

export type NativeSoakRuntime = {
  now: () => number;
  timer: (callback: () => void, delay: number) => () => void;
  frame: (callback: (at: number) => void) => () => void;
  visible: () => boolean;
  onVisibility: (callback: () => void) => () => void;
};

const browserRuntime: NativeSoakRuntime = {
  now: () => performance.now(),
  timer: (callback, delay) => {
    const id = window.setTimeout(callback, delay);
    return () => window.clearTimeout(id);
  },
  frame: (callback) => {
    const id = window.requestAnimationFrame(callback);
    return () => window.cancelAnimationFrame(id);
  },
  visible: () => document.visibilityState === "visible",
  onVisibility: (callback) => {
    document.addEventListener("visibilitychange", callback);
    return () => document.removeEventListener("visibilitychange", callback);
  },
};

const SAMPLE_INTERVAL_MS = 30_000;
// One write per WebView, even if a previous mount's IPC has not settled. A
// hung writer loses samples (visible as interval gaps), never creates a queue.
let writeInFlight = false;

/** Starts only after the native opt-in check. All queue counters, peaks,
 * frameCount, maxFrameGapMs and previousWriteFailures are cumulative for this
 * activation. pending* are current queue gauges. intervalMs is time since the
 * preceding attempted sample (or activation); timerLagMs is lateness against
 * this timer's scheduled deadline, NOT a render heartbeat. Visible RAF gaps
 * exclude every hidden interval. No payloads/IDs are read or retained here.
 */
export function startNativeSoakTelemetry(
  queues: NativeSoakQueues,
  write: (sample: NativeSoakSample) => Promise<void>,
  runtime: NativeSoakRuntime = browserRuntime,
): () => void {
  const stopQueues = queues.start();
  const started = runtime.now();
  let lastSample = started;
  let sequence = 0;
  let frames = 0;
  let previousFrame: number | null = null;
  let maxFrameGap = 0;
  let failures = 0;
  let stopped = false;
  let cancelFrame: (() => void) | undefined;
  let cancelTimer: (() => void) | undefined;

  const scheduleFrame = () => {
    if (stopped || !runtime.visible()) return;
    cancelFrame = runtime.frame((at) => {
      cancelFrame = undefined;
      if (stopped || !runtime.visible()) { previousFrame = null; return; }
      frames += 1;
      if (previousFrame !== null) maxFrameGap = Math.max(maxFrameGap, at - previousFrame);
      previousFrame = at;
      scheduleFrame();
    });
  };
  const stopVisibility = runtime.onVisibility(() => {
    cancelFrame?.();
    cancelFrame = undefined;
    previousFrame = null;
    scheduleFrame();
  });

  const scheduleSample = () => {
    const due = runtime.now() + SAMPLE_INTERVAL_MS;
    cancelTimer = runtime.timer(() => {
      if (stopped) return;
      const at = runtime.now();
      if (!writeInFlight) {
        const sample: NativeSoakSample = {
          version: 1, sequence: ++sequence, elapsedMs: at - started,
          intervalMs: at - lastSample, timerLagMs: Math.max(0, at - due),
          frameCount: frames, maxFrameGapMs: maxFrameGap,
          documentVisible: runtime.visible(), ...queues.snapshot(),
          previousWriteFailures: failures,
        };
        lastSample = at;
        writeInFlight = true;
        // Normalize sync throws too; telemetry must not escape into UI timers.
        void Promise.resolve().then(() => {
          if (!stopped) return write(sample);
        }).catch(() => { failures += 1; }).finally(() => { writeInFlight = false; });
      }
      scheduleSample();
    }, SAMPLE_INTERVAL_MS);
  };
  scheduleFrame();
  scheduleSample();
  return () => {
    stopped = true;
    cancelTimer?.();
    cancelFrame?.();
    stopVisibility();
    stopQueues();
  };
}
