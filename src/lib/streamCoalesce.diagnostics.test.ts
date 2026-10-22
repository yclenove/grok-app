import { afterEach, describe, expect, it, vi } from "vitest";
import { StreamCoalescer, TimedBatchQueue, type StreamQueueDiagnostics, type ToolQueueDiagnostics } from "./streamCoalesce";

afterEach(() => vi.useRealTimers());

describe("opt-in stream queue diagnostics", () => {
  it("counts original pushes, merged callback attempts and UTF16 pending text without payloads", () => {
    vi.useFakeTimers();
    const flush = vi.fn();
    const samples: StreamQueueDiagnostics[] = [];
    const queue = new StreamCoalescer({ onFlush: flush, flushMs: 40 });
    queue.observeDiagnostics((sample) => samples.push(sample));
    queue.push({ sessionId: "secret-session", messageId: "secret-id", text: "密😀" });
    queue.push({ sessionId: "secret-session", messageId: "secret-id", text: "语" });
    expect(samples.at(-1)).toEqual({ receivedChunks: 2, flushedChunks: 0,
      pendingKeys: 1, pendingTextCodeUnits: 4, peakPendingKeys: 1, peakPendingTextCodeUnits: 4 });
    vi.advanceTimersByTime(40);
    expect(flush).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ text: "密😀语" }));
    expect(samples.at(-1)).toMatchObject({ receivedChunks: 2, flushedChunks: 1, pendingKeys: 0, pendingTextCodeUnits: 0 });
    expect(JSON.stringify(samples)).not.toMatch(/secret|密|语|😀/);
    queue.dispose();
  });

  it("late subscription measures old pending work without inventing earlier received counts", () => {
    const queue = new StreamCoalescer({ onFlush: vi.fn() });
    queue.push({ text: "😀" });
    let latest: StreamQueueDiagnostics | undefined;
    const stop = queue.observeDiagnostics((sample) => { latest = sample; });
    expect(latest).toMatchObject({ receivedChunks: 0, pendingKeys: 1, pendingTextCodeUnits: 2 });
    queue.flushAll();
    expect(latest).toMatchObject({ receivedChunks: 0, flushedChunks: 1, pendingKeys: 0 });
    stop();
    queue.push({ text: "not-observed", done: true });
    expect(latest?.flushedChunks).toBe(1);
    queue.dispose();
  });

  it("keeps cross-kind terminal ordering and independent keys", () => {
    const delivered: string[] = [];
    const queue = new StreamCoalescer({ onFlush: (c) => delivered.push(c.text ?? "") });
    let latest: StreamQueueDiagnostics | undefined;
    queue.observeDiagnostics((sample) => { latest = sample; });
    queue.push({ sessionId: "a", text: "thought", kind: "thought" });
    queue.push({ sessionId: "b", text: "other" });
    queue.push({ sessionId: "a", text: "answer" });
    queue.push({ sessionId: "a", text: "!", done: true });
    expect(delivered).toEqual(["thought", "answer!"]);
    expect(latest).toMatchObject({ receivedChunks: 4, flushedChunks: 2, pendingKeys: 1,
      pendingTextCodeUnits: 5, peakPendingKeys: 3 });
    queue.dispose();
    expect(delivered).toEqual(["thought", "answer!", "other"]);
  });

  it.each([{ thoughtPhase: "new" }, { thoughtPhase: "open" }, { text: "" }, { done: true }])(
    "keeps immediate-flush semantics for %j", (boundary) => {
      const flush = vi.fn();
      const queue = new StreamCoalescer({ onFlush: flush });
      const observer = vi.fn(() => { throw new Error("observer must be isolated"); });
      queue.observeDiagnostics(observer);
      queue.push(boundary);
      expect(flush).toHaveBeenCalledTimes(1);
      queue.dispose();
    },
  );

  it("preserves reentrant trailing flush and does not expose mutable counters", () => {
    vi.useFakeTimers();
    const delivered: string[] = [];
    const queue = new StreamCoalescer({ flushMs: 40, onFlush: (c) => {
      delivered.push(c.text ?? "");
      if (c.text === "first") queue.push({ text: "second" });
    } });
    const samples: StreamQueueDiagnostics[] = [];
    queue.observeDiagnostics((value) => { samples.push({ ...value }); value.receivedChunks = -10; });
    queue.push({ text: "first" });
    vi.advanceTimersByTime(80);
    expect(delivered).toEqual(["first", "second"]);
    expect(samples.at(-1)).toMatchObject({ receivedChunks: 2, flushedChunks: 2, pendingKeys: 0 });
    queue.dispose();
  });

  it("preserves original thrown callback and disposal failure behavior", () => {
    const error = new Error("business failure");
    const flush = vi.fn<() => void>(() => { throw error; });
    const queue = new StreamCoalescer({ onFlush: flush });
    const samples: StreamQueueDiagnostics[] = [];
    queue.observeDiagnostics((sample) => samples.push(sample));
    queue.push({ text: "one", sessionId: "a" });
    queue.push({ text: "two", sessionId: "b" });
    expect(() => queue.dispose()).toThrow(error);
    expect(flush).toHaveBeenCalledTimes(1);
    expect(samples.at(-1)).toMatchObject({ flushedChunks: 1, pendingKeys: 0, pendingTextCodeUnits: 0 });
    flush.mockImplementation(() => undefined);
    queue.push({ text: "still open", done: true });
    expect(flush).toHaveBeenCalledTimes(2);
    queue.dispose();
    queue.push({ text: "ignored", done: true });
    expect(flush).toHaveBeenCalledTimes(2);
  });

  it("old unsubscribe cannot remove a replacement observer", () => {
    const queue = new StreamCoalescer({ onFlush: vi.fn() });
    const stop = queue.observeDiagnostics(vi.fn());
    const observer = vi.fn();
    queue.observeDiagnostics(observer);
    stop();
    queue.push({ text: "x" });
    expect(observer).toHaveBeenLastCalledWith(expect.objectContaining({ receivedChunks: 1 }));
    queue.dispose();
  });
});

