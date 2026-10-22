/**
 * Merge high-frequency stream chunks before React setState.
 * Same session + messageId + kind text is concatenated; terminal `done`
 * and thought phase boundaries flush immediately.
 *
 * Adaptive flushMs targets fewer setMessages/sec on lower-core (Intel) machines
 * without delaying turn-end honesty (`done` / phase boundaries stay immediate).
 */

export type CoalesceStreamChunk = {
  sessionId?: string | null;
  messageId?: string | null;
  text?: string | null;
  done?: boolean | null;
  hostFinalized?: boolean | null;
  kind?: string | null;
  thoughtPhase?: string | null;
};

/**
 * Default stream coalesce interval (ms). Used when core count is mid-range
 * or when the caller does not pass an explicit flushMs.
 * Lowered for smoother streaming; still batches high-frequency chunks.
 */
export const STREAM_COALESCE_FLUSH_MS = 88;

/**
 * Pick a stream flush interval from hardware concurrency.
 * Fewer cores → longer batch window (Intel MBP / low-power laptops).
 * More cores → slightly snappier UI updates.
 */
export function resolveStreamFlushMs(
  hardwareConcurrency: number = typeof navigator !== "undefined"
    ? navigator.hardwareConcurrency || 8
    : 8,
): number {
  if (hardwareConcurrency <= 8) return 104;
  if (hardwareConcurrency <= 12) return STREAM_COALESCE_FLUSH_MS;
  return 60;
}

/** Stable key for mergeable stream rows. */
export function streamCoalesceKey(chunk: CoalesceStreamChunk): string {
  const sid = chunk.sessionId ?? "";
  const mid = chunk.messageId ?? "";
  const kind = chunk.kind ?? "assistant";
  return `${sid}\0${mid}\0${kind}`;
}

/** Whether this chunk must not wait in the batch buffer. */
export function streamChunkNeedsImmediateFlush(chunk: CoalesceStreamChunk): boolean {
  if (chunk.done) return true;
  const phase = (chunk.thoughtPhase ?? "").toLowerCase();
  // Phase boundary opens a new thought block — flush prior + this promptly.
  if (phase === "new" || phase === "open") return true;
  return false;
}

/**
 * Merge `next` into `prev` when they share the coalesce key.
 * Returns null when they cannot merge (caller should flush prev first).
 */
export function mergeStreamChunks(
  prev: CoalesceStreamChunk,
  next: CoalesceStreamChunk,
): CoalesceStreamChunk | null {
  if (streamCoalesceKey(prev) !== streamCoalesceKey(next)) return null;
  const text = `${prev.text ?? ""}${next.text ?? ""}`;
  const done = !!(prev.done || next.done);
  const hostFinalized = !!(prev.hostFinalized || next.hostFinalized);
  // Prefer the latest non-empty thought phase (new/open/continue).
  const thoughtPhase =
    next.thoughtPhase && next.thoughtPhase !== "none"
      ? next.thoughtPhase
      : prev.thoughtPhase;
  return {
    ...prev,
    ...next,
    text,
    done,
    hostFinalized,
    thoughtPhase: thoughtPhase ?? next.thoughtPhase ?? prev.thoughtPhase,
  };
}

export type StreamCoalescerOptions = {
  /** Max hold time before a non-terminal batch is flushed (default STREAM_COALESCE_FLUSH_MS). */
  flushMs?: number;
  /** Deliver one (possibly merged) chunk to the UI reducer. */
  onFlush: (chunk: CoalesceStreamChunk) => void;
};

/** Numeric-only, opt-in observations; no keys, identifiers or payloads escape. */
export type StreamQueueDiagnostics = {
  receivedChunks: number;
  /** Coalesced output callbacks attempted, not the original input count. */
  flushedChunks: number;
  pendingKeys: number;
  pendingTextCodeUnits: number;
  peakPendingKeys: number;
  peakPendingTextCodeUnits: number;
};

export type ToolQueueDiagnostics = {
  receivedEvents: number;
  /** Events handed to an output callback, including one that throws. */
  flushedEvents: number;
  pendingEvents: number;
  peakPendingEvents: number;
};

type QueueObserver<T> = { value: T; notify: (value: T) => void };

function notifyQueue<T extends object>(observer: QueueObserver<T> | null): void {
  if (!observer) return;
  try {
    observer.notify({ ...observer.value });
  } catch {
    // Optional diagnostics must never affect batching or callback exceptions.
  }
}

/**
 * Batches stream chunks per key. Call `push` from the Tauri event listener
 * and `dispose` on unmount / cancel.
 */
