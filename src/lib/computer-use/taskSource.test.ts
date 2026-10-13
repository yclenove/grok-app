import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { ComputerTaskEvent, ComputerTaskSubscription } from "@/lib/api/computerTaskEvents";
import type { ComputerTaskView } from "@/lib/api/computerTask";
import { ComputerTaskSource, EMPTY_TASK_SNAPSHOT, TASK_HEALTH_TIMEOUT_MS } from "./taskSource";

const owner: ComputerTaskSubscription = {
  subscriptionId: "synthetic-subscription", sessionId: "synthetic-app",
  connectionId: "synthetic-connection", hostInstanceId: "synthetic-host",
};
const view: ComputerTaskView = {
  sessionId: owner.sessionId, connectionId: owner.connectionId, taskId: "synthetic-task",
  featureEnabled: false, grants: [], executions: [],
  pickerSurfaces: ["desktop"],
  request: {
    key: { connectionId: owner.connectionId, taskId: "synthetic-task", requestId: "synthetic-request", revision: 1 },
    phase: "pending", purpose: "synthetic request", scopeIntent: "owned fixture", surfaces: ["desktop"],
    capabilities: ["observe"], remainingMs: 60_000,
  },
};

function event(sequence = "1", overrides: Partial<ComputerTaskSubscription> = {}) {
  return { ...owner, ...overrides, sequence, kind: "view" as const, view };
}

function fixture(available = true) {
  const starts: { receive: (event: ComputerTaskEvent) => void; resolve: (owner: ComputerTaskSubscription) => void; reject: (reason: Error) => void }[] = [];
  const start = vi.fn((_session: string, receive: (event: ComputerTaskEvent) => void) => new Promise<ComputerTaskSubscription>((resolve, reject) => { starts.push({ receive, resolve, reject }); }));
  const stop = vi.fn(async () => true);
  const source = new ComputerTaskSource(owner.sessionId, { available: () => available, start, stop });
  return { source, start, stop, starts };
}

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());
async function flush() { await Promise.resolve(); await Promise.resolve(); }

it("two consumers share one stream; pending needs no execution run", async () => {
  const f = fixture();
  const first = f.source.subscribe(vi.fn());
  const second = f.source.subscribe(vi.fn());
  expect(f.start).toHaveBeenCalledTimes(1);
  f.starts[0].resolve(owner);
  await flush();
  f.starts[0].receive(event());
  expect(f.source.getSnapshot()).toEqual({ state: "live", owner, view });
  first();
  expect(f.stop).not.toHaveBeenCalled();
  second();
  expect(f.stop).toHaveBeenCalledExactlyOnceWith(owner.sessionId, owner.subscriptionId);
  expect(f.source.getSnapshot()).toBe(EMPTY_TASK_SNAPSHOT);
});

it("accepts the full latest event arriving before the subscribe ack", async () => {
  const f = fixture();
  const release = f.source.subscribe(vi.fn());
  for (let i = 1; i <= 100; i++) f.starts[0].receive(event(String(i)));
  expect(f.source.getSnapshot().state).toBe("connecting");
  f.starts[0].resolve(owner);
  await flush();
  expect(f.source.getSnapshot().state).toBe("live");
  expect(f.source.getSnapshot().view).toBe(view);
  release();
});

it("StrictMode remount cancels only the old late ack and rejects its late events", async () => {
  const f = fixture();
  f.source.subscribe(vi.fn())();
  const release = f.source.subscribe(vi.fn());
  const newOwner = { ...owner, subscriptionId: "synthetic-new-subscription" };
  f.starts[1].resolve(newOwner);
  await flush();
  f.starts[1].receive(event("1", newOwner));
  f.starts[0].resolve(owner);
  await flush();
  f.starts[0].receive({ ...event("2"), kind: "retired" });
  expect(f.stop).toHaveBeenCalledExactlyOnceWith(owner.sessionId, owner.subscriptionId);
  expect(f.source.getSnapshot().owner).toEqual(newOwner);
  expect(f.source.getSnapshot().state).toBe("live");
  release();
});

it("unmount before ack releases the eventual original worker without reviving UI", async () => {
  const f = fixture();
  const listener = vi.fn();
  f.source.subscribe(listener)();
  const calls = listener.mock.calls.length;
  f.starts[0].receive(event());
  f.starts[0].resolve(owner);
  await flush();
  expect(listener).toHaveBeenCalledTimes(calls);
  expect(f.stop).toHaveBeenCalledExactlyOnceWith(owner.sessionId, owner.subscriptionId);
  expect(f.source.getSnapshot()).toBe(EMPTY_TASK_SNAPSHOT);
});

it.each(["sessionId", "connectionId", "subscriptionId", "hostInstanceId"] as const)("rejects a foreign %s before modifying the snapshot", async (key) => {
  const f = fixture();
  const release = f.source.subscribe(vi.fn());
  f.starts[0].resolve(owner);
  await flush();
  f.starts[0].receive(event());
  const snapshot = f.source.getSnapshot();
  f.starts[0].receive(event("2", { [key]: "foreign" }));
  expect(f.source.getSnapshot()).toBe(snapshot);
  release();
});

