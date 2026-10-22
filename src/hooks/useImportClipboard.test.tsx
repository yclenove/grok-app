/** @vitest-environment jsdom */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { useImportClipboard } from "./useImportClipboard";

function deferred() {
  let resolve!: () => void;
  let reject!: (cause: unknown) => void;
  const promise = new Promise<void>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

const writeText = vi.fn();
beforeEach(() => {
  writeText.mockReset().mockResolvedValue(undefined);
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
});
afterEach(cleanup);

test("waits for clipboard acknowledgement and blocks duplicate commands", async () => {
  const pending = deferred();
  writeText.mockReturnValueOnce(pending.promise);
  const { result } = renderHook(() => useImportClipboard("reader:one"));
  let first!: Promise<boolean>;
  act(() => { first = result.current.copy("hello"); void result.current.copy("duplicate"); });
  expect(writeText).toHaveBeenCalledTimes(1);
  expect(result.current.status).toBe("pending");
  await act(async () => { pending.resolve(); expect(await first).toBe(true); });
  expect(result.current.status).toBe("copied");
});

test("handles denied clipboard access and allows a retry", async () => {
  writeText.mockRejectedValueOnce(new Error("denied"));
  const { result } = renderHook(() => useImportClipboard("reader:one"));
  await act(async () => { expect(await result.current.copy("hello")).toBe(false); });
  expect(result.current.status).toBe("failed");
  await act(async () => { await result.current.copy("retry"); });
  expect(result.current.status).toBe("copied");
});

test("late success cannot claim a different reader was copied", async () => {
  const pending = deferred();
  writeText.mockReturnValueOnce(pending.promise);
  const { result, rerender } = renderHook(({ owner }) => useImportClipboard(owner), { initialProps: { owner: "one" } });
  let first!: Promise<boolean>;
  act(() => { first = result.current.copy("old text"); });
  rerender({ owner: "two" });
  expect(result.current.status).toBe("idle");
  await act(async () => { pending.resolve(); await first; });
  expect(result.current.status).toBe("idle");
});

test("late rejection does not clear the new reader's pending operation", async () => {
  const old = deferred();
  const current = deferred();
  writeText.mockReturnValueOnce(old.promise).mockReturnValueOnce(current.promise);
  const { result, rerender } = renderHook(({ owner }) => useImportClipboard(owner), { initialProps: { owner: "one" } });
  let first!: Promise<boolean>;
  let second!: Promise<boolean>;
  act(() => { first = result.current.copy("old"); });
  rerender({ owner: "two" });
  act(() => { second = result.current.copy("current"); });
  await act(async () => { old.reject(new Error("old denial")); await first; });
  expect(result.current.status).toBe("pending");
  await act(async () => { current.resolve(); await second; });
  expect(result.current.status).toBe("copied");
});

test("missing clipboard support is a handled error, not an unhandled rejection", async () => {
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: undefined });
  const { result } = renderHook(() => useImportClipboard("reader:one"));
  await act(async () => { expect(await result.current.copy("hello")).toBe(false); });
  expect(result.current.status).toBe("failed");
});

test("an unmounted owner cannot issue more clipboard writes", async () => {
  const { result, unmount } = renderHook(() => useImportClipboard("reader:one"));
  const copy = result.current.copy;
  unmount();
  expect(await copy("stale")).toBe(false);
  expect(writeText).not.toHaveBeenCalled();
});
