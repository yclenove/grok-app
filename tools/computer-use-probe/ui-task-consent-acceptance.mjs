// Isolated Chromium renders real product components. Synthetic IPC is not
// evidence of App/ACP/native authorization, platform readiness or human consent.
import assert from "node:assert/strict";
import path from "node:path";
import { copyFile, lstat, mkdir, readdir, readFile, rm, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { fileURLToPath } from "node:url";
import { build, preview } from "vite";
import { chromium } from "../computer-use-browser/node_modules/playwright-core/index.mjs";
import { pinnedChromeLaunchPath } from "../computer-use-browser/immutable-chrome.mjs";
import { runManagedConsentAcceptance } from "./ui-task-managed-acceptance.mjs";
import { runExistingConsentAcceptance } from "./ui-task-existing-acceptance.mjs";
import { runExistingLifecycleAcceptance } from "./ui-task-existing-lifecycle-acceptance.mjs";
import { runExtensionExportAcceptance } from "./ui-extension-export-acceptance.mjs";
import { runUploadConsentAcceptance } from "./ui-task-upload-acceptance.mjs";

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const output = path.join(repo, "tools/computer-use-probe/.run", `task-consent-ui-${new Date().toISOString().replace(/[:.]/g, "-")}`);
await mkdir(output, { recursive: true });
const cases = [];
for (const width of [300, 320, 900]) for (const theme of ["light", "dark"]) for (const locale of ["en", "zh", "de", "ru"]) {
  cases.push({ width, theme, locale, state: width === 900 ? "running" : "pending", page: "panel", dpr: 1, long: "1" });
}
for (const dpr of [1.25, 1.5, 2]) cases.push({ width: 320, theme: "dark", locale: "zh", state: "pending", page: "panel", dpr });
for (const state of ["preparing", "paused", "resuming", "target_closed", "stop_requested", "stopped", "error", "expired", "unavailable"]) {
  cases.push({ width: 320, theme: "light", locale: "en", state, page: "panel", dpr: 1 });
}
cases.push({ width: 320, theme: "dark", locale: "zh", state: "pending", page: "card", dpr: 1, long: "1" });
for (const locale of ["de", "en", "es", "fil", "fr", "id", "it", "ja", "ko", "pt-BR", "ru", "ta", "uk", "zh", "zh-TW"]) {
  for (const theme of ["light", "dark"]) cases.push({ width: 300, theme, locale, state: "pending", page: "panel", dpr: 1, surface: "managed-browser" });
}
for (const dpr of [1.25, 1.5, 2]) cases.push({ width: 320, theme: "dark", locale: "zh", state: "pending", page: "card", dpr, surface: "managed-browser" });
for (const locale of ["en", "de", "ta", "zh"]) for (const theme of ["light", "dark"]) {
  cases.push({ width: 300, theme, locale, state: "pending", page: "panel", dpr: 1, surface: "app-webview" });
}
const reports = [];
let server, browser;
// Discard only this run's compiled site and writable browser copy, after both
// owners close. Keep screenshots, failure/report, logs, stamps and file hashes.
async function cleanupBuilds() {
  const artifacts = [];
  for (const leaf of ["site", "browser-runtime"]) {
    const directory = path.resolve(output, leaf);
    assert.equal(path.dirname(directory), output);
    const files = [];
    async function inventory(current) {
      const metadata = await lstat(current);
      assert(!metadata.isSymbolicLink(), "never clean a linked artifact");
      if (metadata.isDirectory()) {
        for (const name of await readdir(current)) await inventory(path.join(current, name));
      } else {
        assert(metadata.isFile(), "only generated files may be cleaned");
        const relative = path.relative(directory, current);
        files.push({ path: relative, bytes: metadata.size, sha256: createHash("sha256").update(await readFile(current)).digest("hex") });
        if (relative.endsWith(".log") || relative === "source-stamp.txt") {
          const saved = path.join(output, "artifact-logs", leaf, relative);
          await mkdir(path.dirname(saved), { recursive: true });
          await copyFile(current, saved);
        }
      }
    }
    try { await inventory(directory); }
    catch (error) { if (error.code === "ENOENT") continue; throw error; }
    artifacts.push({ leaf, files });
  }
  await writeFile(path.join(output, "build-artifacts.json"), JSON.stringify({ artifacts }, null, 2) + "\n");
  for (const { leaf } of artifacts) await rm(path.join(output, leaf), { recursive: true });
  await writeFile(path.join(output, "cleanup.json"), JSON.stringify({ removed: artifacts.map(({ leaf }) => leaf), bytes: artifacts.reduce((sum, { files }) => sum + files.reduce((size, file) => size + file.bytes, 0), 0), ownersClosed: true }, null, 2) + "\n");
}
try {
  const site = path.join(output, "site");
  await build({ root: repo, configFile: path.join(repo, "vite.config.ts"), build: { outDir: site, emptyOutDir: false, target: "esnext", rollupOptions: { input: path.join(repo, "tools/computer-use-probe/ui-task-consent-fixture.html") } } });
  server = await preview({ root: repo, configFile: false, build: { outDir: site }, preview: { host: "127.0.0.1", port: 0, open: false } });
  const origin = `http://127.0.0.1:${server.httpServer.address().port}`;
  const executablePath = pinnedChromeLaunchPath(path.join(repo, "src-tauri/resources/computer-use/seed/chromium/chrome-win/chrome.exe"), path.join(output, "browser-runtime"));
  browser = await chromium.launch({ headless: true, executablePath });
  for (let index = 0; index < cases.length; index++) {
    const item = cases[index];
    const context = await browser.newContext({ viewport: { width: item.width, height: 800 }, deviceScaleFactor: item.dpr });
    const page = await context.newPage();
    const errors = [];
    page.on("pageerror", (error) => errors.push(String(error)));
    await page.route("**/*", (route) => route.request().url().startsWith(origin + "/") || route.request().url().startsWith("data:") ? route.continue() : route.abort());
    try {
      await page.goto(`${origin}/tools/computer-use-probe/ui-task-consent-fixture.html?${new URLSearchParams(item)}`);
      await page.locator(".cu-panel,.cu-task-card").waitFor();
      await page.waitForFunction(() => window.__taskUi?.calls.includes("computer_use_task_subscribe"));
      await page.waitForFunction(() => document.querySelector(".cu-task-card__purpose") !== null);
      const stop = page.locator(".cu-panel__stop");
      if (!["stopped", "expired"].includes(item.state)) {
        assert(await stop.isVisible(), "Stop remains visible");
        const box = await stop.boundingBox();
        assert(box && box.x >= 0 && box.y >= 0 && box.x + box.width <= item.width + 1 && box.y + box.height <= 801, "Stop is inside the viewport");
      }
      const layout = await page.evaluate(() => ({ documentWidth: document.documentElement.scrollWidth, viewport: innerWidth, panelWidth: document.querySelector(".cu-panel,.cu-task-card").scrollWidth }));
      assert(layout.documentWidth <= item.width + 1 && layout.panelWidth <= item.width + 1, "no horizontal overflow");
      const calls = await page.evaluate(() => [...window.__taskUi.calls]);
      assert.equal(calls.filter((name) => name === "computer_use_task_subscribe").length, 1);
      assert(!calls.some((name) => /picker_|managed_approve|authorize|request_access|computer_use_status/.test(name)), "opening UI has no permission/discovery side effects");
      if (item.state !== "running") assert(!calls.includes("computer_use_task_preview"), "inactive UI never captures");
      if (item.state === "unavailable") {
        assert(await page.getByRole("button", { name: "Select target" }).isDisabled(), "unsupported selection cannot approve");
        await page.getByText("Target selection for this surface is not available yet.", { exact: true }).waitFor();
        assert(await page.getByRole("button", { name: "Deny", exact: true }).isEnabled());
      }
      assert.deepEqual(errors, []);
      const screenshot = `${String(index).padStart(2, "0")}-${item.page}-${item.state}-${item.width}-${item.locale}-${item.theme}-${item.dpr}.png`;
      await page.screenshot({ path: path.join(output, screenshot), fullPage: true });
      reports.push({ ...item, screenshot, layout, calls, errors, passed: true });
    } finally { await context.close(); }
  }
  // Keyboard-only picker, focus return and exact synthetic approval/Stop.
  const context = await browser.newContext({ viewport: { width: 900, height: 800 } });
  try {
    const page = await context.newPage();
    await page.goto(`${origin}/tools/computer-use-probe/ui-task-consent-fixture.html?locale=en&state=pending&theme=dark`);
    const choose = page.getByRole("button", { name: "Choose windows for this task" });
    await choose.focus(); await page.keyboard.press("Enter");
    await page.getByRole("dialog").waitFor();
    await page.screenshot({ path: path.join(output, "keyboard-picker.png") });
    await page.keyboard.press("Escape");
    await page.getByRole("dialog").waitFor({ state: "hidden" });
    await page.waitForFunction(() => document.activeElement?.textContent === "Choose windows for this task");
    assert(await choose.evaluate((element) => element === document.activeElement), "Escape restores the initiating focus");
    await page.keyboard.press("Enter");
    await page.getByRole("button", { name: "Research editor Owned fixture" }).click();
    await page.getByRole("dialog").getByRole("button", { name: "Enable and allow this task" }).click();
    await page.getByText("Control authorized", { exact: true }).waitFor();
    await page.getByRole("button", { name: "Stop", exact: true }).click();
    await page.getByText("Stopped", { exact: true }).waitFor();
    reports.push({ keyboardPickerAndControl: true, passed: true });
  } finally { await context.close(); }
  // The launcher describes selection; only the modal's final button grants the
  // chosen App pages. This guards the two-step wording that confused the live user.
  const appContext = await browser.newContext({ viewport: { width: 320, height: 800 } });
  try {
    const page = await appContext.newPage();
    await page.goto(`${origin}/tools/computer-use-probe/ui-task-consent-fixture.html?locale=en&state=pending&theme=dark&surface=app-webview`);
    const choose = page.getByRole("button", { name: "Choose App pages for this task", exact: true });
    await choose.waitFor();
    assert.equal(await choose.getAttribute("aria-haspopup"), "dialog");
    await choose.click();
    const dialog = page.getByRole("dialog", { name: "Choose App pages for this task" });
    await dialog.waitFor();
    const approve = dialog.getByRole("button", { name: "Enable and allow this task", exact: true });
    assert(await approve.isDisabled(), "opening selection is not approval");
    await dialog.getByRole("button", { name: "App page A Grok", exact: true }).click();
    await dialog.getByRole("button", { name: "App page B Grok", exact: true }).click();
    assert(await approve.isEnabled(), "an explicit selection enables final approval");
    await approve.click();
    await page.getByText("Control authorized", { exact: true }).waitFor();
    const calls = await page.evaluate(() => [...window.__taskUi.calls]);
    assert.equal(calls.filter((name) => name === "computer_use_task_picker_approve").length, 1);
    reports.push({ appWebviewSelectionWording: true, calls, passed: true });
  } finally { await appContext.close(); }
  for (const interrupted of [false, true]) {
    const context = await browser.newContext({ viewport: { width: 320, height: 800 } });
    try {
      const page = await context.newPage();
      await page.goto(`${origin}/tools/computer-use-probe/ui-task-consent-fixture.html?locale=en&state=paused&theme=dark`);
      const resume = page.getByRole("button", { name: "Resume", exact: true });
      await resume.waitFor();
      assert(await resume.isEnabled(), "only an eligible suspended scope offers Continue");
      await resume.focus(); await page.keyboard.press("Enter");
      await page.getByText("Resuming…", { exact: true }).waitFor();
      assert(await resume.isDisabled(), "pending recovery deduplicates the user gesture");
      const stop = page.getByRole("button", { name: "Stop", exact: true });
      assert(await stop.isEnabled(), "Stop stays reachable during recovery");
      if (interrupted) {
        await stop.click();
        await page.getByText("Stopped", { exact: true }).waitFor();
      } else {
        await page.getByText("Control authorized", { exact: true }).waitFor();
        assert(await page.getByRole("button", { name: "Pause", exact: true }).isEnabled());
      }
      const calls = await page.evaluate(() => [...window.__taskUi.calls]);
      assert.equal(calls.filter((name) => name === "computer_use_task_resume").length, 1);
      assert(!calls.some((name) => /picker_|authorize|request_access/.test(name)), "Continue does not ask for the original scope again");
      assert.equal(await page.getByRole("alert").count(), 0);
      await page.screenshot({ path: path.join(output, `keyboard-resume-${interrupted ? "stopped" : "running"}.png`), fullPage: true });
      reports.push({ keyboardResume: true, interrupted, calls, passed: true });
    } finally { await context.close(); }
  }
  await runManagedConsentAcceptance({ browser, origin, output, reports });
  await runExistingConsentAcceptance({ browser, origin, output, reports });
  await runExistingLifecycleAcceptance({ browser, origin, output, reports });
  await runExtensionExportAcceptance({ browser, origin, output, reports });
  await runUploadConsentAcceptance({ browser, origin, output, reports });
  await writeFile(path.join(output, "report.json"), JSON.stringify({ syntheticOnly: true, cases: reports }, null, 2) + "\n");
  console.log(JSON.stringify({ output, passed: reports.length, syntheticOnly: true }));
} catch (error) {
  await writeFile(path.join(output, "failure.json"), JSON.stringify({ error: String(error), completed: reports }, null, 2) + "\n");
  console.error(JSON.stringify({ output, failed: String(error) }));
  process.exitCode = 1;
} finally {
  // A close failure deliberately prevents deleting an owner's live files.
  if (browser) await browser.close();
  if (server) await new Promise((resolve, reject) => server.httpServer.close((error) => error ? reject(error) : resolve()));
  await cleanupBuilds();
}
