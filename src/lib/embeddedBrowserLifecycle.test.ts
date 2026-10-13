import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { BrowserClosePendingError, createEmbeddedBrowserLifecycle } from "./embeddedBrowserLifecycle";

function deferred<T = void>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

const settle = async () => { for (let i = 0; i < 8; i++) await Promise.resolve(); };
beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe("native browser close ownership across React lifetimes", () => {
  it("cancels a StrictMode cleanup before dispatch and closes only the last client", async () => {
    const close = vi.fn(async () => undefined);
    const life = createEmbeddedBrowserLifecycle(close);
    const first = life.retain("A", "First page");
    first.release();
    await vi.advanceTimersByTimeAsync(149);
    const second = life.retain("A", "First page");
    await vi.advanceTimersByTimeAsync(500);
    expect(close).not.toHaveBeenCalled();
    expect(await second.prepare()).toBe(true);
    const third = life.retain("A", "First page");
    second.release(); second.release();
    await vi.advanceTimersByTimeAsync(150);
    expect(close).not.toHaveBeenCalled();
    third.release();
    await vi.advanceTimersByTimeAsync(150);
    expect(close).toHaveBeenCalledExactlyOnceWith("A");
    expect(life.getFailures()).toEqual([]);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("a remount waits for the original close and retains its failure without retrying", async () => {
    const original = deferred();
    const close = vi.fn(() => original.promise);
    const life = createEmbeddedBrowserLifecycle(close);
    life.retain("A", "Download page").release();
    await vi.advanceTimersByTimeAsync(150);
    const remount = life.retain("A", "Download page");
    let admitted = false;
    const ready = remount.prepare().then(() => { admitted = true; });
    const rejected = expect(ready).rejects.toBeInstanceOf(BrowserClosePendingError);
    await settle();
    expect(admitted).toBe(false);
    expect(life.getState("A").phase).toBe("closing");
    original.reject(new Error("original native download remains pending"));
    await rejected;
    expect(life.getState("A").phase).toBe("failed");
    remount.release();
    await vi.advanceTimersByTimeAsync(60_000);
    expect(close).toHaveBeenCalledTimes(1);
    expect(life.getFailures()).toMatchObject([{ label: "A", title: "Download page", phase: "failed" }]);
    expect(admitted).toBe(false);
  });

  it("does not retire an unknown native close merely because time has elapsed", async () => {
    const original = deferred();
    const close = vi.fn(() => original.promise);
    const life = createEmbeddedBrowserLifecycle(close);
    life.retain("A", "A").release();
    await vi.advanceTimersByTimeAsync(600_000);
    expect(life.getState("A").phase).toBe("closing");
    expect(close).toHaveBeenCalledTimes(1);
    original.resolve(); await settle();
    expect(life.getState("A").phase).toBe("idle");
  });

  it("joins concurrent retries and retires only after the authentic close reply", async () => {
    const original = deferred();
    const close = vi.fn().mockRejectedValueOnce(new Error("still downloading"))
      .mockImplementation(() => original.promise);
    const life = createEmbeddedBrowserLifecycle(close);
    life.retain("A", "A").release();
    await vi.advanceTimersByTimeAsync(150);
    const one = life.retryClose("A");
    const two = life.retryClose("A");
    expect(one).toBe(two);
    await settle();
    expect(close).toHaveBeenCalledTimes(2);
    expect(life.getFailures()[0].phase).toBe("closing");
    original.resolve(); await one;
    expect(life.getFailures()).toEqual([]);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("deferring the notice retains native ownership and a new failed retry is visible", async () => {
    const close = vi.fn(async () => { throw new Error("native owner retained"); });
    const life = createEmbeddedBrowserLifecycle(close);
    life.retain("A", "A").release();
    await vi.advanceTimersByTimeAsync(150);
    const first = life.getFailures()[0];
    life.deferFailure("A", first.attempt);
    expect(life.getFailures()).toEqual([]);
    expect(life.getState("A").error).toBe("native owner retained");
    await expect(life.retryClose("A")).rejects.toThrow("native owner retained");
    expect(life.getFailures()[0].attempt).toBe(first.attempt + 1);
    life.deferFailure("A", first.attempt);
    expect(life.getFailures()).toHaveLength(1);
    expect(close).toHaveBeenCalledTimes(2);
  });

  it("joins a late create reply before dispatching close after unmount", async () => {
    const creation = deferred();
    const close = vi.fn(async () => undefined);
    const life = createEmbeddedBrowserLifecycle(close);
    const lease = life.retain("A", "A");
    lease.trackCreate(creation.promise);
    lease.release();
    await vi.advanceTimersByTimeAsync(150);
    expect(close).not.toHaveBeenCalled();
    expect(life.getState("A").phase).toBe("closing");
    creation.resolve(); await settle();
    expect(close).toHaveBeenCalledExactlyOnceWith("A");
    expect(await lease.prepare()).toBe(false);
    expect(life.getState("A").phase).toBe("idle");
  });

  it("a remount can cancel cleanup while the original create is still pending", async () => {
    const creation = deferred();
    const close = vi.fn(async () => undefined);
    const life = createEmbeddedBrowserLifecycle(close);
    const first = life.retain("A", "A");
    first.trackCreate(creation.promise); first.release();
    await vi.advanceTimersByTimeAsync(150);
    const second = life.retain("A", "A");
    const ready = second.prepare();
    creation.resolve();
    expect(await ready).toBe(true);
    expect(close).not.toHaveBeenCalled();
    second.release();
    await vi.advanceTimersByTimeAsync(150);
    expect(close).toHaveBeenCalledExactlyOnceWith("A");
  });

  it("a rejected create cannot leave a removed tab orphaned or poison its cleanup", async () => {
    const creation = deferred();
    const close = vi.fn(async () => undefined);
    const life = createEmbeddedBrowserLifecycle(close);
    const first = life.retain("A", "A");
    const created = first.trackCreate(creation.promise);
    const rejected = expect(created).rejects.toThrow("create failed");
    first.release();
    await vi.advanceTimersByTimeAsync(150);
    creation.reject(new Error("create failed")); await rejected; await settle();
    expect(close).toHaveBeenCalledExactlyOnceWith("A");
    expect(life.getFailures()).toEqual([]);
  });

  it("restores a Host-retained owner after renderer reload without classifying ordinary failures", async () => {
    const close = vi.fn(async () => undefined);
    const life = createEmbeddedBrowserLifecycle(close);
    const lease = life.retain("A", "A");
    expect(life.recoverNativeFailure("A", "network unavailable")).toBe(false);
    expect(life.getState("A").phase).toBe("idle");
    expect(life.recoverNativeFailure("A", "side-browser-close-pending: retry closing the retained view")).toBe(true);
    await expect(lease.prepare()).rejects.toBeInstanceOf(BrowserClosePendingError);
    await life.retryClose("A");
    expect(await lease.prepare()).toBe(true);
    expect(close).toHaveBeenCalledExactlyOnceWith("A");
  });

  it("retiring one failed page does not erase another page's actionable failure", async () => {
    const close = vi.fn().mockRejectedValue(new Error("still owned"));
    const life = createEmbeddedBrowserLifecycle(close);
    life.retain("A", "A").release(); life.retain("B", "B").release();
    await vi.advanceTimersByTimeAsync(150);
    close.mockResolvedValueOnce(undefined);
    await life.retryClose("A");
    expect(life.getFailures()).toMatchObject([{ label: "B", phase: "failed" }]);
    expect(life.getState("B").error).toBe("still owned");
  });
});
