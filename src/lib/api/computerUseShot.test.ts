import { expect, it, vi } from "vitest";
import { sharedShotBook } from "@/lib/computer-use/events";
import { listenComputerUseSessionClosed } from "./computerUseShot";

const host = vi.hoisted(() => ({
  desktop: false,
  handler: null as ((event: { sessionId: string }) => void) | null,
  listen: vi.fn(),
}));

vi.mock("./host", () => ({
  isDesktopHost: () => host.desktop,
  invoke: vi.fn(),
  listen: async (event: string, handler: (event: { sessionId: string }) => void) => {
    host.listen(event);
    host.handler = handler;
    return () => {};
  },
}));

it("keeps desktop-only shared image cleanup active after all visible cards unsubscribe", async () => {
  const first = vi.fn();
  await listenComputerUseSessionClosed(first);
  expect(host.listen).not.toHaveBeenCalled();
  host.desktop = true;
  const stopFirst = await listenComputerUseSessionClosed(first);
  const second = vi.fn();
  const stopSecond = await listenComputerUseSessionClosed(second);
  expect(host.listen).toHaveBeenCalledOnce();
  expect(host.listen).toHaveBeenCalledWith("cu://session-closed");
  for (const [sessionId, id] of [["a", "a-shot"], ["b", "b-shot"]]) {
    sharedShotBook.admit({ sessionId, id, fullPath: "/tmp/shot.png", source: "redacted" });
  }
  host.handler?.({ sessionId: "a" });
  expect(first).toHaveBeenCalledWith("a");
  expect(second).toHaveBeenCalledWith("a");
  expect(sharedShotBook.resolve("a", "a-shot")).toBe("missing");
  expect(sharedShotBook.resolve("b", "b-shot")).not.toBe("missing");
  stopFirst();
  stopSecond();
  host.handler?.({ sessionId: "b" });
  expect(sharedShotBook.resolve("b", "b-shot")).toBe("missing");
  expect(first).toHaveBeenCalledOnce();
  expect(second).toHaveBeenCalledOnce();
});
