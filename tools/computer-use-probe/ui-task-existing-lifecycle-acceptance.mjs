// Real component/browser lifecycle; Share, visibility and Host IPC are synthetic.
import assert from "node:assert/strict";
import path from "node:path";

export async function runExistingLifecycleAcceptance({ browser, origin, output, reports }) {
  for (const scenario of ["unshare-reshare", "hidden-queued-confirm", "card-to-panel"]) {
    const context = await browser.newContext({ viewport: { width: 320, height: 800 }, deviceScaleFactor: 1.5 });
    const page = await context.newPage(); const errors = [];
    page.on("pageerror", error => errors.push(String(error)));
    await page.route("**/*", route => route.request().url().startsWith(origin + "/") || route.request().url().startsWith("data:") ? route.continue() : route.abort());
    try {
      const query = new URLSearchParams({ surface: "existing-tabs", locale: "en", theme: "dark", page: scenario === "card-to-panel" ? "both" : "card" });
      if (scenario === "unshare-reshare") { query.set("existingStage", "shared"); query.set("holdLoad", "1"); }
      await page.goto(`${origin}/tools/computer-use-probe/ui-task-consent-fixture.html?${query}`);
      const initial = scenario === "card-to-panel" ? page.locator(".fixture-card .cu-task-card__actions .btn--solid")
        : page.locator(".cu-task-card__actions .btn--solid");
      await initial.click();
      const dialog = page.getByRole("dialog"); await dialog.waitFor();
      if (scenario === "unshare-reshare") {
        await page.waitForFunction(() => window.__taskUi.calls.includes("computer_use_task_picker_load"));
        // Withdraw after readiness, before the original discovery reply. Neither
        // an empty receipt nor its retry may create a new request or authority.
        await page.evaluate(() => { window.__taskUi.unshare(); window.__taskUi.completeLoad(); });
        await dialog.getByText("No selectable shared tabs. Share a tab and check again.", { exact: true }).waitFor();
        assert(await dialog.locator(".modal-actions .btn--solid").isDisabled());
        await page.evaluate(() => window.__taskUi.share());
        await dialog.getByRole("button", { name: "Check shared tabs", exact: true }).click();
        const candidate = dialog.getByRole("button", { name: "Shared fixture tab Chrome", exact: true }); await candidate.waitFor();
        assert.equal(await candidate.getAttribute("aria-pressed"), "false");
        await candidate.click(); await dialog.locator(".modal-actions .btn--solid").click();
        await page.getByText("Control authorized", { exact: true }).waitFor();
      } else {
        const confirm = dialog.getByRole("button", { name: "Confirm pairing in App" }); await confirm.waitFor();
        await page.evaluate(() => window.__taskUi.holdMetadata());
        const reads = await page.evaluate(() => window.__taskUi.calls.filter(name => name === "computer_use_task_picker_preparation").length);
        await page.waitForFunction(previous => window.__taskUi.calls.filter(name => name === "computer_use_task_picker_preparation").length > previous, reads);
        if (scenario === "hidden-queued-confirm") {
          await confirm.focus(); await page.keyboard.press("Enter");
          assert.equal(await confirm.getAttribute("aria-disabled"), "true");
          // Deterministic injected visibility signal; not evidence of native
          // browser activation or a real user's tab-sharing gesture.
          await page.evaluate(() => {
            Object.defineProperty(document, "hidden", { configurable: true, value: true });
            document.dispatchEvent(new Event("visibilitychange"));
            window.__taskUi.share(); window.__taskUi.releaseMetadata();
          });
          await page.waitForTimeout(1100);
          const hiddenCalls = await page.evaluate(() => [...window.__taskUi.calls]);
          assert(!hiddenCalls.some(name => /pairing_confirm|picker_load|picker_approve/.test(name)), "hidden reply cannot confirm/discover/approve");
          assert.equal(await dialog.getByTestId("cu-pairing-code").count(), 0);
          await page.evaluate(() => { Reflect.deleteProperty(document, "hidden"); document.dispatchEvent(new Event("visibilitychange")); });
          await dialog.getByRole("button", { name: "Shared fixture tab Chrome", exact: true }).waitFor();
        } else {
          await page.evaluate(() => window.__taskUi.closeCard());
          await dialog.waitFor({ state: "hidden" });
          await page.waitForFunction(() => window.__taskUi.calls.includes("computer_use_task_picker_cancel"));
          const panel = page.locator(".fixture-panel .cu-task-card__actions .btn--solid");
          await panel.click(); await dialog.waitFor();
          await dialog.getByRole("button", { name: "Confirm pairing in App" }).waitFor();
          await page.evaluate(() => { window.__taskUi.share(); window.__taskUi.releaseMetadata(); });
          await dialog.getByRole("button", { name: "Shared fixture tab Chrome", exact: true }).waitFor();
          assert.equal(await dialog.getByTestId("cu-pairing-code").count(), 0, "new UI owner never adopts old code/gesture");
        }
      }
      const { calls, pickerCalls } = await page.evaluate(() => ({ calls: [...window.__taskUi.calls], pickerCalls: window.__taskUi.pickerCalls }));
      const starts = calls.filter(name => name === "computer_use_task_picker_begin").length;
      assert.equal(starts, scenario === "card-to-panel" ? 2 : 1);
      assert.equal(calls.filter(name => name === "computer_use_task_subscribe").length, 1, "card/panel keep one original metadata stream");
      assert.equal(calls.filter(name => name === "computer_use_task_picker_pairing_confirm").length, 0);
      assert.equal(calls.filter(name => name === "computer_use_task_picker_approve").length, scenario === "unshare-reshare" ? 1 : 0);
      const loads = pickerCalls.filter(call => call.command === "computer_use_task_picker_load");
      assert(loads.length > 0 && loads.every(call => call.payload.pickerId === `synthetic-ui-picker-${starts}`), "late former owner cannot discover for the new picker");
      if (scenario === "unshare-reshare") {
        assert(loads.length >= 2);
        const approval = pickerCalls.find(call => call.command === "computer_use_task_picker_approve");
        assert.deepEqual(approval.payload.candidateIds, ["synthetic-shared-tab@reshared-doc"]);
      }
      if (scenario === "card-to-panel") {
        const cancels = pickerCalls.filter(call => call.command === "computer_use_task_picker_cancel");
        assert.equal(cancels.length, 1); assert.equal(cancels[0].payload.pickerId, "synthetic-ui-picker-1");
      }
      assert(!calls.some(name => /computer_use_begin_pairing|computer_use_confirm_pairing_app|computer_use_authorize/.test(name)));
      assert.deepEqual(errors, []);
      await page.screenshot({ path: path.join(output, `existing-lifecycle-${scenario}.png`), fullPage: true });
      reports.push({ existingLifecycle: true, synthetic: true, scenario, calls, pickerCalls, passed: true });
    } finally { await context.close(); }
  }
}
