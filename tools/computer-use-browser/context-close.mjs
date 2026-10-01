import { WORKER_COMPLETION_UNKNOWN, workerException } from "./worker-errors.mjs";

// Bound only the HTTP observer, never the owned cleanup itself. In particular,
// expiry cannot release a lease, redispatch close, or claim physical idleness.
// Race observes late rejection too, even after the caller has received pending.
export async function waitForCleanup(pending, timeoutMs = 5000) {
  let timer;
  try {
    return await Promise.race([
      pending,
      new Promise((_, reject) => {
        timer = setTimeout(() => reject(workerException(409, "run_cleanup_pending",
          "browser cleanup is still pending", WORKER_COMPLETION_UNKNOWN)), timeoutMs);
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

// Playwright's second BrowserContext.close() may resolve before the first one.
// Every route for this exact slot must share its first physical close attempt.
export class ContextClose {
  #context;
  #closed = false;
  #started = false;
  #settled = false;
  #fulfilled = false;
  #pending = null;
  #retainCleanup;
  #cleanup = null;

  constructor(context, retainCleanup = () => null) {
    this.#context = context;
    this.#retainCleanup = retainCleanup;
    context.once("close", () => {
      this.#closed = true;
      this.#releaseConfirmedCleanup();
    });
  }

  #releaseConfirmedCleanup() {
    if (!this.#closed || !this.#settled || !this.#fulfilled) return;
    const cleanup = this.#cleanup;
    this.#cleanup = null;
    cleanup?.finish();
  }

  check() {
    if (this.#started || this.#closed) {
      throw workerException(409, "profile_closing", "the original browser profile is closing");
    }
  }

  close() {
    if (this.#pending && !this.#settled) return this.#pending;
    // Protocol closure can precede native process exit/artifact cleanup. A
    // late close event may reconcile a fulfilled attempt, never a rejected one.
    if (this.#closed && (!this.#started || this.#fulfilled)) return Promise.resolve();
    if (!this.#pending) {
      this.#cleanup = this.#retainCleanup();
      this.#started = true;
      this.#pending = Promise.resolve().then(() => this.#context.close()).then(() => {
        this.#fulfilled = true;
        if (!this.#closed) {
          throw workerException(409, "context_close_unconfirmed",
            "browser context closure is not confirmed", WORKER_COMPLETION_UNKNOWN);
        }
      }).finally(() => {
        this.#settled = true;
        this.#releaseConfirmedCleanup();
      });
    }
    // Do not retry a failed Playwright close: its internal one-shot flag could
    // turn the second call into false success without closing the browser.
    return this.#pending;
  }
}