export class StreamCoalescer {
  private readonly flushMs: number;
  private readonly onFlush: (chunk: CoalesceStreamChunk) => void;
  private pending = new Map<string, CoalesceStreamChunk>();
  private timer: ReturnType<typeof setTimeout> | null = null;
  private disposed = false;
  private diagnostics: QueueObserver<StreamQueueDiagnostics> | null = null;

  constructor(opts: StreamCoalescerOptions) {
    this.flushMs = Math.max(8, opts.flushMs ?? STREAM_COALESCE_FLUSH_MS);
    this.onFlush = opts.onFlush;
  }

  /** Current flush interval (ms). */
  get intervalMs(): number {
    return this.flushMs;
  }

  /** One scoped observer; no counters are collected without a subscriber.
   * Counts start at subscription, but pending includes earlier queued work.
   * Replacing/removing an observer cannot change the queue's lifecycle.
   */
  observeDiagnostics(notify: (value: StreamQueueDiagnostics) => void): () => void {
    let text = 0;
    for (const chunk of this.pending.values()) text += (chunk.text ?? "").length;
    const observer = { notify, value: {
      receivedChunks: 0, flushedChunks: 0, pendingKeys: this.pending.size,
      pendingTextCodeUnits: text, peakPendingKeys: this.pending.size,
      peakPendingTextCodeUnits: text,
    } };
    this.diagnostics = observer;
    notifyQueue(observer);
    return () => { if (this.diagnostics === observer) this.diagnostics = null; };
  }

  private setPending(key: string, chunk: CoalesceStreamChunk): void {
    const d = this.diagnostics?.value;
    if (d) {
      d.pendingTextCodeUnits += (chunk.text ?? "").length - (this.pending.get(key)?.text ?? "").length;
    }
    this.pending.set(key, chunk);
    if (d) {
      d.pendingKeys = this.pending.size;
      d.peakPendingKeys = Math.max(d.peakPendingKeys, d.pendingKeys);
      d.peakPendingTextCodeUnits = Math.max(d.peakPendingTextCodeUnits, d.pendingTextCodeUnits);
    }
  }

  private deletePending(key: string): void {
    const d = this.diagnostics?.value;
    if (d) d.pendingTextCodeUnits -= (this.pending.get(key)?.text ?? "").length;
    this.pending.delete(key);
    if (d) d.pendingKeys = this.pending.size;
  }

  private flushChunk(chunk: CoalesceStreamChunk): void {
    if (this.diagnostics) this.diagnostics.value.flushedChunks += 1;
    this.onFlush(chunk);
  }

  push(chunk: CoalesceStreamChunk): void {
    if (this.disposed) return;
    if (this.diagnostics) this.diagnostics.value.receivedChunks += 1;
    try {
      this.pushAccepted(chunk);
    } finally {
      notifyQueue(this.diagnostics);
    }
  }

  private pushAccepted(chunk: CoalesceStreamChunk): void {
    // Thought and assistant use different keys. A done tick must not settle
    // the bubble while the other kind is still buffered — that left CoT on
    // screen as the "final" reply until remount.
    if (chunk.done) {
      const sid = chunk.sessionId ?? "";
      const selfKey = streamCoalesceKey(chunk);
      const others: CoalesceStreamChunk[] = [];
      for (const [k, v] of this.pending) {
        if (k === selfKey) continue;
        if ((v.sessionId ?? "") === sid) {
          others.push(v);
          this.deletePending(k);
        }
      }
      for (const v of others) this.flushChunk(v);
    }
    const key = streamCoalesceKey(chunk);
    const existing = this.pending.get(key);
    if (existing) {
      const merged = mergeStreamChunks(existing, chunk);
      if (merged) {
        if (streamChunkNeedsImmediateFlush(merged)) {
          this.deletePending(key);
          this.flushChunk(merged);
          this.armOrClear();
          return;
        }
        this.setPending(key, merged);
        this.armOrClear();
        return;
      }
      // Different shape (shouldn't for same key) — flush old.
      this.deletePending(key);
      this.flushChunk(existing);
    }

    if (streamChunkNeedsImmediateFlush(chunk) || !(chunk.text ?? "")) {
      this.flushChunk(chunk);
      this.armOrClear();
      return;
    }

    this.setPending(key, chunk);
    this.armOrClear();
  }

  /** Flush all pending immediately (turn end / unmount). */
  flushAll(): void {
    if (this.timer != null) {
      clearTimeout(this.timer);
      this.timer = null;
    }
    const items = [...this.pending.values()];
    this.pending.clear();
    if (this.diagnostics) {
      this.diagnostics.value.pendingKeys = 0;
      this.diagnostics.value.pendingTextCodeUnits = 0;
    }
    try {
      for (const c of items) this.flushChunk(c);
    } finally {
      notifyQueue(this.diagnostics);
    }
  }

