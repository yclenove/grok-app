// Experimental consumer for the versioned compositor counter. Not imported by
// extension.js and not shipped/installed by App helper management.
// provider.read() returns only {version, generation}; subscribe/disconnect are
// a read-only main-thread notification adapter. No event/source classification.
export class PhysicalInputWatch {
  constructor(provider, advance, fault) {
    this._provider = provider;
    this._advance = advance;
    this._fault = fault;
    this._started = false;
    this._running = false;
    this._subscribing = false;
    this._id = null;
    this._generation = null;
  }

  start() {
    if (this._started) throw new Error('Observer already started');
    this._started = true;
    this._running = true;
    try {
      for (const name of ['read', 'subscribe', 'disconnect'])
        if (typeof this._provider?.[name] !== 'function')
          throw new Error('Physical counter unavailable');
      this._subscribing = true;
      let id;
      try {
        id = this._provider.subscribe(() => {
          if (!this._running) return;
          if (this._subscribing) { this._abort(); return; }
          this.poll();
        });
      } finally { this._subscribing = false; }
      if (!Number.isSafeInteger(id) || id <= 0 || !this._running) {
        if (Number.isSafeInteger(id) && id > 0) this._provider.disconnect(id);
        throw new Error('Physical counter subscription failed');
      }
      this._id = id;
      // Subscribe first. No grant exists before this initial baseline. Subsequent
      // reads are monotonic; notification coalescing never discards the delta.
      this._read();
    } catch (error) {
      this._abort();
      throw error;
    }
  }

  _read() {
    const snapshot = this._provider.read();
    if (snapshot?.version !== 1 ||
        !Number.isSafeInteger(snapshot.generation) || snapshot.generation < 0)
      throw new Error('Invalid physical counter snapshot');
    const next = snapshot.generation;
    if (this._generation !== null && next < this._generation)
      throw new Error('Physical counter regressed');
    const previous = this._generation;
    this._generation = next;
    if (previous !== null && next !== previous) this._advance(next - previous);
  }

  // Call on GetState/heartbeat too: readback sees the input-thread counter even
  // if its main-thread notify has not dispatched. This is NOT idle-time polling.
  poll() {
    if (!this._running) return;
    try { this._read(); } catch { this._abort(); }
  }

  _abort() {
    const wasRunning = this._running;
    try { this.stop(); } catch { /* Fault stays sticky after disconnect failure. */ }
    if (wasRunning) {
      try { this._fault(); } catch { /* Never escape into compositor dispatch. */ }
    }
  }

  stop() {
    this._running = false;
    const id = this._id;
    this._id = null;
    if (id !== null) this._provider.disconnect(id);
  }
}
