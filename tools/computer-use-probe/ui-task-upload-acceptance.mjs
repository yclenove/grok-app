// Real Chromium renders product UI; file choice and Host IPC are synthetic.
import assert from "node:assert/strict";
import path from "node:path";

export async function runUploadConsentAcceptance({ browser, origin, output, reports }) {
  for (const locale of ["en", "zh", "zh-TW", "de", "es", "fil", "fr", "id", "it", "ja", "ko", "pt-BR", "ru", "ta", "uk"]) {
    for (const theme of ["light", "dark"]) {
      const context = await browser.newContext({ viewport: { width: 300, height: 800 } });
      try {
        const page = await context.newPage();
        const errors = [];
        page.on("pageerror", error => errors.push(String(error)));
        await page.route("**/*", route => route.request().url().startsWith(origin + "/") || route.request().url().startsWith("data:") ? route.continue() : route.abort());
        const query = new URLSearchParams({ locale, theme, state: "running", upload: "1", surface: "app-webview", page: "card", long: "1" });
        await page.goto(`${origin}/tools/computer-use-probe/ui-task-consent-fixture.html?${query}`);
        const choose = page.locator(".cu-upload .btn--solid");
        await choose.waitFor();
        assert(await choose.isEnabled());
        const calls = await page.evaluate(() => [...window.__taskUi.calls]);
        assert(!calls.some(name => name.includes("_upload")), "mount cannot choose or cancel files");
        const layout = await page.evaluate(() => ({ width: innerWidth, document: document.documentElement.scrollWidth,
          upload: document.querySelector(".cu-upload").scrollWidth, box: document.querySelector(".cu-upload").getBoundingClientRect().width }));
        assert(layout.document <= layout.width + 1 && layout.upload <= layout.box + 1, "localized upload has no horizontal overflow");
        await page.screenshot({ path: path.join(output, `upload-${locale}-${theme}-300.png`), fullPage: true });
        assert.deepEqual(errors, []);
        reports.push({ uploadLayout: true, locale, theme, width: 300, layout, syntheticFileChoice: true, passed: true });
      } finally { await context.close(); }
    }
  }
  for (const outcome of ["verified", "rejected", "unknown", "pause", "stop", "cancel"]) {
    const context = await browser.newContext({ viewport: { width: 900, height: 800 } });
    try {
      const page = await context.newPage();
      const errors = [];
      page.on("pageerror", error => errors.push(String(error)));
      await page.route("**/*", route => route.request().url().startsWith(origin + "/") || route.request().url().startsWith("data:") ? route.continue() : route.abort());
      await page.goto(`${origin}/tools/computer-use-probe/ui-task-consent-fixture.html?locale=en&theme=dark&state=running&upload=1&surface=app-webview&page=both`);
      const choose = page.getByRole("button", { name: "Choose and upload", exact: true });
      await choose.first().waitFor();
      assert.equal(await choose.count(), 2);
      await choose.first().focus(); await page.keyboard.press("Enter");
      await page.getByText("Choosing a file…", { exact: true }).first().waitFor();
      assert.equal(await choose.count(), 0, "both views read the same selecting phase");
      if (outcome === "pause" || outcome === "stop") {
        await page.getByRole("button", { name: outcome === "pause" ? "Pause" : "Stop", exact: true }).first().click();
      } else if (outcome === "cancel") {
        await page.getByRole("button", { name: "Cancel", exact: true }).first().click();
        await page.getByText("File selection cancelled", { exact: true }).first().waitFor();
        await page.getByText("Close the file dialog so cleanup can finish.", { exact: true }).first().waitFor();
      }
      await page.evaluate(kind => window.__uploadUi.finish(kind), ["verified", "unknown"].includes(outcome) ? outcome : "rejected");
      if (outcome === "verified") await page.getByText("File assigned · website submission not confirmed.", { exact: true }).first().waitFor();
      else if (outcome === "unknown") await page.locator(".cu-upload [role=alert]").first().waitFor();
      else if (["cancel", "rejected"].includes(outcome)) await page.getByText("File selection cancelled", { exact: true }).first().waitFor();
      else assert.equal(await page.getByText("File assigned · website submission not confirmed.", { exact: true }).count(), 0);
      const calls = await page.evaluate(() => [...window.__taskUi.calls]);
      assert.equal(calls.filter(name => name === "computer_use_task_choose_upload").length, 1, "one original chooser across both views");
      assert.deepEqual(errors, []);
      await page.screenshot({ path: path.join(output, `upload-outcome-${outcome}.png`), fullPage: true });
      reports.push({ uploadOutcome: outcome, calls, syntheticFileChoice: true, passed: true });
    } finally { await context.close(); }
  }
}
