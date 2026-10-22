import { describe, expect, it, vi } from "vitest";
import { NativeSoakQueues, startNativeSoakTelemetry, type NativeSoakRuntime, type NativeSoakSample } from "./nativeSoakTelemetry";
import { StreamCoalescer, TimedBatchQueue } from "./streamCoalesce";

class Runtime implements NativeSoakRuntime {
  at = 0;
  isVisible = true;
  timerCallback?: () => void;
  frameCallback?: (at: number) => void;
  visibilityCallback?: () => void;
  now = () => this.at;
  visible = () => this.isVisible;
  timer = (callback: () => void, delay: number) => {
    expect(delay).toBe(30_000);
    this.timerCallback = callback;
    return () => { if (this.timerCallback === callback) this.timerCallback = undefined; };
  };
  frame = (callback: (at: number) => void) => {
    this.frameCallback = callback;
    return () => { if (this.frameCallback === callback) this.frameCallback = undefined; };
  };
  onVisibility = (callback: () => void) => {
    this.visibilityCallback = callback;
    return () => { if (this.visibilityCallback === callback) this.visibilityCallback = undefined; };
  };
  tick(at: number) {
    this.at = at;
    const callback = this.timerCallback;
    this.timerCallback = undefined;
    callback?.();
  }
  draw(at: number) {
    this.at = at;
    const callback = this.frameCallback;
    this.frameCallback = undefined;
    callback?.(at);
  }
  visibility(visible: boolean, at: number) {
    this.at = at;
    this.isVisible = visible;
    this.visibilityCallback?.();
  }
}

async function settled() { for (let i = 0; i < 8; i += 1) await Promise.resolve(); }

describe("native soak queue ownership", () => {
  it("does not subscribe or collect when disabled", () => {
    const queue = { observeDiagnostics: vi.fn(() => vi.fn()) };
    const queues = new NativeSoakQueues();
    queues.attachStream(queue);
    queues.attachTools(queue);
    expect(queue.observeDiagnostics).not.toHaveBeenCalled();
    expect(queues.snapshot().stream.receivedChunks).toBe(0);
  });

  it("only counts owned queues and accumulates across replacements", () => {
    const queues = new NativeSoakQueues();
    const first = new StreamCoalescer({ onFlush: vi.fn() });
    const second = new StreamCoalescer({ onFlush: vi.fn() });
    const unrelated = new StreamCoalescer({ onFlush: vi.fn() });
    const tools = new TimedBatchQueue<string>({ onFlush: vi.fn() });
    const removeFirst = queues.attachStream(first);
    queues.attachTools(tools);
    first.push({ text: "before" });
    const stop = queues.start();
    first.flushAll();
    first.push({ text: "first", done: true });
    unrelated.push({ text: "unrelated", done: true });
    tools.push("secret tool payload");
    tools.flushAll();
    queues.attachStream(second);
    removeFirst();
    second.push({ text: "second", done: true });
    expect(queues.snapshot().stream).toMatchObject({ receivedChunks: 2, flushedChunks: 3, peakPendingTextCodeUnits: 6 });
    expect(queues.snapshot().tools).toEqual({ receivedEvents: 1, flushedEvents: 1, pendingEvents: 0, peakPendingEvents: 1 });
    stop();
    second.push({ text: "not counted", done: true });
    expect(queues.snapshot().stream.receivedChunks).toBe(2);
    for (const queue of [first, second, unrelated, tools]) queue.dispose();
  });

  it("old activation cleanup cannot stop a newer activation", () => {
    const queues = new NativeSoakQueues();
    const queue = new StreamCoalescer({ onFlush: vi.fn() });
    queues.attachStream(queue);
    const oldStop = queues.start();
    const stop = queues.start();
    oldStop();
    queue.push({ text: "active", done: true });
    expect(queues.snapshot().stream.receivedChunks).toBe(1);
    stop();
    queue.dispose();
  });

  it("detach clears gauges without erasing counters and works when attached after enable", () => {
    const queues = new NativeSoakQueues();
    const stop = queues.start();
    const queue = new TimedBatchQueue<string>({ onFlush: vi.fn() });
    const detach = queues.attachTools(queue);
    queue.push("x");
    detach();
    queue.dispose();
    expect(queues.snapshot().tools).toEqual({ receivedEvents: 1, flushedEvents: 0, pendingEvents: 0, peakPendingEvents: 1 });
    stop();
  });
});

