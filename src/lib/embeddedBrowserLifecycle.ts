/** Frontend leases survive pane remounts; only the native close reply retires an owner. */
import { sideBrowserClose } from "@/lib/api";

export type BrowserCloseState = Readonly<{
  label: string;
  title: string;
  phase: "idle" | "closing" | "failed";
  error: string | null;
  clients: number;
  attempt: number;
}>;

const IDLE: BrowserCloseState = Object.freeze({
  label: "", title: "", phase: "idle", error: null, clients: 0, attempt: 0,
});
const EMPTY: readonly BrowserCloseState[] = Object.freeze([]);

type Owner = {
  label: string;
  title: string;
  clients: Set<symbol>;
  creates: Set<Promise<void>>;
  timer: ReturnType<typeof setTimeout> | null;
  close: Promise<void> | null;
  deferredAttempt: number;
  view: BrowserCloseState;
};

export class BrowserClosePendingError extends Error {
  constructor(readonly label: string, detail: string) {
    super(detail);
    this.name = "BrowserClosePendingError";
  }
}

export function createEmbeddedBrowserLifecycle(
  closeNative: (label: string) => Promise<void>,
  graceMs = 150,
) {
  const owners = new Map<string, Owner>();
  const listeners = new Set<() => void>();
  let failures = EMPTY;

  function ownerFor(label: string, title = label): Owner {
    let owner = owners.get(label);
    if (!owner) {
      owner = { label, title, clients: new Set(), creates: new Set(), timer: null, close: null, deferredAttempt: -1, view: IDLE };
      owners.set(label, owner);
    }
    return owner;
  }

  function publish(owner: Owner, phase = owner.view.phase, error = owner.view.error) {
    owner.view = Object.freeze({
      label: owner.label, title: owner.title, phase, error,
      clients: owner.clients.size, attempt: owner.view.attempt,
    });
    failures = Object.freeze([...owners.values()]
      .filter(value => value.deferredAttempt !== value.view.attempt)
      .map(value => value.view).filter(value => value.error !== null && value.clients === 0));
    for (const listener of listeners) listener();
  }

  function cancelTimer(owner: Owner) {
    if (owner.timer !== null) clearTimeout(owner.timer);
    owner.timer = null;
  }

  function close(owner: Owner, explicit: boolean): Promise<void> {
    if (owner.close) return owner.close;
    cancelTimer(owner);
    owner.view = { ...owner.view, attempt: owner.view.attempt + 1 };
    const original = Promise.resolve().then(async () => {
      // A timed-out create promise may still create a native child. Its original
      // reply must settle before cleanup can retire that label.
      while (owner.creates.size) await Promise.all([...owner.creates]);
      // A remount can still cancel cleanup before any native close was sent.
      if (!explicit && owner.clients.size) return;
      await closeNative(owner.label);
    }).then(() => {
      owner.close = null;
      if (!owner.clients.size) owners.delete(owner.label);
      publish(owner, "idle", null);
    }, error => {
      owner.close = null;
      const detail = error instanceof Error ? error.message : String(error);
      publish(owner, "failed", detail);
      throw new BrowserClosePendingError(owner.label, detail);
    });
    owner.close = original;
    publish(owner, "closing");
    return original;
  }

  function schedule(owner: Owner) {
    if (owner.clients.size || owner.close || owner.view.error !== null) return;
    cancelTimer(owner);
    owner.timer = setTimeout(() => {
      owner.timer = null;
      if (owners.get(owner.label) !== owner || owner.clients.size) return;
      // The persistent state, rather than a console warning, owns this failure.
      void close(owner, false).catch(() => undefined);
    }, graceMs);
  }

  return {
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => { listeners.delete(listener); };
    },
    getState: (label: string) => owners.get(label)?.view ?? IDLE,
    getFailures: () => failures,
    getServerState: () => IDLE,
    getServerFailures: () => EMPTY,
    retain(label: string, title: string) {
      const owner = ownerFor(label, title);
      owner.title = title;
      const client = Symbol(label);
      owner.clients.add(client);
      cancelTimer(owner);
      publish(owner);
      let released = false;
      return {
        async prepare() {
          if (owner.close) await owner.close;
          if (owner.view.error !== null) throw new BrowserClosePendingError(label, owner.view.error);
          return !released;
        },
        trackCreate<T>(operation: Promise<T>): Promise<T> {
          const settled = operation.then(() => undefined, () => undefined);
          owner.creates.add(settled);
          void settled.then(() => {
            owner.creates.delete(settled);
            if (!released) return;
            schedule(owner);
          });
          return operation;
        },
        release() {
          if (released) return;
          released = true;
          owner.clients.delete(client);
          publish(owner);
          schedule(owner);
        },
      };
    },
    // Restore the retained native-owner state after a frontend reload loses the
    // in-memory lease. Only the Host's explicit machine code identifies it.
    recoverNativeFailure(label: string, detail: string) {
      if (!detail.includes("side-browser-close-pending:")) return false;
      const owner = ownerFor(label);
      publish(owner, "failed", detail);
      return true;
    },
    retryClose(label: string) { return close(ownerFor(label), true); },
    deferFailure(label: string, attempt: number) {
      const owner = owners.get(label);
      if (!owner || owner.view.attempt !== attempt) return;
      owner.deferredAttempt = attempt;
      publish(owner);
    },
  };
}

export const embeddedBrowserLifecycle = createEmbeddedBrowserLifecycle(sideBrowserClose);