describe("opt-in tool queue diagnostics", () => {
  it("counts ordered events and includes terminal events in the peak", () => {
    const flush = vi.fn();
    const queue = new TimedBatchQueue<{ secret: string; done?: boolean }>({
      onFlush: flush, shouldFlushImmediate: (event) => !!event.done,
    });
    const samples: ToolQueueDiagnostics[] = [];
    queue.observeDiagnostics((sample) => samples.push(sample));
    queue.push({ secret: "never leak first" });
    queue.push({ secret: "never leak second", done: true });
    expect(flush.mock.calls[0][0]).toEqual([{ secret: "never leak first" }, { secret: "never leak second", done: true }]);
    expect(samples.at(-1)).toEqual({ receivedEvents: 2, flushedEvents: 2, pendingEvents: 0, peakPendingEvents: 2 });
    expect(JSON.stringify(samples)).not.toContain("secret");
    queue.dispose();
  });

  it("preserves reentrant order, timer and dispose", () => {
    vi.useFakeTimers();
    const out: number[] = [];
    const queue = new TimedBatchQueue<number>({ flushMs: 40, onFlush: (items) => {
      out.push(...items);
      if (items[0] === 1) queue.push(2);
    } });
    const observer = vi.fn(() => { throw new Error("ignored observer error"); });
    queue.observeDiagnostics(observer);
    queue.push(1);
    vi.advanceTimersByTime(40);
    queue.dispose();
    queue.push(3);
    vi.advanceTimersByTime(80);
    expect(out).toEqual([1, 2]);
    expect(observer).toHaveBeenLastCalledWith({ receivedEvents: 2, flushedEvents: 2, pendingEvents: 0, peakPendingEvents: 1 });
  });

  it("retains pending items when the immediate predicate throws", () => {
    const error = new Error("predicate failure");
    const flush = vi.fn();
    const queue = new TimedBatchQueue<number>({ onFlush: flush, shouldFlushImmediate: () => { throw error; } });
    const observer = vi.fn();
    queue.observeDiagnostics(observer);
    expect(() => queue.push(7)).toThrow(error);
    expect(observer).toHaveBeenLastCalledWith({ receivedEvents: 1, flushedEvents: 0, pendingEvents: 1, peakPendingEvents: 1 });
    queue.dispose();
    expect(flush).toHaveBeenCalledExactlyOnceWith([7]);
  });

  it("preserves callback throws, late subscription and observer replacement", () => {
    const error = new Error("flush failure");
    const queue = new TimedBatchQueue<number>({ onFlush: () => { throw error; } });
    queue.push(1);
    const oldStop = queue.observeDiagnostics(vi.fn());
    const observer = vi.fn();
    const stop = queue.observeDiagnostics(observer);
    oldStop();
    expect(() => queue.flushAll()).toThrow(error);
    expect(observer).toHaveBeenLastCalledWith({ receivedEvents: 0, flushedEvents: 1, pendingEvents: 0, peakPendingEvents: 1 });
    stop();
    const calls = observer.mock.calls.length;
    queue.dispose();
    expect(observer).toHaveBeenCalledTimes(calls);
  });
});
