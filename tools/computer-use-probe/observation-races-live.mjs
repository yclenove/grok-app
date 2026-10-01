// Real document results are held after Chrome executes, then invalidated by an
// independent browser event. No product result/permission/clock is fabricated.
import assert from "node:assert/strict";

export async function verifyObservationRaces({ worker, fixture, unrelated, popup, rpc, until, share, stage, passed }) {
  for (const mode of ["tab-switch", "scroll", "unshare", "reload", "script-timeout", "deadline"]) {
    stage(`observation-race-${mode}`);
    if ((await rpc("shared")).length === 0) await share();
    const [candidate] = await rpc("shared");
    await worker.evaluate(mode => {
      globalThis.__cuPauseObservation = true;
      globalThis.__cuPauseResult = mode === "deadline";
    }, mode);
    const requestedAt = Date.now();
    const observing = rpc("observe-shared", { selector: candidate.id });
    await until(() => worker.evaluate(() => typeof globalThis.__cuReleaseObservation === "function"));
    const invalidatedAt = Date.now();
    if (mode === "tab-switch") { await unrelated.bringToFront(); await fixture.bringToFront(); }
    if (mode === "scroll") {
      await fixture.evaluate(() => { document.body.style.minHeight = "3000px"; scrollTo(0, 200); });
      await fixture.waitForFunction(() => scrollY === 200);
      await fixture.evaluate(() => scrollTo(0, 0));
      await fixture.waitForFunction(() => scrollY === 0);
    }
    if (mode === "unshare") await popup.locator("#unshare-current").click();
    if (mode === "reload") await fixture.reload();
    let reply;
    if (["deadline", "script-timeout", "unshare", "reload"].includes(mode)) {
      if (mode === "deadline") {
        // Hold the real rejection's HTTP dispatch as well: otherwise the script's
        // 5s timeout completes the Host early and never exercises its own 10s TTL.
        await until(() => worker.evaluate(() => typeof globalThis.__cuReleaseResult === "function"));
      }
      reply = await observing;
      if (mode === "deadline") assert(Date.now() - requestedAt >= 9500, "Host monotonic deadline must actually elapse");
      else if (mode === "script-timeout") assert(Date.now() - requestedAt >= 4500, "script timeout must actually elapse");
      else assert(Date.now() - invalidatedAt < 2000, "Host revocation must not wait for the browser's deadline");
    }
    await worker.evaluate(() => {
      globalThis.__cuReleaseObservation(); delete globalThis.__cuReleaseObservation;
      globalThis.__cuReleaseResult?.(); delete globalThis.__cuReleaseResult;
    });
    reply ??= await observing;
    assert.equal(reply.ok, false, `${mode} must not release old content to the Host consumer`);
    assert(!Object.hasOwn(reply, "observation"));
    process.stderr.write(`observation race: ${mode}, staleResult=rejected, elapsedMs=${Date.now() - invalidatedAt}\n`);
    assert.equal((await rpc("status")).paired, true);
    assert(!fixture.isClosed() && !unrelated.isClosed(), "cancellation must not close user tabs");
    if (mode === "unshare" || mode === "reload") {
      await until(async () => (await rpc("shared")).length === 0);
      await share();
    }
    passed(`observation-race-${mode}`);
  }
}
