import { runOwner } from "./profile.mjs";
import { workerException, WORKER_COMPLETION_UNKNOWN } from "./worker-errors.mjs";

// Keep terminal owner tombstones until worker exit. Never evict one and let a
// delayed request recreate authority; capacity exhaustion is fail-closed.
export class RunOperations {
  #runs = new Map();
  #active = 0;
  #maxRuns;
  #maxActive;
  #closed = false;

  constructor({ maxRuns = 256, maxActive = 64 } = {}) {
    this.#maxRuns = maxRuns;
    this.#maxActive = maxActive;
  }

  #get(owner, create = false) {
    try {
      if (typeof owner !== "string") throw new Error("invalid owner");
      runOwner(owner);
    } catch {
      throw workerException(400, "invalid_owner", "valid worker owner required");
    }
    let run = this.#runs.get(owner);
    if (!run && create) {
      if (this.#runs.size >= this.#maxRuns) {
        throw workerException(429, "run_capacity", "worker run capacity exhausted; restart the owned runtime");
      }
      run = { revision: 1, controlled: false, phase: "running", active: 0,
        controller: new AbortController(), idleWaiters: new Set(), resourceCleanups: new Map() };
      this.#runs.set(owner, run);
    }
    if (!run) throw workerException(404, "run_unknown", "worker run is unknown");
    return run;
  }

  #revision(run, revision, legacy = false) {
    if (legacy && !run.controlled && revision === undefined) return;
    if (!Number.isSafeInteger(revision) || revision < 1 || revision !== run.revision) {
      throw workerException(409, "stale_run_revision", "worker run revision is missing or stale");
    }
  }

  #view(run) {
    return { runRevision: run.revision, phase: run.phase,
      activeOperations: run.active + run.resourceCleanups.size, idle: this.#idle(run) };
  }

  #idle(run) {
    return run.active === 0 && run.resourceCleanups.size === 0;
  }

  #wakeIdle(run) {
    if (!this.#idle(run)) return;
    for (const wake of run.idleWaiters) wake();
    run.idleWaiters.clear();
  }

  begin(owner, revision) {
    if (this.#closed) throw workerException(409, "run_fenced", "worker is shutting down");
    if (revision !== undefined && (!Number.isSafeInteger(revision) || revision < 1)) {
      throw workerException(400, "invalid_run_revision", "positive integer runRevision required");
    }
    if (!this.#runs.has(owner) && revision !== undefined && revision !== 1) {
      throw workerException(409, "stale_run_revision", "new worker runs start at revision 1");
    }
    const run = this.#get(owner, true);
    this.#revision(run, revision, true);
    if (run.phase !== "running") {
      throw workerException(409, "run_fenced", "worker run is paused or stopped");
    }
    if (revision !== undefined) run.controlled = true;
    return this.#enter(run);
  }

  // Host-only profile cleanup also counts as physical work, even while paused.
  // Its caller must not run business actions or interpret its signal as a grant.
  holdCleanup(owner) {
    return this.#enter(this.#get(owner));
  }

  // An already-owned resource needs cleanup even when ordinary admission is
  // full. Deduplicate by that resource's object identity, never by request/PID.
  holdResourceCleanup(owner, resource) {
    if (!resource || typeof resource !== "object") throw new Error("owned cleanup resource required");
    const run = this.#get(owner);
    const existing = run.resourceCleanups.get(resource);
    if (existing) return existing;
    const lease = { finish: () => {
      if (run.resourceCleanups.get(resource) !== lease) return;
      run.resourceCleanups.delete(resource);
      this.#wakeIdle(run);
    } };
    run.resourceCleanups.set(resource, lease);
    return lease;
  }

  #enter(run) {
    if (this.#active >= this.#maxActive) {
      throw workerException(429, "operation_capacity", "worker operation capacity exhausted");
    }
    const signal = run.controller.signal;
    run.active += 1;
    this.#active += 1;
    let finished = false;
    return {
      signal,
      check() {
        if (signal.aborted) {
          throw workerException(409, "run_cancelled", "worker operation was cancelled", WORKER_COMPLETION_UNKNOWN);
        }
      },
      finish: () => {
        if (finished) return;
        finished = true;
        run.active -= 1;
        this.#active -= 1;
        this.#wakeIdle(run);
      },
    };
  }

  pause(owner, revision) {
    if (!Number.isSafeInteger(revision) || revision < 1) {
      throw workerException(400, "invalid_run_revision", "positive integer runRevision required");
    }
    if (!this.#runs.has(owner) && revision !== 1) {
      throw workerException(409, "stale_run_revision", "new worker runs start at revision 1");
    }
    const run = this.#get(owner, true);
    this.#revision(run, revision);
    if (run.phase === "stopped") throw workerException(409, "run_fenced", "worker run is stopped");
    run.controlled = true;
    run.phase = "paused";
    run.controller.abort();
    return this.#view(run);
  }

  status(owner, revision) {
    const run = this.#get(owner);
    this.#revision(run, revision);
    return this.#view(run);
  }

  resume(owner, revision, nextRevision) {
    if (this.#closed) throw workerException(409, "run_fenced", "worker is shutting down");
    const run = this.#get(owner);
    this.#revision(run, revision);
    if (run.phase !== "paused" || !this.#idle(run)) {
      throw workerException(409, "run_not_quiescent", "resume requires a paused and physically idle run");
    }
    if (!Number.isSafeInteger(nextRevision) || nextRevision !== revision + 1) {
      throw workerException(400, "invalid_run_revision", "nextRunRevision must advance exactly once");
    }
    run.revision = nextRevision;
    run.controller = new AbortController();
    run.phase = "running";
    return this.#view(run);
  }

  stop(owner) {
    const run = this.#get(owner, true);
    run.controlled = true;
    run.phase = "stopped";
    run.controller.abort();
    return this.#view(run);
  }

  stopAll() {
    this.#closed = true;
    for (const owner of this.#runs.keys()) this.stop(owner);
  }

  async waitAllIdle(timeoutMs) {
    return (await Promise.all([...this.#runs.keys()].map(owner => this.waitIdle(owner, timeoutMs)))).every(Boolean);
  }

  // Physical cleanup outlives any HTTP observer. The shutdown caller fences
  // new admission first, so this set includes every run that can still settle.
  async whenAllIdle() {
    await Promise.all([...this.#runs.keys()].map(owner => this.whenIdle(owner)));
  }

  whenIdle(owner) {
    const run = this.#get(owner);
    if (this.#idle(run)) return Promise.resolve();
    return new Promise(resolve => {
      const wake = () => {
        run.idleWaiters.delete(wake);
        resolve();
      };
      run.idleWaiters.add(wake);
    });
  }

  async waitIdle(owner, timeoutMs) {
    const run = this.#get(owner);
    if (this.#idle(run)) return true;
    return new Promise(resolve => {
      const finish = idle => {
        clearTimeout(timer);
        run.idleWaiters.delete(wake);
        resolve(idle);
      };
      const wake = () => finish(true);
      const timer = setTimeout(() => finish(false), timeoutMs);
      run.idleWaiters.add(wake);
    });
  }
}