  dispose(): void {
    this.flushAll();
    this.disposed = true;
    this.diagnostics = null;
  }

  private armOrClear(): void {
    if (this.pending.size === 0) {
      if (this.timer != null) {
        clearTimeout(this.timer);
        this.timer = null;
      }
      return;
    }
    if (this.timer != null) return;
    this.timer = setTimeout(() => {
      this.timer = null;
      this.flushAll();
      // Trailing re-arm: if onFlush callbacks pushed more chunks, schedule again.
      this.armOrClear();
    }, this.flushMs);
  }
}

// ─── High-frequency non-stream batching (tool progress / detail) ────────────

export type TimedBatchQueueOptions<T> = {
  /** Max hold time before a non-terminal batch is flushed. */
  flushMs?: number;
  /** Deliver the ordered batch to the UI (single setState preferred). */
  onFlush: (items: T[]) => void;
  /** When true, flush immediately including this item (terminal statuses). */
  shouldFlushImmediate?: (item: T) => boolean;
};

/**
 * Order-preserving timer batch for heterogeneous high-frequency events
 * (e.g. `session://tool` progress/detail). Unlike {@link StreamCoalescer},
 * items are not text-merged — they are applied in sequence in one flush.
 */
export class TimedBatchQueue<T> {
  private readonly flushMs: number;
  private readonly onFlush: (items: T[]) => void;
  private readonly shouldFlushImmediate?: (item: T) => boolean;
  private pending: T[] = [];
  private timer: ReturnType<typeof setTimeout> | null = null;
  private disposed = false;
  private diagnostics: QueueObserver<ToolQueueDiagnostics> | null = null;

  constructor(opts: TimedBatchQueueOptions<T>) {
    this.flushMs = Math.max(8, opts.flushMs ?? STREAM_COALESCE_FLUSH_MS);
    this.onFlush = opts.onFlush;
    this.shouldFlushImmediate = opts.shouldFlushImmediate;
  }

  /** Same late-subscription/throw semantics as StreamCoalescer diagnostics. */
  observeDiagnostics(notify: (value: ToolQueueDiagnostics) => void): () => void {
    const observer = { notify, value: {
      receivedEvents: 0, flushedEvents: 0, pendingEvents: this.pending.length,
      peakPendingEvents: this.pending.length,
    } };
    this.diagnostics = observer;
    notifyQueue(observer);
    return () => { if (this.diagnostics === observer) this.diagnostics = null; };
  }

  push(item: T): void {
    if (this.disposed) return;
    this.pending.push(item);
    if (this.diagnostics) {
      const d = this.diagnostics.value;
      d.receivedEvents += 1;
      d.pendingEvents = this.pending.length;
      d.peakPendingEvents = Math.max(d.peakPendingEvents, d.pendingEvents);
    }
    try {
      if (this.shouldFlushImmediate?.(item)) {
        this.flushAll();
        return;
      }
      this.armOrClear();
    } finally {
      notifyQueue(this.diagnostics);
    }
  }

  flushAll(): void {
    if (this.timer != null) {
      clearTimeout(this.timer);
      this.timer = null;
    }
    if (this.pending.length === 0) return;
    const items = this.pending;
    this.pending = [];
    if (this.diagnostics) {
      this.diagnostics.value.pendingEvents = 0;
      this.diagnostics.value.flushedEvents += items.length;
    }
    try {
      this.onFlush(items);
    } finally {
      notifyQueue(this.diagnostics);
    }
  }

  dispose(): void {
    this.flushAll();
    this.disposed = true;
    this.diagnostics = null;
  }

  private armOrClear(): void {
    if (this.pending.length === 0) {
      if (this.timer != null) {
        clearTimeout(this.timer);
        this.timer = null;
      }
      return;
    }
    if (this.timer != null) return;
    this.timer = setTimeout(() => {
      this.timer = null;
      this.flushAll();
      // Trailing re-arm if flush callbacks enqueued more work.
      this.armOrClear();
    }, this.flushMs);
  }
}

/** Terminal tool statuses that must not wait in the tool batch buffer. */
export function toolEventNeedsImmediateFlush(status?: string | null): boolean {
  const s = (status ?? "").toLowerCase();
  return (
    s === "completed" ||
    s === "failed" ||
    s === "error" ||
    s === "cancelled" ||
    s === "canceled" ||
    s === "done"
  );
}
