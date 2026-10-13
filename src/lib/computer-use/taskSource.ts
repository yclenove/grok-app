/** One metadata stream per observed session. Never stores execution grants. */
import {
  computerTaskSubscribe, computerTaskUnsubscribe,
  type ComputerTaskEvent, type ComputerTaskSubscription,
} from "@/lib/api/computerTaskEvents";
import { isDesktopHost } from "@/lib/api/host";
import type { ComputerTaskView } from "@/lib/api/computerTask";

export type ComputerTaskSnapshot = Readonly<{
  state: "idle" | "connecting" | "live" | "stale" | "retired" | "unavailable";
  view: ComputerTaskView | null;
  owner: ComputerTaskSubscription | null;
}>;

export const EMPTY_TASK_SNAPSHOT: ComputerTaskSnapshot = Object.freeze({ state: "idle", view: null, owner: null });
export const TASK_HEALTH_TIMEOUT_MS = 4_000;

type Dependencies = {
  available: () => boolean;
  start: typeof computerTaskSubscribe;
  stop: typeof computerTaskUnsubscribe;
};

type Generation = {
  active: boolean;
  owner: ComputerTaskSubscription | null;
  early: ComputerTaskEvent | null;
  sequence: bigint;
  watchdog: ReturnType<typeof setTimeout> | null;
  endpointChanged: boolean;
};

function sameOwner(a: ComputerTaskSubscription, b: ComputerTaskSubscription) {
  return a.subscriptionId === b.subscriptionId && a.sessionId === b.sessionId
    && a.connectionId === b.connectionId && a.hostInstanceId === b.hostInstanceId;
}

function belongs(view: ComputerTaskView, owner: ComputerTaskSubscription) {
  if (view.sessionId !== owner.sessionId || view.connectionId !== owner.connectionId) return false;
  const keys = [view.request?.key, ...view.grants.map((grant) => grant.key), ...view.executions.map((run) => run.key)];
  return keys.every((key) => !key || key.connectionId === owner.connectionId);
}

/** Exported for lifecycle tests with synthetic Channel delivery. */
export class ComputerTaskSource {
  private listeners = new Set<() => void>();
  private snapshot: ComputerTaskSnapshot = EMPTY_TASK_SNAPSHOT;
  private generation: Generation | null = null;

  constructor(readonly sessionId: string, private readonly deps: Dependencies = {
    available: isDesktopHost, start: computerTaskSubscribe, stop: computerTaskUnsubscribe,
  }) {}

  getSnapshot = () => this.snapshot;

  subscribe = (listener: () => void) => {
    // Each consumer has its own disposer, including identical callbacks.
    const notify = () => listener();
    this.listeners.add(notify);
    if (this.listeners.size === 1) this.start();
    let active = true;
    return () => {
      if (!active) return;
      active = false;
      this.listeners.delete(notify);
      if (this.listeners.size === 0) {
        this.release();
        this.snapshot = EMPTY_TASK_SNAPSHOT;
      }
    };
  };

  /** Explicit metadata recovery only. It never requests or approves access. */
  reconnect = () => {
    if (!this.listeners.size) return;
    this.release();
    this.start();
  };

  /** A Host lifecycle event only prompts a new metadata subscription. It is
   * not proof of an endpoint, task identity, authorization or execution. */
  endpointReady = () => {
    if (!this.listeners.size || this.snapshot.state === "live") return;
    if (this.snapshot.state === "connecting" && this.generation?.active) {
      this.generation.endpointChanged = true;
      return;
    }
    this.reconnect();
  };

  private publish(snapshot: ComputerTaskSnapshot) {
    this.snapshot = Object.freeze(snapshot);
    for (const listener of this.listeners) listener();
  }

  private stop(owner: ComputerTaskSubscription) {
    // Always cancel using this source's original session. A malformed ack
    // must never grant cancellation authority over a different session.
    void this.deps.stop(this.sessionId, owner.subscriptionId).catch(() => {});
  }