describe("native soak sampler", () => {
  it("reports visible RAF separately from timer lag with numeric-only exact contract", async () => {
    const runtime = new Runtime();
    const samples: NativeSoakSample[] = [];
    const stop = startNativeSoakTelemetry(new NativeSoakQueues(), async (sample) => { samples.push(sample); }, runtime);
    runtime.draw(10);
    runtime.draw(26);
    runtime.draw(76);
    runtime.tick(30_125);
    await settled();
    expect(samples[0]).toEqual({ version: 1, sequence: 1, elapsedMs: 30_125, intervalMs: 30_125,
      timerLagMs: 125, frameCount: 3, maxFrameGapMs: 50, documentVisible: true,
      stream: { receivedChunks: 0, flushedChunks: 0, pendingKeys: 0, pendingTextCodeUnits: 0, peakPendingKeys: 0, peakPendingTextCodeUnits: 0 },
      tools: { receivedEvents: 0, flushedEvents: 0, pendingEvents: 0, peakPendingEvents: 0 }, previousWriteFailures: 0 });
    runtime.draw(30_140);
    runtime.tick(60_125);
    await settled();
    expect(samples[1]).toMatchObject({ sequence: 2, intervalMs: 30_000, timerLagMs: 0, frameCount: 4, maxFrameGapMs: 30_064 });
    stop();
    expect(runtime.timerCallback).toBeUndefined();
    expect(runtime.frameCallback).toBeUndefined();
    expect(runtime.visibilityCallback).toBeUndefined();
  });

  it("cancels RAF while hidden and never counts hidden duration as a rendered-frame gap", async () => {
    const runtime = new Runtime();
    const samples: NativeSoakSample[] = [];
    const stop = startNativeSoakTelemetry(new NativeSoakQueues(), async (sample) => { samples.push(sample); }, runtime);
    runtime.draw(10);
    runtime.draw(26);
    runtime.visibility(false, 100);
    expect(runtime.frameCallback).toBeUndefined();
    runtime.tick(30_000);
    await settled();
    expect(samples[0]).toMatchObject({ frameCount: 2, maxFrameGapMs: 16, documentVisible: false });
    runtime.visibility(true, 59_000);
    runtime.draw(59_010);
    runtime.draw(59_030);
    runtime.tick(60_000);
    await settled();
    expect(samples[1]).toMatchObject({ frameCount: 4, maxFrameGapMs: 20, documentVisible: true });
    stop();
  });

  it("has no catch-up backlog or overlapping writes and reports the missed interval", async () => {
    const runtime = new Runtime();
    let resolve!: () => void;
    const deferred = new Promise<void>((done) => { resolve = done; });
    const write = vi.fn<(sample: NativeSoakSample) => Promise<void>>().mockReturnValueOnce(deferred).mockResolvedValue(undefined);
    const stop = startNativeSoakTelemetry(new NativeSoakQueues(), write, runtime);
    runtime.tick(30_000);
    await settled();
    runtime.tick(60_000);
    runtime.tick(90_000);
    await settled();
    expect(write).toHaveBeenCalledTimes(1);
    resolve();
    await settled();
    expect(write).toHaveBeenCalledTimes(1);
    runtime.tick(120_000);
    await settled();
    expect(write).toHaveBeenCalledTimes(2);
    expect(write.mock.calls[1][0]).toMatchObject({ sequence: 2, intervalMs: 90_000, timerLagMs: 0 });
    stop();
  });

  it("keeps single-flight across unmount/remount and drops late timer callbacks", async () => {
    const first = new Runtime();
    const second = new Runtime();
    let resolve!: () => void;
    const write = vi.fn<(sample: NativeSoakSample) => Promise<void>>().mockReturnValueOnce(new Promise<void>((done) => { resolve = done; })).mockResolvedValue(undefined);
    const stopFirst = startNativeSoakTelemetry(new NativeSoakQueues(), write, first);
    const staleTimer = first.timerCallback;
    first.tick(30_000);
    await settled();
    stopFirst();
    staleTimer?.();
    const stopSecond = startNativeSoakTelemetry(new NativeSoakQueues(), write, second);
    second.tick(30_000);
    await settled();
    expect(write).toHaveBeenCalledTimes(1);
    resolve();
    await settled();
    second.tick(60_000);
    await settled();
    expect(write).toHaveBeenCalledTimes(2);
    expect(write.mock.calls[1][0]).toMatchObject({ sequence: 1, intervalMs: 60_000 });
    stopSecond();
  });

  it("records rejected/synchronous writes without throwing or exposing their errors", async () => {
    const runtime = new Runtime();
    const write = vi.fn<(sample: NativeSoakSample) => Promise<void>>()
      .mockRejectedValueOnce(new Error("secret provider value"))
      .mockImplementationOnce(() => { throw new Error("secret sync value"); })
      .mockResolvedValue(undefined);
    const stop = startNativeSoakTelemetry(new NativeSoakQueues(), write, runtime);
    for (const at of [30_000, 60_000, 90_000]) { runtime.tick(at); await settled(); }
    expect(write.mock.calls.map(([sample]) => sample.previousWriteFailures)).toEqual([0, 1, 2]);
    expect(JSON.stringify(write.mock.calls)).not.toContain("secret");
    stop();
  });

  it("does not send a scheduled sample after cleanup before its microtask", async () => {
    const runtime = new Runtime();
    const write = vi.fn<(sample: NativeSoakSample) => Promise<void>>().mockResolvedValue(undefined);
    const stop = startNativeSoakTelemetry(new NativeSoakQueues(), write, runtime);
    runtime.tick(30_000);
    stop();
    await settled();
    expect(write).not.toHaveBeenCalled();
  });
});
