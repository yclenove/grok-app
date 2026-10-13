import { beforeEach, expect, it, vi } from "vitest";
const { invoke, isDesktopHost } = vi.hoisted(() => ({ invoke: vi.fn(), isDesktopHost: vi.fn() }));
vi.mock("./host", () => ({ invoke, isDesktopHost }));
vi.mock("@tauri-apps/api/core", () => ({ Channel: class { constructor(public onmessage: unknown) {} } }));
import { computerTaskSubscribe, computerTaskUnsubscribe } from "./computerTaskEvents";

beforeEach(() => { invoke.mockReset(); isDesktopHost.mockReturnValue(true); });

it("passes a dedicated Channel to the original session subscribe command", async () => {
  const callback = vi.fn();
  const owner = { sessionId: "synthetic-app", subscriptionId: "synthetic-subscription", connectionId: "synthetic-connection", hostInstanceId: "synthetic-host" };
  invoke.mockResolvedValueOnce(owner);
  expect(await computerTaskSubscribe(owner.sessionId, callback)).toBe(owner);
  expect(invoke).toHaveBeenCalledExactlyOnceWith("computer_use_task_subscribe", { sessionId: owner.sessionId, events: expect.objectContaining({ onmessage: expect.any(Function) }) });
  const channel = invoke.mock.calls[0][1].events;
  const event = { ...owner, sequence: "1", kind: "retired" };
  invoke.mockResolvedValueOnce(undefined);
  channel.onmessage(event);
  expect(callback).toHaveBeenCalledExactlyOnceWith(event);
  expect(invoke).toHaveBeenLastCalledWith("computer_use_task_acknowledge", { sessionId: owner.sessionId, subscriptionId: owner.subscriptionId, sequence: "1" });
});

it("cancels exactly the observer, without a task decision", async () => {
  invoke.mockResolvedValueOnce(true);
  expect(await computerTaskUnsubscribe("synthetic-app", "synthetic-subscription")).toBe(true);
  expect(invoke).toHaveBeenCalledExactlyOnceWith("computer_use_task_unsubscribe", { sessionId: "synthetic-app", subscriptionId: "synthetic-subscription" });
});

it("browser/mirror cannot start or stop local observations", async () => {
  isDesktopHost.mockReturnValue(false);
  await expect(computerTaskSubscribe("synthetic-app", vi.fn())).rejects.toThrow("local desktop host");
  await expect(computerTaskUnsubscribe("synthetic-app", "synthetic-subscription")).rejects.toThrow("local desktop host");
  expect(invoke).not.toHaveBeenCalled();
});
