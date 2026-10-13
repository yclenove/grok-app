// Real product chrome, deliberately synthetic Host export replies. No browser install.
import assert from "node:assert/strict";
import path from "node:path";
export async function runExtensionExportAcceptance({ browser, origin, output, reports }) {
  for (const locale of ["de", "en", "es", "fil", "fr", "id", "it", "ja", "ko", "pt-BR", "ru", "ta", "uk", "zh", "zh-TW"]) {
    for (const theme of ["light", "dark"]) {
      const context = await browser.newContext({ viewport: { width: 300, height: 800 }, deviceScaleFactor: 1.5 });
      const page = await context.newPage(); const errors = [];
      page.on("pageerror", error => errors.push(String(error)));
      await page.route("**/*", route => route.request().url().startsWith(origin + "/") || route.request().url().startsWith("data:") ? route.continue() : route.abort());
      try {
        await page.goto(`${origin}/tools/computer-use-probe/ui-task-consent-fixture.html?${new URLSearchParams({ surface: "existing-tabs", locale, theme, page: "card" })}`);
        await page.locator(".cu-task-card__actions .btn--solid").click();
        const setup = page.getByRole("dialog").locator(".cu-extension-setup"); await setup.locator("summary").click();
        const get = setup.locator(".cu-extension-setup__body > .btn"); await get.waitFor(); await get.focus(); await page.keyboard.press("Enter");
        const field = setup.locator(".cu-extension-setup__path .input"); await field.waitFor();
        assert.equal(await field.inputValue(), "H:/Synthetic App data/browser-extension/owned-bundle");
        assert(await field.evaluate(node => node === document.activeElement));
        assert.equal(await setup.locator(".cu-extension-setup__steps li").count(), 2);
        assert((await setup.locator(".cu-extension-setup__identity").innerText()).includes("bgegbabkegkanjbmjbeaockdijnbkjgi"));
        const layout = await setup.evaluate(node => ({ width: node.clientWidth, content: node.scrollWidth, body: node.querySelector(".cu-extension-setup__body").scrollWidth, viewport: innerWidth, document: document.documentElement.scrollWidth }));
        assert(layout.content <= layout.width + 1 && layout.body <= layout.width + 1 && layout.document <= layout.viewport + 1, "extension steps/path must fit inside the narrow modal");
        await setup.locator(".cu-panel__actions .btn").last().click();
        const calls = await page.evaluate(() => [...window.__taskUi.calls]);
        assert.equal(calls.filter(name => name === "computer_use_extension_info").length, 1);
        assert.equal(calls.filter(name => name === "computer_use_extension_export").length, 1);
        assert.equal(calls.filter(name => name === "computer_use_extension_open").length, 1);
        assert(!calls.some(name => /pairing_confirm|picker_load|picker_approve|authorize/.test(name)), "export never confirms/pairs/shares/executes");
        assert.deepEqual(errors, []);
        await page.screenshot({ path: path.join(output, `extension-export-${locale}-${theme}.png`), fullPage: true });
        reports.push({ extensionExport: true, synthetic: true, locale, theme, layout, calls, passed: true });
      } finally { await context.close(); }
    }
  }
  for (const scenario of ["fail-first", "stop", "cancel"]) {
    const context = await browser.newContext({ viewport: { width: 320, height: 800 } }); const page = await context.newPage();
    try {
      await page.goto(`${origin}/tools/computer-use-probe/ui-task-consent-fixture.html?surface=existing-tabs&locale=en&page=card&extensionReply=${scenario === "fail-first" ? scenario : "hold"}`);
      await page.locator(".cu-task-card__actions .btn--solid").click();
      const dialog = page.getByRole("dialog"); await dialog.locator(".cu-extension-setup summary").click();
      const get = dialog.getByRole("button", { name: "Get bundled extension" }); await get.waitFor(); await get.click();
      if (scenario === "fail-first") {
        await dialog.getByRole("alert").waitFor(); await get.click(); await dialog.getByLabel("Extension folder").waitFor();
      } else {
        await dialog.getByText("Preparing extension files…", { exact: true }).waitFor();
        const end = dialog.locator(".modal-actions").getByRole("button", { name: scenario === "stop" ? "Stop" : "Cancel", exact: true });
        assert(await end.isEnabled()); await end.click(); await dialog.waitFor({ state: "hidden" });
        await page.evaluate(() => window.__taskUi.completeExport());
        assert.equal(await page.getByLabel("Extension folder").count(), 0);
      }
      const calls = await page.evaluate(() => [...window.__taskUi.calls]);
      assert.equal(calls.filter(name => name === "computer_use_extension_export").length, scenario === "fail-first" ? 2 : 1);
      assert(!calls.some(name => /extension_open|pairing_confirm|picker_approve/.test(name)));
      reports.push({ extensionExport: true, synthetic: true, scenario, calls, passed: true });
    } finally { await context.close(); }
  }
}
