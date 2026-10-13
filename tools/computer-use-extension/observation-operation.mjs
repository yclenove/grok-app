// A deadline retires authority, not a browser Promise. run() always joins the
// original call, so its owner cannot report physical completion while it is live.
export class ObservationOperation {
  #controller = new AbortController();
  #signal;
  #abort;
  #timer;
  #running = false;
  #closed = false;
  #deadline;

  constructor(deadlineMs, signal) {
    this.#deadline = deadlineMs;
    this.#signal = signal;
    this.#abort = () => this.cancel();
    signal?.addEventListener("abort", this.#abort, { once: true });
    if (signal?.aborted || !Number.isSafeInteger(deadlineMs) || deadlineMs <= Date.now()) this.cancel();
    else {
      this.#timer = setTimeout(this.#abort, Math.min(10000, deadlineMs - Date.now()));
      this.#timer.unref?.();
    }
  }

  live() { return !this.#closed && !this.#controller.signal.aborted && Date.now() < this.#deadline; }
  cancel() { this.#controller.abort(); }
  check() { if (!this.live()) throw new Error("cancelled"); }

  async run(start) {
    this.check();
    if (this.#running) throw new Error("busy");
    this.#running = true;
    const timer = setTimeout(this.#abort, 5000);
    timer.unref?.();
    try {
      const result = await start();
      this.check();
      return result;
    } catch (error) {
      this.check();
      throw error;
    } finally {
      clearTimeout(timer);
      this.#running = false;
    }
  }

  close() {
    if (this.#running) throw new Error("observationUnsettled");
    this.#closed = true;
    clearTimeout(this.#timer);
    this.#signal?.removeEventListener("abort", this.#abort);
  }
}
