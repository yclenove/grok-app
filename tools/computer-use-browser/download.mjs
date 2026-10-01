import { randomBytes } from "node:crypto";
import { createWriteStream, renameSync, rmSync } from "node:fs";
import { Transform } from "node:stream";
import { pipeline } from "node:stream/promises";
import { inspectHandle } from "./observation-extract.mjs";
import {
  resolveObservationTarget,
} from "./observation-state.mjs";
import {
  WORKER_COMPLETION_NOT_STARTED,
  WORKER_COMPLETION_UNKNOWN,
  workerException,
} from "./worker-errors.mjs";

const MAX_REDIRECTS = 5;

function fail(status, code, message) {
  throw workerException(status, code, message, WORKER_COMPLETION_NOT_STARTED);
}

export function uniquePartPath(destPath) {
  return `${destPath}.${randomBytes(8).toString("hex")}.part`;
}

export function removePart(partPath) {
  try {
    rmSync(partPath, { force: true });
  } catch {
    /* ignore */
  }
}

function cookieHeader(cookies, url) {
  const host = new URL(url).hostname;
  return cookies
    .filter((cookie) => {
      const domain = String(cookie.domain || "").replace(/^\./, "");
      return !domain || host === domain || host.endsWith(`.${domain}`);
    })
    .map((cookie) => `${cookie.name}=${cookie.value}`)
    .join("; ");
}

export async function streamToPart(readable, partPath, maxBytes, signal) {
  let bytes = 0;
  const counted = new Transform({
    transform(chunk, _encoding, done) {
      const buf = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
      bytes += buf.length;
      if (bytes > maxBytes) {
        done(workerException(413, "download_too_large", "download exceeds size limit", WORKER_COMPLETION_NOT_STARTED));
      } else {
        done(null, buf);
      }
    },
  });
  // Cancellation must wake a blocked read or disk backpressure, not only be
  // noticed when another chunk eventually arrives. pipeline joins both ends.
  const out = createWriteStream(partPath);
  const closed = new Promise(resolve => out.once("close", resolve));
  try {
    await pipeline(readable, counted, out, { signal });
  } finally {
    // An upstream error can reject pipeline before an opening file stream
    // emits close. In particular, Windows cannot remove that .part yet.
    if (!out.closed) out.destroy();
    await closed;
  }
  if (!bytes) fail(400, "download_empty", "download produced no bytes");
  return bytes;
}

export async function fetchWithRedirectChecks(startUrl, { urlAllowed, cookiesForUrl = async () => [], signal, maxBytes, partPath }) {
  let current = startUrl;
  for (let hop = 0; hop <= MAX_REDIRECTS; hop += 1) {
    if (!urlAllowed(current)) {
      fail(400, "download_redirect_rejected", "download url must re-check the actual target");
    }
    const headers = { accept: "*/*" };
    // Ask the browser for this URL's applicable cookies at every hop. A whole
    // context cookie dump would ignore Secure, expiry and path restrictions.
    const cookie = cookieHeader(await cookiesForUrl(current), current);
    if (cookie) headers.cookie = cookie;
    const response = await fetch(current, {
      redirect: "manual",
      signal,
      headers,
    });
    const status = response.status;
    if ([301, 302, 303, 307, 308].includes(status)) {
      const location = response.headers.get("location");
      await response.body?.cancel();
      if (!location) {
        fail(400, "download_redirect_rejected", "redirect is missing Location");
      }
      current = new URL(location, current).href;
      continue;
    }
    if (!response.ok || !response.body) {
      await response.body?.cancel();
      fail(400, "download_failed", `download fetch ${status}`);
    }
    const bytes = await streamToPart(response.body, partPath, maxBytes, signal);
    return { bytes, finalUrl: current };
  }
  fail(400, "download_redirect_rejected", "too many redirects");
}