  private release() {
    const generation = this.generation;
    this.generation = null;
    if (!generation) return;
    generation.active = false;
    generation.early = null;
    if (generation.watchdog !== null) clearTimeout(generation.watchdog);
    if (generation.owner) this.stop(generation.owner);
  }

  private start() {
    if (!this.deps.available()) {
      this.publish({ state: "unavailable", view: null, owner: null });
      return;
    }
    const generation: Generation = { active: true, owner: null, early: null, sequence: 0n, watchdog: null, endpointChanged: false };
    this.generation = generation;
    // Old facts remain available for an exact-key Stop, but aren't green.
    this.publish({ ...this.snapshot, state: "connecting" });
    this.armWatchdog(generation);
    void this.deps.start(this.sessionId, (event) => {
      if (!this.current(generation)) return;
      if (!generation.owner) {
        // Bounded pre-ack buffer. The next heartbeat carries a full view.
        generation.early = event;
        return;
      }
      this.accept(generation, event);
    }).then((owner) => {
      if (!this.current(generation)) { this.stop(owner); return; }
      if (owner.sessionId !== this.sessionId || !owner.subscriptionId || !owner.connectionId || !owner.hostInstanceId) {
        this.stop(owner);
        generation.active = false;
        if (generation.watchdog !== null) clearTimeout(generation.watchdog);
        this.publish({ ...this.snapshot, state: "unavailable" });
        return;
      }
      generation.owner = owner;
      const early = generation.early;
      generation.early = null;
      if (early) this.accept(generation, early);
    }).catch(() => {
      if (!this.current(generation)) return;
      generation.active = false;
      generation.early = null;
      if (generation.watchdog !== null) clearTimeout(generation.watchdog);
      this.publish({ ...this.snapshot, state: "unavailable" });
      if (generation.endpointChanged) this.reconnect();
    });
  }

  private current(generation: Generation) {
    return generation.active && this.generation === generation && this.listeners.size > 0;
  }

  private armWatchdog(generation: Generation) {
    if (generation.watchdog !== null) clearTimeout(generation.watchdog);
    generation.watchdog = setTimeout(() => {
      if (this.current(generation)) this.publish({ ...this.snapshot, state: "stale" });
    }, TASK_HEALTH_TIMEOUT_MS);
  }

  private accept(generation: Generation, event: ComputerTaskEvent) {
    const owner = generation.owner;
    if (!owner || !this.current(generation) || !sameOwner(event, owner)) return;
    if (!/^[1-9]\d{0,19}$/.test(event.sequence)) return;
    const sequence = BigInt(event.sequence);
    if (sequence > 18446744073709551615n || sequence <= generation.sequence) return;
    if (event.kind !== "retired" && !belongs(event.view, owner)) return;
    generation.sequence = sequence;
    if (event.kind === "retired") {
      generation.active = false;
      if (generation.watchdog !== null) clearTimeout(generation.watchdog);
      this.publish({ ...this.snapshot, state: "retired", owner });
      this.stop(owner);
      return;
    }
    this.armWatchdog(generation);
    this.publish({ state: "live", view: event.view, owner });
  }
}

/** Entries exist only while consumed; a late ack retains its own generation. */
const sources = new Map<string, ComputerTaskSource>();

export function getTaskSessionSnapshot(sessionId: string | null): ComputerTaskSnapshot {
  return sessionId ? sources.get(sessionId)?.getSnapshot() ?? EMPTY_TASK_SNAPSHOT : EMPTY_TASK_SNAPSHOT;
}

export function subscribeTaskSession(sessionId: string | null, listener: () => void) {
  if (!sessionId) return () => {};
  let source = sources.get(sessionId);
  if (!source) { source = new ComputerTaskSource(sessionId); sources.set(sessionId, source); }
  const release = source.subscribe(listener);
  return () => {
    release();
    if (source.getSnapshot() === EMPTY_TASK_SNAPSHOT && sources.get(sessionId) === source) sources.delete(sessionId);
  };
}

export function reconnectTaskSession(sessionId: string) {
  sources.get(sessionId)?.reconnect();
}

export function noteTaskEndpointReady(sessionId: string) {
  sources.get(sessionId)?.endpointReady();
}