it("validates decimal u64 order and rejects duplicate, backward and malformed sequence", async () => {
  const f = fixture();
  const release = f.source.subscribe(vi.fn());
  f.starts[0].resolve(owner);
  await flush();
  f.starts[0].receive(event("9007199254740993"));
  const snapshot = f.source.getSnapshot();
  for (const seq of ["0", "01", "-1", "1e3", "1.1", "NaN", "9007199254740993", "9007199254740992", "18446744073709551616"]) {
    f.starts[0].receive(event(seq));
    expect(f.source.getSnapshot()).toBe(snapshot);
  }
  f.starts[0].receive({ ...event("9007199254740994"), kind: "heartbeat" });
  expect(f.source.getSnapshot()).not.toBe(snapshot);
  release();
});

it("rejects a foreign view or embedded request key despite valid event identity", async () => {
  const f = fixture();
  const release = f.source.subscribe(vi.fn());
  f.starts[0].resolve(owner);
  await flush();
  f.starts[0].receive(event());
  const snapshot = f.source.getSnapshot();
  f.starts[0].receive({ ...event("2"), view: { ...view, connectionId: "foreign" } });
  f.starts[0].receive({ ...event("3"), view: { ...view, request: { ...view.request!, key: { ...view.request!.key, connectionId: "foreign" } } } });
  expect(f.source.getSnapshot()).toBe(snapshot);
  release();
});

it("stale retains the exact last key for Stop; a valid heartbeat recovers health", async () => {
  const f = fixture();
  const release = f.source.subscribe(vi.fn());
  f.starts[0].resolve(owner);
  await flush();
  f.starts[0].receive(event());
  await vi.advanceTimersByTimeAsync(TASK_HEALTH_TIMEOUT_MS);
  expect(f.source.getSnapshot().state).toBe("stale");
  expect(f.source.getSnapshot().view?.request?.key).toBe(view.request!.key);
  f.starts[0].receive({ ...event("2"), kind: "heartbeat" });
  expect(f.source.getSnapshot().state).toBe("live");
  release();
});

it("retirement stays terminal until explicit metadata reconnect, with no approval or retry", async () => {
  const f = fixture();
  const release = f.source.subscribe(vi.fn());
  f.starts[0].resolve(owner);
  await flush();
  f.starts[0].receive(event());
  f.starts[0].receive({ ...event("2"), kind: "retired" });
  f.starts[0].receive(event("3"));
  await vi.advanceTimersByTimeAsync(TASK_HEALTH_TIMEOUT_MS * 2);
  expect(f.source.getSnapshot().state).toBe("retired");
  expect(f.start).toHaveBeenCalledTimes(1);
  f.source.reconnect();
  expect(f.start).toHaveBeenCalledTimes(2);
  expect(f.source.getSnapshot().state).toBe("connecting");
  // No new permission facts have been invented by recovery.
  expect(f.source.getSnapshot().view?.featureEnabled).toBe(false);
  release();
});

it("a failed old start cannot overwrite a newer stream", async () => {
  const f = fixture();
  const release = f.source.subscribe(vi.fn());
  f.source.reconnect();
  f.starts[1].resolve(owner);
  await flush();
  f.starts[1].receive(event());
  f.starts[0].reject(new Error("synthetic old transport failed"));
  await flush();
  expect(f.source.getSnapshot().state).toBe("live");
  release();
});

it("non-desktop never starts a local stream", () => {
  const f = fixture(false);
  const release = f.source.subscribe(vi.fn());
  expect(f.source.getSnapshot().state).toBe("unavailable");
  expect(f.start).not.toHaveBeenCalled();
  release();
});

it("a newly connected Host retries metadata after a no-endpoint start without inventing access", async () => {
  const f = fixture();
  const release = f.source.subscribe(vi.fn());
  f.starts[0].reject(new Error("no original endpoint yet"));
  await flush();
  expect(f.source.getSnapshot().state).toBe("unavailable");
  f.source.endpointReady();
  expect(f.start).toHaveBeenCalledTimes(2);
  f.starts[1].resolve(owner);
  await flush();
  f.starts[1].receive(event());
  expect(f.source.getSnapshot().view?.featureEnabled).toBe(false);
  f.source.endpointReady();
  expect(f.start).toHaveBeenCalledTimes(2);
  release();
});

it("an endpoint-ready event in the failed subscribe gap is retained for exactly one new start", async () => {
  const f = fixture();
  const release = f.source.subscribe(vi.fn());
  f.source.endpointReady();
  f.source.endpointReady();
  expect(f.start).toHaveBeenCalledTimes(1);
  f.starts[0].reject(new Error("initial no endpoint"));
  await flush();
  expect(f.start).toHaveBeenCalledTimes(2);
  f.starts[1].reject(new Error("still unavailable"));
  await flush();
  expect(f.start).toHaveBeenCalledTimes(2);
  release();
});
