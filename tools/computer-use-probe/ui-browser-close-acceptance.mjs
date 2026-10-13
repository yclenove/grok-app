// Frozen Vite output and actual product components; this is rendered UI evidence,
// not real native close/ACP/MCP/model/installed authorization evidence.
import assert from "node:assert/strict";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { fileURLToPath } from "node:url";
import { copyFile, lstat, mkdir, readdir, readFile, realpath, rm, writeFile } from "node:fs/promises";
import { build, preview } from "vite";
import { chromium } from "../computer-use-browser/node_modules/playwright-core/index.mjs";
import { pinnedChromeLaunchPath } from "../computer-use-browser/immutable-chrome.mjs";

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const output = path.join(repo, "tools/computer-use-probe/.run", `browser-close-ui-${new Date().toISOString().replace(/[:.]/g, "-")}`);
await mkdir(output, { recursive: true });
const locales = ["de", "en", "es", "fil", "fr", "id", "it", "ja", "ko", "pt-BR", "ru", "ta", "uk", "zh", "zh-TW"];
const cases = [];
for (const width of [320, 900]) for (const theme of ["light", "dark"]) for (const locale of locales) {
  cases.push({ width, height: 800, theme, locale, text: "100", dpr: 1 });
}
for (const width of [320, 900]) for (const theme of ["light", "dark"]) for (const locale of ["en", "de", "ta", "zh"]) {
  cases.push({ width, height: 480, theme, locale, text: "200", dpr: 1 });
}
for (const dpr of [1.25, 1.5, 2]) cases.push({ width: 320, height: 800, theme: "dark", locale: "zh", text: "100", dpr });
const reports = [];
let browser, server;
function diskFree() {
  return Number(execFileSync("powershell.exe", ["-NoProfile", "-Command", "(Get-PSDrive -Name H).Free"], { encoding: "utf8", windowsHide: true }).trim());
}
const freeBeforeBuild = diskFree();
assert(freeBeforeBuild > 2 * 1024 ** 3, "insufficient space for isolated UI artifacts");
async function cleanupBuilds() {
  // Browser.close and server.close must both finish before calling this function.
  const live = JSON.parse(execFileSync("powershell.exe", ["-NoProfile", "-Command",
    "ConvertTo-Json -Compress -InputObject @(Get-CimInstance Win32_Process | Select-Object ProcessId,ExecutablePath)"], { encoding: "utf8", windowsHide: true }));
  assert(!live.some(owner => owner.ExecutablePath?.toLowerCase().startsWith(output.toLowerCase() + path.sep)), "generated browser still has a live owner");
  const artifacts = [];
  for (const leaf of ["site", "browser-runtime"]) {
    const directory = path.resolve(output, leaf);
    assert.equal(path.dirname(directory), output);
    const files = [];
    async function inventory(current) {
      const meta = await lstat(current);
      assert(!meta.isSymbolicLink(), "refuse linked/reparse artifacts");
      const resolved = await realpath(current);
      const relativeResolved = path.relative(output, resolved);
      assert(relativeResolved && !relativeResolved.startsWith("..") && !path.isAbsolute(relativeResolved), "artifact escaped its run directory");
      if (meta.isDirectory()) {
        for (const name of await readdir(current)) await inventory(path.join(current, name));
      } else {
        assert(meta.isFile(), "only generated ordinary files can be cleaned");
        const relative = path.relative(directory, current);
        files.push({ path: relative, bytes: meta.size, sha256: createHash("sha256").update(await readFile(current)).digest("hex") });
        if (relative.endsWith(".log") || relative === "source-stamp.txt") {
          const saved = path.join(output, "artifact-logs", leaf, relative);
          await mkdir(path.dirname(saved), { recursive: true });
          await copyFile(current, saved);
        }
      }
    }
    try { await inventory(directory); } catch (error) { if (error.code === "ENOENT") continue; throw error; }
    artifacts.push({ directory, leaf, files });
  }
  await writeFile(path.join(output, "build-artifacts.json"), JSON.stringify({ artifacts }, null, 2) + "\n");
  const freeBeforeCleanup = diskFree();
  for (const artifact of artifacts) await rm(artifact.directory, { recursive: true });
  const freeAfterCleanup = diskFree();
  await writeFile(path.join(output, "cleanup.json"), JSON.stringify({
    ownersClosed: true, removed: artifacts.map(item => item.directory),
    logicalBytes: artifacts.flatMap(item => item.files).reduce((sum, item) => sum + item.bytes, 0),
    freeBeforeBuild, freeBeforeCleanup, freeAfterCleanup,
    observedFreeIncrease: freeAfterCleanup - freeBeforeCleanup,
    measurementLimit: "disk free delta can include concurrent filesystem activity",
    regenerate: "node tools/computer-use-probe/ui-browser-close-acceptance.mjs",
  }, null, 2) + "\n");
}
async function inspectLayout(page, item) {
  const layout = await page.evaluate(() => {
    const box = element => {
      const rect = element.getBoundingClientRect();
      return { x: rect.x, y: rect.y, width: rect.width, height: rect.height,
        scrollWidth: element.scrollWidth, clientWidth: element.clientWidth };
    };
    const modal = document.querySelector('[role="dialog"]');
    return { documentWidth: document.documentElement.scrollWidth, modal: box(modal),
      buttons: [...modal.querySelectorAll("button")].map(box),
      body: box(modal.querySelector(".modal-body")) };
  });
  const fits = box => box.x >= -1 && box.y >= -1 && box.x + box.width <= item.width + 1 && box.y + box.height <= item.height + 1;
  assert(layout.documentWidth <= item.width + 1, "document horizontal overflow");
  assert(fits(layout.modal), "notice must fit viewport");
  assert(layout.buttons.every(box => fits(box) && box.scrollWidth <= box.clientWidth + 1), "every action fits and its label remains readable");
  assert(layout.body.scrollWidth <= layout.body.clientWidth + 1, "long title/details do not overflow horizontally");
  return layout;
}
try {
  const sourceFiles = ["vite.config.ts", "src/components/EmbeddedBrowserCloseNotice.tsx", "src/components/EmbeddedBrowserCloseDetails.tsx", "src/lib/embeddedBrowserLifecycle.ts", "src/styles/embedded-browser-close.css", "src/components/GlassModal.tsx", "src/styles/app.css", "src/styles/chat.part4.css", "tools/computer-use-probe/ui-browser-close-fixture.tsx", ...locales.map(locale => `src/i18n/messages/${locale}/workspace.ts`)];
  const source = [];
  for (const file of sourceFiles) source.push({ file, sha256: createHash("sha256").update(await readFile(path.join(repo, file))).digest("hex") });
  await writeFile(path.join(output, "source.json"), JSON.stringify(source, null, 2) + "\n");
  const site = path.join(output, "site");
  await build({ root: repo, configFile: path.join(repo, "vite.config.ts"), build: { outDir: site, emptyOutDir: false, target: "esnext", rollupOptions: { input: path.join(repo, "tools/computer-use-probe/ui-browser-close-fixture.html") } } });
  server = await preview({ root: repo, configFile: false, build: { outDir: site }, preview: { host: "127.0.0.1", port: 0, open: false } });
  const origin = `http://127.0.0.1:${server.httpServer.address().port}`;
  const executablePath = pinnedChromeLaunchPath(path.join(repo, "src-tauri/resources/computer-use/seed/chromium/chrome-win/chrome.exe"), path.join(output, "browser-runtime"));
  browser = await chromium.launch({ headless: true, executablePath });
  async function fixture(context, item) {
    const page = await context.newPage();
    const errors = [];
    page.on("pageerror", error => errors.push(String(error)));
    await page.route("**/*", route => route.request().url().startsWith(origin + "/") || route.request().url().startsWith("data:") ? route.continue() : route.abort());
    await page.goto(`${origin}/tools/computer-use-probe/ui-browser-close-fixture.html?${new URLSearchParams(item)}`);
    await page.getByRole("button", { name: "Create synthetic close failure", exact: true }).click();
    await page.getByRole("dialog").waitFor();
    const labels = await page.evaluate(() => window.__browserCloseUi.labels);
    return { page, labels, errors };
  }
  for (let index = 0; index < cases.length; index++) {
    const item = cases[index];
    const context = await browser.newContext({ viewport: { width: item.width, height: item.height }, deviceScaleFactor: item.dpr });
    let page;
    const screenshot = `${String(index).padStart(2, "0")}-${item.width}-${item.height}-${item.locale}-${item.theme}-text${item.text}-dpr${item.dpr}.png`;
    try {
      const opened = await fixture(context, item); page = opened.page;
      await page.getByRole("dialog", { name: opened.labels.title, exact: true }).waitFor();
      assert.equal(await page.getByRole("status").textContent(), opened.labels.failure, "full error message stays in the scrollable body");
      const layout = await inspectLayout(page, item);
      await page.locator("details summary").click();
      assert(await page.locator("details").evaluate(element => element.open), "technical details can expand");
      const expanded = await inspectLayout(page, item);
      const state = await page.evaluate(() => window.__browserCloseUi.inspect());
      assert.equal(state.coverDepth, 1);
      assert.equal(state.calls.length, 1, "rendering never auto-retries a close");
      assert.equal(state.state.phase, "failed");
      assert.deepEqual(opened.errors, []);
      await page.screenshot({ path: path.join(output, screenshot), fullPage: true });
      reports.push({ ...item, layout, expanded, state, screenshot, passed: true });
    } catch (error) {
      if (page) await page.screenshot({ path: path.join(output, `failure-${screenshot}`), fullPage: true });
      throw error;
    } finally { await context.close(); }
  }
  for (const action of ["success", "failure", "escape-pending"]) {
    const item = { width: 320, height: 480, theme: "dark", locale: "en", text: "100", dpr: 1 };
    const context = await browser.newContext({ viewport: { width: item.width, height: item.height } });
    try {
      const { page, labels, errors } = await fixture(context, item);
      // Focus trap: both Tab directions remain within the actual modal.
      for (let count = 0; count < 7; count++) {
        await page.keyboard.press(count < 4 ? "Tab" : "Shift+Tab");
        assert(await page.evaluate(() => !!document.activeElement?.closest('[role="dialog"]')));
      }
      const retry = page.getByRole("button", { name: labels.retry, exact: true });
      await retry.focus(); await page.keyboard.press("Enter");
      const busy = page.getByRole("button", { name: labels.closing, exact: true });
      await busy.waitFor(); assert(await busy.isDisabled());
      await page.keyboard.press("Enter");
      const pending = await page.evaluate(() => window.__browserCloseUi.inspect());
      assert.equal(pending.calls.length, 2); assert(pending.pending);
      assert.equal(pending.state.phase, "closing"); assert.equal(pending.coverDepth, 1);
      await inspectLayout(page, item);
      await page.screenshot({ path: path.join(output, `keyboard-${action}-pending.png`) });
      if (action === "escape-pending") {
        await page.keyboard.press("Escape");
        await page.getByRole("dialog").waitFor({ state: "hidden" });
        await page.waitForFunction(() => window.__browserCloseUi.inspect().coverDepth === 0);
        assert(await page.getByRole("button", { name: "Create synthetic close failure" }).evaluate(element => element === document.activeElement), "Escape restores initiating focus");
        const hidden = await page.evaluate(() => window.__browserCloseUi.inspect());
        assert.equal(hidden.state.phase, "closing"); assert.equal(hidden.calls.length, 2); assert(hidden.pending);
      }
      await page.evaluate(success => window.__browserCloseUi.finish(success), action === "success");
      if (action === "success") {
        await page.getByRole("dialog").waitFor({ state: "hidden" });
        await page.waitForFunction(() => window.__browserCloseUi.inspect().coverDepth === 0);
        assert.equal((await page.evaluate(() => window.__browserCloseUi.inspect())).state.phase, "idle");
      } else if (action === "failure") {
        await retry.waitFor(); assert(await retry.isEnabled());
        await page.locator("details summary").click();
        await page.getByText("synthetic retry still pending; original owner retained", { exact: true }).waitFor();
        await page.keyboard.press("Escape");
        await page.getByRole("dialog").waitFor({ state: "hidden" });
        assert.equal((await page.evaluate(() => window.__browserCloseUi.inspect())).state.phase, "failed");
      } else {
        await page.waitForFunction(() => window.__browserCloseUi.inspect().state.phase === "failed");
        assert.equal(await page.getByRole("dialog").count(), 0, "deferred attempt stays quiet when the same original reply fails");
      }
      assert.deepEqual(errors, []);
      reports.push({ keyboard: action, pending, result: await page.evaluate(() => window.__browserCloseUi.inspect()), passed: true });
    } finally { await context.close(); }
  }
  for (const entry of JSON.parse(await readFile(path.join(output, "source.json"), "utf8"))) {
    assert.equal(createHash("sha256").update(await readFile(path.join(repo, entry.file))).digest("hex"), entry.sha256, "source changed during frozen UI acceptance");
  }
  await writeFile(path.join(output, "report.json"), JSON.stringify({ syntheticOnly: true, nativeClose: false, osDpi: false, sourceFrozen: true, cases: reports }, null, 2) + "\n");
  console.log(JSON.stringify({ output, passed: reports.length, syntheticOnly: true }));
} catch (error) {
  await writeFile(path.join(output, "failure.json"), JSON.stringify({ error: String(error), completed: reports }, null, 2) + "\n");
  console.error(JSON.stringify({ output, failed: String(error) }));
  process.exitCode = 1;
} finally {
  // A close/join failure intentionally prevents deletion of live or uncertain files.
  if (browser) await browser.close();
  if (server) await new Promise((resolve, reject) => server.httpServer.close(error => error ? reject(error) : resolve()));
  await cleanupBuilds();
}
