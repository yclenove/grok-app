// Product modal in isolated Chromium. Host decisions are deliberately synthetic.
import assert from "node:assert/strict";
import path from "node:path";

export async function runExistingConsentAcceptance({ browser, origin, output, reports }) {
  for (const locale of ["de", "en", "es", "fil", "fr", "id", "it", "ja", "ko", "pt-BR", "ru", "ta", "uk", "zh", "zh-TW"]) {
    for (const theme of ["light", "dark"]) {
      const context = await browser.newContext({ viewport: { width: 300, height: 800 } });
      const page = await context.newPage(); const errors = [];
      page.on("pageerror", error => errors.push(String(error)));
      await page.route("**/*", route => route.request().url().startsWith(origin + "/") || route.request().url().startsWith("data:") ? route.continue() : route.abort());
      try {
        const query = new URLSearchParams({ locale, theme, state: "pending", surface: "existing-tabs", existingStage: "pair", page: "card" });
        await page.goto(`${origin}/tools/computer-use-probe/ui-task-consent-fixture.html?${query}`);
        const allow = page.locator(".cu-task-card__actions .btn--solid"); await allow.waitFor();
        let calls = await page.evaluate(() => [...window.__taskUi.calls]);
        assert(!calls.some(name => /picker_|authorize/.test(name)), "mount does not Pair, Share, discover or grant");
        await allow.focus(); await page.keyboard.press("Enter");
        const dialog = page.getByRole("dialog"); await dialog.waitFor();
        const confirm = dialog.locator(".cu-consent__preparation > .btn--solid"); await confirm.waitFor();
        assert.equal(await dialog.locator("input").count(), 0, "one-time code stays hidden until explicit App confirmation");
        await confirm.focus(); await page.keyboard.press("Enter");
        await dialog.locator('[data-testid="cu-pairing-code"]').waitFor();
        assert.equal(await dialog.locator('[data-testid="cu-pairing-code"]').inputValue(), "123456");
        const layout = await page.evaluate(() => {
          const modal = document.querySelector('[role="dialog"]'); const rect = modal.getBoundingClientRect();
          const body = modal.querySelector(".modal-body");
          const copies = [...modal.querySelectorAll(".cu-pairing__copy")].map(button => {
            const box = button.getBoundingClientRect();
            return { x: box.x, right: box.right, width: box.width, height: box.height };
          });
          return { viewport: innerWidth, documentWidth: document.documentElement.scrollWidth, scrollWidth: modal.scrollWidth, width: rect.width, x: rect.x, background: getComputedStyle(modal).backgroundColor,
            bodyWidth: body.clientWidth, bodyScrollWidth: body.scrollWidth, copies };
        });
        assert(layout.documentWidth <= 301 && layout.scrollWidth <= layout.width + 1 && layout.x >= 0 && layout.x + layout.width <= 301, "modal has no narrow-screen horizontal overflow");
        assert(layout.bodyScrollWidth <= layout.bodyWidth + 1 && layout.copies.length === 2
          && layout.copies.every(box => box.width >= 24 && box.height >= 24 && box.x >= layout.x && box.right <= layout.x + layout.width),
        `both copy controls remain inside the visible modal body: ${JSON.stringify(layout)}`);
        assert(!["transparent", "rgba(0, 0, 0, 0)"].includes(layout.background), "modal has an opaque surface");
        await page.screenshot({ path: path.join(output, `existing-pair-${locale}-${theme}.png`), fullPage: true });
        await page.keyboard.press("Escape"); await dialog.waitFor({ state: "hidden" });
        await page.waitForFunction(() => document.activeElement?.matches(".cu-task-card__actions .btn--solid"));
        assert(await allow.evaluate(element => element === document.activeElement), "Escape restores original trigger");
        calls = await page.evaluate(() => [...window.__taskUi.calls]);
        assert.equal(calls.filter(name => name === "computer_use_task_picker_begin").length, 1);
        assert.equal(calls.filter(name => name === "computer_use_task_picker_pairing_confirm").length, 1);
        assert.equal(calls.filter(name => name === "computer_use_task_picker_cancel").length, 1);
        assert(!calls.some(name => /picker_load|picker_approve|computer_use_begin_pairing|computer_use_confirm_pairing_app|computer_use_authorize/.test(name)));
        assert.deepEqual(errors, []);
        reports.push({ existingConsent: true, locale, theme, layout, calls, passed: true });
      } finally { await context.close(); }
    }
  }
  for (const scenario of ["shared", "race", "busy", "fail", "stop", "cancel"]) {
    const context = await browser.newContext({ viewport: { width: 320, height: 800 }, deviceScaleFactor: 1.5 });
    const page = await context.newPage(); const errors = [];
    page.on("pageerror", error => errors.push(String(error)));
    await page.route("**/*", route => route.request().url().startsWith(origin + "/") || route.request().url().startsWith("data:") ? route.continue() : route.abort());
    try {
      const held = ["stop", "cancel"].includes(scenario);
      await page.goto(`${origin}/tools/computer-use-probe/ui-task-consent-fixture.html?${new URLSearchParams({ locale: "en", theme: "dark", surface: "existing-tabs", existingStage: held ? "hold" : scenario, page: "card" })}`);
      await page.locator(".cu-task-card__actions .btn--solid").click();
      const dialog = page.getByRole("dialog"); await dialog.waitFor();
      if (held) {
        const control = dialog.locator(".modal-actions").getByRole("button", { name: scenario === "stop" ? "Stop" : "Cancel", exact: true });
        assert(await control.isEnabled(), "cancel and Stop never wait for metadata"); await control.click();
        await dialog.waitFor({ state: "hidden" }); await page.evaluate(() => window.__taskUi.completeExisting());
        await page.evaluate(() => window.__taskUi.mode("cancelled"));
        await page.getByText("Request cancelled", { exact: true }).waitFor();
      } else if (scenario === "busy") {
        await dialog.getByText("Tabs in use by another task: 2. Wait for it to finish.", { exact: true }).waitFor();
        await dialog.getByText("Tabs needing an extension update: 1. Update and share again.", { exact: true }).waitFor();
        assert(await dialog.locator(".modal-actions .btn--solid").isDisabled());
      } else {
        if (scenario === "fail") {
          await dialog.getByRole("alert").waitFor(); await dialog.getByRole("button", { name: "Check shared tabs", exact: true }).click();
          await dialog.getByRole("button", { name: "Confirm pairing in App" }).waitFor();
          await page.evaluate(() => window.__taskUi.share());
        }
        if (scenario === "race") await dialog.getByText("No selectable shared tabs. Share a tab and check again.", { exact: true }).waitFor();
        if (scenario !== "shared") await dialog.getByRole("button", { name: "Check shared tabs", exact: true }).click();
        const candidate = dialog.getByRole("button", { name: "Shared fixture tab Chrome", exact: true }); await candidate.waitFor();
        assert(await dialog.locator(".modal-actions .btn--solid").isDisabled(), "no automatic candidate selection");
        await candidate.click(); await dialog.locator(".modal-actions .btn--solid").click();
        await page.getByText("Control authorized", { exact: true }).waitFor();
      }
      const calls = await page.evaluate(() => [...window.__taskUi.calls]);
      assert.equal(calls.filter(name => name === "computer_use_task_picker_begin").length, 1, "empty retry does not create new authority");
      assert.equal(calls.filter(name => name === "computer_use_task_picker_approve").length, held || scenario === "busy" ? 0 : 1);
      if (held || scenario === "busy") assert(!calls.includes("computer_use_task_picker_load"));
      assert(!calls.some(name => /computer_use_begin_pairing|computer_use_confirm_pairing_app|computer_use_authorize/.test(name)));
      assert.deepEqual(errors, []);
      await page.screenshot({ path: path.join(output, `existing-${scenario}-final.png`), fullPage: true });
      reports.push({ existingConsent: true, scenario, calls, passed: true });
    } finally { await context.close(); }
  }
  for (const confirmed of [false, true]) {
    const context = await browser.newContext({ viewport: { width: 320, height: 800 } });
    const page = await context.newPage();
    try {
      await context.grantPermissions(["clipboard-read", "clipboard-write"], { origin });
      await page.goto(`${origin}/tools/computer-use-probe/ui-task-consent-fixture.html?surface=existing-tabs&locale=en&page=card`);
      await page.locator(".cu-task-card__actions .btn--solid").click();
      const dialog = page.getByRole("dialog"); const confirm = dialog.getByRole("button", { name: "Confirm pairing in App" });
      await confirm.waitFor();
      if (confirmed) await confirm.click();
      const target = confirmed ? dialog.getByRole("button", { name: "Copy App address" }) : confirm;
      await target.waitFor(); await target.focus();
      if (confirmed) { await target.click(); await dialog.getByText("Copied", { exact: true }).waitFor(); }
      await page.evaluate(() => window.__taskUi.holdMetadata());
      const reads = await page.evaluate(() => window.__taskUi.calls.filter(name => name === "computer_use_task_picker_preparation").length);
      await page.waitForFunction(previous => window.__taskUi.calls.filter(name => name === "computer_use_task_picker_preparation").length > previous, reads);
      // Hold the real browser pending for longer than two poll intervals.
      // This is deliberate synthetic latency, not a native App timing claim.
      await page.waitForTimeout(2200);
      assert(await target.isEnabled()); assert(await target.evaluate(node => document.activeElement === node));
      if (confirmed) { assert(await dialog.getByText("Copied", { exact: true }).isVisible()); await target.click(); }
      else { await page.keyboard.press("Enter"); assert.equal(await confirm.getAttribute("aria-disabled"), "true"); }
      await page.evaluate(() => window.__taskUi.releaseMetadata());
      if (!confirmed) { await dialog.getByTestId("cu-pairing-code").waitFor(); assert(await dialog.getByLabel("App address", { exact: true }).evaluate(node => node === document.activeElement)); }
      else assert(await target.evaluate(node => document.activeElement === node));
      const calls = await page.evaluate(() => [...window.__taskUi.calls]);
      assert.equal(calls.filter(name => name === "computer_use_task_picker_pairing_confirm").length, 1);
      assert(!calls.some(name => /picker_load|picker_approve/.test(name)));
      await page.screenshot({ path: path.join(output, `existing-slow-${confirmed ? "copy" : "confirm"}.png`), fullPage: true });
      reports.push({ existingConsent: true, slowMetadataMs: 2200, confirmed, calls, passed: true });
    } finally { await context.close(); }
  }
}
