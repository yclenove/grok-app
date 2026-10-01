import { workerException, WORKER_COMPLETION_UNKNOWN } from "./worker-errors.mjs";

export const NAVIGATION_TIMEOUT_MS = 8000;

// Use Playwright's own deadline, not a response-only Promise.race which could
// release admission while the original navigation call is still running.
export async function navigatePage(page, { kind = "goto", url, signal } = {}) {
  if (kind !== "goto" && kind !== "reload") throw new TypeError("invalid navigation kind");
  if (signal?.aborted) throw workerException(409, "run_cancelled", "worker operation was cancelled");
  const options = { waitUntil: "domcontentloaded", timeout: NAVIGATION_TIMEOUT_MS };
  let result;
  try {
    result = kind === "reload" ? await page.reload(options) : await page.goto(url, options);
  } catch (error) {
    if (signal?.aborted) {
      throw workerException(409, "run_cancelled", "worker operation was cancelled", WORKER_COMPLETION_UNKNOWN);
    }
    // Match the pinned library's actual error type, never an arbitrary name or
    // message. Native messages can contain URLs, queries and profile paths.
    const { errors } = await import("playwright-core");
    if (error instanceof errors.TimeoutError) {
      throw workerException(504, "navigation_timeout",
        "page navigation did not finish before its deadline; observe before continuing",
        WORKER_COMPLETION_UNKNOWN);
    }
    throw error;
  }
  if (signal?.aborted) {
    throw workerException(409, "run_cancelled", "worker operation was cancelled", WORKER_COMPLETION_UNKNOWN);
  }
  return result;
}