export function prepareDownloadTarget(observation, body) {
  if (!body.elementRef) {
    fail(400, "element_ref_required", "elementRef required");
  }
  return resolveObservationTarget(observation, {
    pageGeneration: body.pageGeneration,
    snapshotId: body.snapshotId,
    elementRef: body.elementRef,
  });
}

export async function downloadViaHandle({
  page,
  target,
  destPath,
  maxBytes,
  urlAllowed,
  signal,
}) {
  const checkCancelled = () => { if (signal?.aborted) throw new Error("cancelled"); };
  checkCancelled();
  const live = await inspectHandle(target.handle);
  checkCancelled();
  const tag = String(live.signature || "").split("|")[0];
  if (tag !== "a") {
    fail(400, "download_target_invalid", "download target must be a link");
  }
  const link = await target.handle.evaluate((element) => ({
    href: element.getAttribute("href") || "",
    native: element.hasAttribute("download"),
  }));
  checkCancelled();
  if (!link.href) {
    fail(400, "download_target_invalid", "download target has no href");
  }
  const fileUrl = new URL(link.href, page.url()).href;
  if (!urlAllowed(fileUrl)) {
    fail(400, "download_redirect_rejected", "download url must re-check the actual target");
  }

  const partPath = uniquePartPath(destPath);
  let nativeDownload;
  let downloadPromise;
  let cancellation;
  let nativeComplete = false;
  const cancelNative = () => {
    if (nativeDownload && !cancellation) {
      // Observe rejection immediately and retain the task until finally. If
      // cancellation fails, the original native operation still must settle.
      cancellation = Promise.resolve().then(() => nativeDownload.cancel()).then(() => null, error => error);
    }
  };
  signal?.addEventListener("abort", cancelNative);
  try {
    let bytes = 0;
    if (link.native) {
      checkCancelled();
      // Catch the event waiter immediately: click may fail before it settles.
      downloadPromise = page.waitForEvent("download", { timeout: 4000 }).then(download => {
        nativeDownload = download;
        if (signal?.aborted) cancelNative();
        return { download };
      }, error => ({ error }));
      await target.handle.click();
      const event = await downloadPromise;
      if (event.error) {
        throw workerException(409, "download_start_unconfirmed",
          "native download was not confirmed; its managed browser context is closed before cleanup completes",
          WORKER_COMPLETION_UNKNOWN);
      }
      const { download } = event;
      checkCancelled();
      const actual = download.url();
      if (actual && !urlAllowed(actual)) {
        try {
          await download.cancel();
        } catch {
          /* ignore */
        }
        fail(400, "download_redirect_rejected", "download url must re-check the actual target");
      }
      const stream = await download.createReadStream();
      if (!stream) {
        throw new Error("no download stream");
      }
      bytes = await streamToPart(stream, partPath, maxBytes, signal);
      checkCancelled();
      nativeComplete = true;
    } else {
      // Plain links use one explicit HTTP transfer. Never click first and then
      // retry via fetch: a missing Download event does not prove no request ran.
      const fetched = await fetchWithRedirectChecks(fileUrl, {
        urlAllowed,
        cookiesForUrl: url => page.context().cookies(url),
        signal,
        maxBytes,
        partPath,
      });
      bytes = fetched.bytes;
    }
    checkCancelled();
    renameSync(partPath, destPath);
    return bytes;
  } finally {
    try {
      if (downloadPromise) await downloadPromise;
      if (downloadPromise && !nativeDownload) {
        // Chromium may still be sniffing a stalled response before it creates a
        // Download handle. There is no cancelDownload target yet. Before releasing
        // physical admission, close this run's owned context; a timed-out event
        // waiter alone is not proof that its network request has stopped. The
        // persistent profile stays on disk, but its tabs need explicit reopening.
        await page.context().close();
      }
      if (nativeDownload && (!nativeComplete || signal?.aborted)) cancelNative();
      if (cancellation) await cancellation;
    } finally {
      signal?.removeEventListener("abort", cancelNative);
      removePart(partPath);
    }
  }
}
