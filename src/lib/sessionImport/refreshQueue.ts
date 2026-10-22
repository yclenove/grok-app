/** Import events are hints: coalesce bursts, serialize background reads, and
 * retain one trailing refresh when a hint arrives during an outstanding read.
 * Every read re-authenticates the whole signed job, so the pause after a read
 * grows with its duration: large jobs stay responsive without the progress
 * view competing with the worker it is reporting on. */
const MIN_PAUSE_MS = 250;
const MAX_PAUSE_MS = 4000;

/** Keep background reads to at most a third of the time, within fixed bounds. */
export function refreshPauseMs(readMs: number): number {
  return Math.min(MAX_PAUSE_MS, Math.max(MIN_PAUSE_MS, Math.round(2 * readMs)));
}

export function createImportRefreshQueue(
  refresh: () => Promise<void>,
  isBusy: () => boolean = () => false,
) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let dirty = false;
  let running = false;
  let disposed = false;
  let generation = 0;
  let pause = MIN_PAUSE_MS;

  const schedule = () => {
    if (disposed || !dirty || running || timer !== undefined) return;
    timer = setTimeout(() => { void drain(); }, pause);
  };
  const drain = async () => {
    timer = undefined;
    if (disposed || !dirty) return;
    if (isBusy()) { schedule(); return; }
    const owner = generation;
    dirty = false;
    running = true;
    const started = Date.now();
    try { await refresh(); }
    catch { /* The domain loader owns visible errors; later hints may retry. */ }
    finally {
      if (owner === generation) {
        pause = refreshPauseMs(Date.now() - started);
        running = false;
        schedule();
      }
    }
  };
  const reset = () => {
    generation += 1;
    dirty = false;
    running = false;
    if (timer !== undefined) clearTimeout(timer);
    timer = undefined;
  };
  return {
    request: () => { if (!disposed) { dirty = true; schedule(); } },
    reset,
    dispose: () => { disposed = true; reset(); },
  };
}

export type ImportRefreshQueue = ReturnType<typeof createImportRefreshQueue>;
