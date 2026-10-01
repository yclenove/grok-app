import test from "node:test";
import assert from "node:assert/strict";
import { Readable } from "node:stream";
import { mkdtempSync, readFileSync, rmSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { streamToPart } from "./download.mjs";

test("aborting a stalled download stream closes the reader and partial-file writer", async () => {
  const root = mkdtempSync(join(tmpdir(), "cu-download-abort-"));
  const part = join(root, "file.part");
  let sent = false;
  const readable = new Readable({ read() { if (!sent) { sent = true; this.push(Buffer.from("begin")); } } });
  const controller = new AbortController();
  const pending = streamToPart(readable, part, 1024, controller.signal).then(value => ({ value }), error => ({ error }));
  let timer;
  try {
    const deadline = performance.now() + 2000;
    while ((!existsSync(part) || readFileSync(part).length !== 5) && performance.now() < deadline) {
      await new Promise(resolve => setTimeout(resolve, 10));
    }
    assert.equal(readFileSync(part).toString(), "begin", "first chunk must be consumed before cancellation");
    controller.abort();
    const outcome = await Promise.race([pending, new Promise(resolve => { timer = setTimeout(() => resolve("still-blocked"), 250); })]);
    assert.notEqual(outcome, "still-blocked", "abort must interrupt a read waiting for the next chunk");
    assert.ok(outcome.error, "cancelled transfer cannot report success");
    assert.equal(readable.destroyed, true);
  } finally {
    clearTimeout(timer);
    readable.destroy(new Error("owned fixture cleanup"));
    await pending;
    rmSync(root, { recursive: true });
  }
});
