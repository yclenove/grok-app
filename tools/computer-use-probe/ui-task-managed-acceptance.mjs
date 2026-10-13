// Product UI with deliberately synthetic IPC; not a native/App grant proof.
import assert from "node:assert/strict";
import path from "node:path";

export async function runManagedConsentAcceptance({ browser, origin, output, reports }) {
  for (const outcome of ["receipt", "fail-first", "late-approved", "stop", "cancel", "success"]) {
    const context = await browser.newContext({ viewport: { width: 320, height: 800 } });
    const page = await context.newPage();
    const errors = [];
    page.on("pageerror", (error) => errors.push(String(error)));
    await page.route("**/*", (route) => route.request().url().startsWith(origin + "/") || route.request().url().startsWith("data:") ? route.continue() : route.abort());
    try {
      const held = outcome === "stop" || outcome === "cancel";
      const query = new URLSearchParams({ locale: "en", state: "pending", theme: "dark", surface: "managed-browser", managedReply: held ? "hold" : outcome === "late-approved" ? "fail-first" : outcome, page: "card" });
      await page.goto(`${origin}/tools/computer-use-probe/ui-task-consent-fixture.html?${query}`);
      const allow = page.getByRole("button", { name: "Enable and allow this task", exact: true });
      await allow.waitFor();
      await page.getByText("Isolated task browser · current tab only. Your existing browser stays private.", { exact: true }).waitFor();
      let calls = await page.evaluate(() => [...window.__taskUi.calls]);
      assert(!calls.includes("computer_use_task_managed_approve"), "mount cannot create a browser");
      await allow.focus(); await page.keyboard.press("Enter");
      await page.waitForFunction(() => window.__taskUi.calls.includes("computer_use_task_managed_approve"));
      assert.equal(await page.getByRole("dialog").count(), 0, "no fake native picker or extra confirmation");
      if (outcome === "receipt" || held) {
        await page.getByText("Preparing access…", { exact: true }).waitFor();
        assert(await allow.isDisabled(), "a single gesture remains in flight");
        assert(await page.getByRole("button", { name: "Cancel", exact: true }).isEnabled(), "cancel is immediate, before Host preparing");
        assert(await page.getByRole("button", { name: "Stop", exact: true }).isEnabled(), "Stop is available without a run");
        calls = await page.evaluate(() => [...window.__taskUi.calls]);
        assert(!calls.includes("computer_use_task_preview"), "a receipt is not an execution grant");
        await page.screenshot({ path: path.join(output, `managed-${outcome}-preparing.png`), fullPage: true });
        if (held) {
          await page.getByRole("button", { name: outcome === "stop" ? "Stop" : "Cancel", exact: true }).click();
          await page.getByText(outcome === "stop" ? "Stopped" : "Request cancelled", { exact: true }).waitFor();
          await page.evaluate((success) => window.__taskUi.completeManaged(success), outcome === "stop");
          // Serialize a fresh Host event after settling the old reply. The old
          // success/failure must not publish a grant or an alert in this task.
          await page.evaluate(() => window.__taskUi.mode("cancelled"));
          await page.getByText("Request cancelled", { exact: true }).waitFor();
        } else {
          await page.evaluate(() => window.__taskUi.mode("running"));
          await page.getByText("Control authorized", { exact: true }).waitFor();
        }
      } else if (outcome === "fail-first" || outcome === "late-approved") {
        await page.getByRole("alert").getByText("Could not prepare the task browser. Try again.", { exact: true }).waitFor();
        assert(await allow.isEnabled(), "failure is recoverable with a new gesture");
        await page.screenshot({ path: path.join(output, `managed-${outcome}-error.png`), fullPage: true });
        await page.evaluate((mode) => window.__taskUi.mode(mode), outcome === "late-approved" ? "running" : "pending");
        calls = await page.evaluate(() => [...window.__taskUi.calls]);
        assert.equal(calls.filter((name) => name === "computer_use_task_managed_approve").length, 1, "no automatic retry");
        if (outcome === "fail-first") { await allow.focus(); await page.keyboard.press("Enter"); }
        await page.getByText("Control authorized", { exact: true }).waitFor();
      } else {
        await page.getByText("Control authorized", { exact: true }).waitFor();
      }
      calls = await page.evaluate(() => [...window.__taskUi.calls]);
      if (held) assert(!calls.includes("computer_use_task_preview"), "late preparation never restores stopped/cancelled authority");
      assert.equal(calls.filter((name) => name === "computer_use_task_managed_approve").length, outcome === "fail-first" ? 2 : 1);
      assert(!calls.some((name) => /picker_|computer_use_authorize/.test(name)), "managed does not borrow Desktop or legacy authorization");
      assert.equal(await page.getByRole("alert").count(), 0);
      assert.deepEqual(errors, []);
      await page.screenshot({ path: path.join(output, `managed-${outcome}-final.png`), fullPage: true });
      reports.push({ managedConsent: true, outcome, calls, errors, passed: true });
    } finally { await context.close(); }
  }
}
