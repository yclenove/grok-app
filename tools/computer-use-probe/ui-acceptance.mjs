// Real browser layout/interaction acceptance. IPC is mocked, not native CU evidence.
import assert from "node:assert/strict";
import path from "node:path";
import { mkdir, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { build, preview } from "vite";
import { execFileSync } from "node:child_process";
import { chromium } from "../computer-use-browser/node_modules/playwright-core/index.mjs";
import { pinnedChromeLaunchPath } from "../computer-use-browser/immutable-chrome.mjs";
import { verifyResize } from "./ui-resize-acceptance.mjs";
import { verifyPairing } from "./ui-pairing-acceptance.mjs";
import { verifyTargetRefresh } from "./ui-targets-acceptance.mjs";
import { verifySettingsRead, verifySettingsShell, verifySettingsMutationReadback, verifySettingsReadability } from "./ui-settings-acceptance.mjs";

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const output = path.join(repo, "tools/computer-use-probe/.run", `ui-redesign-${new Date().toISOString().replace(/[:.]/g, "-")}`);
await mkdir(output, { recursive: true });
const seed = path.join(repo, "src-tauri/resources/computer-use/seed");
function seedFingerprint() {
  const report = JSON.parse(execFileSync(process.execPath, [
    path.join(repo, "scripts/audit-computer-use-bundle.mjs"), seed,
    "--target", "x86_64-windows", "--seed-only",
  ], { cwd: repo, encoding: "utf8", timeout: 30000, maxBuffer: 1024 * 1024, windowsHide: true }));
  assert.deepEqual(report.hits, [], "the UI probe requires a clean seed");
  return report.digest;
}
const seedBefore = seedFingerprint();
let server;
let browser;
let activePage;
let activeCase;
let activeErrors = [];
let activeRequests = new Map();
let activeNetwork = [];
const reports = [];
const failures = [];
try {
  // Compile a frozen fixture with the product pipeline before browser acceptance.
  // A pending dev-server CSS transform is not a loaded, reviewable UI. Build
  // failures stay failures; do not relax page readiness or omit product styles.
  const site = path.join(output, "site");
  await build({ root: repo, configFile: path.join(repo, "vite.config.ts"),
    build: { outDir: site, emptyOutDir: false, target: "esnext",
      rollupOptions: { input: path.join(repo, "tools/computer-use-probe/ui-fixture.html") } } });
  server = await preview({ root: repo, configFile: false, build: { outDir: site },
    preview: { host: "127.0.0.1", port: 0, strictPort: false, open: false } });
  const address = server.httpServer.address();
  const origin = `http://127.0.0.1:${address.port}`;
  const executablePath = pinnedChromeLaunchPath(path.join(seed, "chromium/chrome-win/chrome.exe"),
    path.join(output, "browser-runtime"));
  assert.notEqual(executablePath, path.join(seed, "chromium/chrome-win/chrome.exe"), "never execute the immutable seed directly");
  browser = await chromium.launch({ headless: true, executablePath });
  const cases = [];
  for (const width of [320, 400, 640, 960]) for (const height of [480, 800]) {
    for (const state of ["ready", "running"]) cases.push({ width, height, state, locale: "zh", theme: "dark" });
  }
  for (const state of ["paused", "stopping", "cleanup", "cleanup-native-running", "unknown", "error", "off"]) {
    cases.push({ width: 400, height: 480, state, locale: "en", theme: "light" });
  }
  cases.push({ width: 320, height: 480, state: "running", locale: "de", theme: "light", long: "1", portrait: "1" });
  cases.push({ width: 320, height: 480, state: "ready", locale: "de", theme: "light", long: "1" });
  cases.push({ width: 400, height: 800, state: "running", locale: "ta", theme: "dark", long: "1", text: "200" });
  cases.push({ width: 400, height: 800, state: "ready", locale: "zh", theme: "dark", surface: "existing-tabs" });
  cases.push({ width: 400, height: 800, state: "ready", locale: "en", theme: "light", surface: "app-webview" });
  for (const page of ["settings", "card"]) for (const theme of ["dark", "light"]) {
    cases.push({ width: 400, height: 800, state: "running", locale: "zh", theme, page });
  }
  for (const theme of ["dark", "light"]) cases.push({
    width: 320, height: 480, state: "off", locale: "zh", theme, page: "settings", runtime: "missing",
  });
  cases.push({ width: 400, height: 800, state: "off", locale: "ta", theme: "dark",
    page: "settings", runtime: "missing", text: "200" });
  for (const layout of [
    { width: 320, height: 480, locale: "zh", theme: "dark" },
    { width: 640, height: 800, locale: "en", theme: "light" },
    { width: 400, height: 800, locale: "de", theme: "light" },
    { width: 400, height: 800, locale: "ta", theme: "dark", text: "200" },
  ]) cases.push({ ...layout, state: "off", page: "settings", shell: "extensions", runtime: "missing" });
  for (const [settingsRead, locale, theme] of [["feature", "zh", "dark"], ["runtime", "en", "light"]]) {
    cases.push({ width: 400, height: 800, state: "off", page: "settings", shell: "extensions",
      runtime: "missing", settingsRead, locale, theme });
  }
  for (const settingsMutation of ["repair", "rollback"]) for (const [locale, theme, width] of [["zh", "dark", 320], ["en", "light", 400]]) {
    cases.push({ width, height: 800, state: "off", page: "settings", settingsMutation, locale, theme });
  }
  cases.push({ width: 320, height: 480, state: "initial-unknown", locale: "en", theme: "light" });
  cases.push({ width: 400, height: 480, state: "initial-unknown", locale: "zh", theme: "dark", knownRun: "0" });
  for (const page of [undefined, "card"]) for (const state of ["stop-reply-lost", "stop-cleanup-reply-lost"]) {
    cases.push({ width: 320, height: 480, state, locale: "en", theme: "light", page, long: "1" });
  }
  cases.push({ width: 320, height: 480, state: "preview-reply-lost", locale: "en", theme: "light", long: "1" });
  for (const surface of ["desktop", "managed-browser", "existing-tabs", "app-webview"]) {
    for (const theme of ["dark", "light"]) cases.push({
      width: 320, height: 480, state: "ready", locale: "en", theme, surface, backend: "none",
    });
  }
  for (const width of [1202, 1440]) for (const theme of ["dark", "light"]) {
    cases.push({ width, height: 802, state: "running", locale: "zh", theme, page: "resize" });
  }
  for (const theme of ["dark", "light"]) for (const layout of [
    { width: 320, height: 480, locale: "en" },
    { width: 400, height: 480, locale: "zh" },
    { width: 400, height: 800, locale: "ta", text: "200" },
  ]) cases.push({ ...layout, state: "ready", surface: "existing-tabs", pairing: "confirmed", theme });
  for (const surface of ["desktop", "managed-browser", "existing-tabs", "app-webview"]) {
    for (const theme of ["dark", "light"]) cases.push({
      width: 320, height: 480, state: "ready", locale: "en", theme, surface, targetRefresh: "1",
    });
  }
  for (const theme of ["dark", "light"]) for (const layout of [
    { width: 320, height: 480, locale: "zh" },
    { width: 320, height: 480, locale: "de" },
    { width: 400, height: 800, locale: "ta", text: "200" },
  ]) cases.push({ ...layout, theme, state: "ready", surface: "desktop", targetRefresh: "1" });
  for (const surface of ["desktop", "managed-browser", "existing-tabs", "app-webview"]) {
    cases.push({ width: 320, height: 480, state: "ready", locale: "en", theme: "light",
      surface, targetRefresh: "timeout" });
  }
  cases.push({ width: 320, height: 480, state: "ready", locale: "zh", theme: "dark",
    surface: "desktop", targetRefresh: "timeout" });
  for (let i = 0; i < cases.length; i++) {
    const test = cases[i];
    const page = await browser.newPage({ viewport: { width: test.width, height: test.height }, reducedMotion: "reduce" });
    const errors = [];
    activePage = page;
    activeCase = test;
    activeErrors = errors;
    activeRequests = new Map();
    activeNetwork = [];
    const pending = activeRequests;
    const network = activeNetwork;
    const noteRequest = (request, outcome) => {
      const started = pending.get(request);
      if (!started) return;
      pending.delete(request);
      network.push({ ...started, ...outcome, elapsedMs: Date.now() - started.startedAt });
      if (network.length > 64) network.shift();
    };
    page.on("request", (request) => {
      if (pending.size >= 128) return;
      const url = new URL(request.url());
      pending.set(request, { route: url.protocol === "data:" ? "data:" : url.pathname,
        startedAt: Date.now() });
    });
    page.on("requestfinished", (request) => noteRequest(request, { finished: true }));
    page.on("requestfailed", (request) => noteRequest(request, { failed: request.failure()?.errorText ?? "unknown" }));
    page.on("pageerror", (error) => errors.push(error.message));
    await page.route("**/*", (route) => route.request().url().startsWith(origin) || route.request().url().startsWith("data:") ? route.continue() : route.abort());
    if (test.page === "resize") {
      const metrics = await verifyResize(page, origin, output, test);
      assert.deepEqual(errors, [], "resize browser page errors");
      reports.push({ test, metrics, errors, evidence: "product resize hook/handle in probe shell; mocked Host IPC" });
      console.log(`UI ${i + 1}/${cases.length} resize ${test.width}x${test.height} ${test.theme}: PASS`);
      await page.close();
      continue;
    }
    await page.goto(`${origin}/tools/computer-use-probe/ui-fixture.html?${new URLSearchParams(test)}`, { waitUntil: "networkidle", timeout: 45000 });
    await page.waitForSelector(test.page === "settings" ? ".cu-settings" : test.page === "card" ? ".cu-task-card" : ".cu-panel");
    if (test.settingsRead) await verifySettingsRead(page, stage => page.screenshot({
      path: path.join(output, `${i}-${stage}-${test.settingsRead}-${test.locale}-${test.theme}.png`),
    }), test);
    if (test.settingsMutation) await verifySettingsMutationReadback(page, stage => page.screenshot({
      path: path.join(output, `${i}-${stage}-${test.settingsMutation}-${test.locale}-${test.theme}.png`),
    }), test);
    const settingsShell = test.shell === "extensions" ? await verifySettingsShell(page, stage => page.screenshot({
      path: path.join(output, `${i}-${stage}-${test.width}-${test.locale}-${test.theme}.png`),
    }), test) : undefined;
    if (test.targetRefresh) await verifyTargetRefresh(page, () => page.screenshot({
      path: path.join(output, `${i}-target-refresh-${test.surface}-${test.theme}.png`),
    }), test.locale, test.targetRefresh);
    const pairing = test.pairing ? await verifyPairing(page, () => page.screenshot({
      path: path.join(output, `${i}-pairing-steps-${test.width}-${test.theme}.png`),
    })) : undefined;
    if (test.backend === "none") {
      const picker = page.getByRole("button", { name: "Surface", exact: true });
      await picker.waitFor();
      assert.ok(await picker.isEnabled(), "desktop availability cannot hide the surface chooser");
      assert.equal(await page.locator(".cu-panel__availability").count(), test.surface === "desktop" ? 1 : 0,
        "desktop-only status is not evidence that the selected browser is unavailable");
      assert.ok(await page.locator(".cu-panel__authorize").isDisabled(), "a fresh surface still requires explicit selection");
      await picker.click();
      for (const name of ["Desktop", "Managed Browser", "Existing Tabs", "App WebView"]) {
        assert.ok(await page.getByRole("button", { name, exact: true }).isVisible());
      }
      await page.keyboard.press("Escape");
      assert.deepEqual(await page.evaluate(() => window.__cuUiFixture.calls.filter(name =>
        name === "computer_use_authorize_surface" || name === "computer_use_observe")), [],
      "opening the surface chooser never grants control or captures pixels");
    }
    if (test.runtime === "missing") {
      await page.waitForSelector('.cu-settings__health[data-state="warning"]');
      assert.equal(await page.locator(".cu-settings__issues li").count(), 1, "six missing components need one recovery instruction");
      const diagnostic = page.locator(".cu-settings__diagnostics");
      assert.equal(await diagnostic.getAttribute("open"), null, "component diagnostics start collapsed");
      assert.equal(await diagnostic.locator("dt").count(), 6, "all Host component evidence remains available");
      await diagnostic.locator("summary").focus();
      await page.keyboard.press("Enter");
      assert.notEqual(await diagnostic.getAttribute("open"), null, "keyboard expands diagnostics");
      assert.equal(await page.locator('button[role="switch"]').getAttribute("aria-checked"), "false");
      const unsafeCalls = await page.evaluate(() => window.__cuUiFixture.calls.filter(name =>
        ["computer_use_runtime_repair", "computer_use_set_enabled", "computer_use_authorize_surface"].includes(name)));
      assert.deepEqual(unsafeCalls, [], "viewing diagnostics never repairs or authorizes");
    }
    if (["unknown", "initial-unknown"].includes(test.state)) {
      await page.waitForSelector('.cu-panel[data-state="unknown"]');
      assert.ok(await page.locator(".cu-panel__stop").isEnabled(), "unknown same-run status must remain stoppable");
    }
    if (["stopping", "cleanup", "cleanup-native-running"].includes(test.state)) {
      assert.equal(await page.locator(".cu-panel__fields").count(), 0, "retiring control must not display a disabled setup form");
    }
    if (test.state === "cleanup-native-running") {
      assert.equal(await page.getByRole("status").textContent(), "Removing Computer Use tools…");
      assert.ok(await page.getByRole("button", { name: "Stop", exact: true }).isEnabled());
    }
    if (["stop-reply-lost", "stop-cleanup-reply-lost"].includes(test.state)) {
      await page.getByRole("button", { name: "Stop", exact: true }).click();
      const state = test.state === "stop-reply-lost" ? "stopped" : "mcp_cleanup_pending";
      const surface = test.page === "card" ? ".cu-task-card" : ".cu-panel";
      await page.waitForSelector(`${surface}[data-state="${state}"]`);
      assert.ok(await page.getByRole("button", { name: "Stop", exact: true }).isDisabled(),
        "exact-run Host completion must retire a lost command reply");
      if (!test.page && state === "mcp_cleanup_pending") {
        assert.equal(await page.locator(".cu-panel__fields").count(), 0);
        assert.ok(await page.getByRole("button", { name: "Retry cleanup", exact: true }).isEnabled(),
          "pending tool cleanup remains an explicit separate operation");
      }
      await page.evaluate(() => window.__cuUiFixture.rejectStopReply());
      await page.waitForFunction(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve(true)))));
      assert.equal(await page.locator('[role="alert"]').count(), 0, "retired reply must not create a false error");
      const stops = await page.evaluate(() => window.__cuUiFixture.calls.filter(name => name === "computer_use_stop").length);
      assert.equal(stops, 1, "readback reconciliation must never replay Stop");
    }
    if (test.state === "preview-reply-lost") {
      await page.waitForSelector(".cu-panel__frame img");
      await page.waitForFunction(() => window.__cuUiFixture.previewAgeMs() > 3000);
      const stale = page.getByText("Last captured frame — not live.", { exact: true });
      assert.ok(await stale.isVisible(), "a hanging capture must age the previous frame");
      assert.equal(await page.locator(".cu-panel").getAttribute("data-state"), "running",
        "preview freshness is not native Stop or authorization state");
      assert.ok(await page.getByRole("button", { name: "Stop", exact: true }).isEnabled());
      assert.equal(await page.evaluate(() => window.__cuUiFixture.calls.filter(name => name === "computer_use_observe").length), 2,
        "frame expiry must not issue parallel replacement captures");
      await page.evaluate(() => window.__cuUiFixture.resolvePreview());
      await page.waitForFunction(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve(true)))));
      assert.ok(await stale.isVisible(), "a late frame is not proof of current pixels");
      await page.screenshot({ path: path.join(output, "preview-expired-320-light.png") });
      await stale.waitFor({ state: "hidden", timeout: 5000 });
      assert.equal(await page.locator('[role="alert"]').count(), 0);
    }
    const metrics = await page.evaluate(() => {
      const panel = document.querySelector(".cu-panel");
      const footer = document.querySelector(".cu-panel__footer");
      const bounds = footer?.getBoundingClientRect();
      const body = document.querySelector(".cu-panel__body");
      const buttons = footer ? [...footer.querySelectorAll("button")].map((button) => {
        const r = button.getBoundingClientRect();
        return { text: button.textContent, left: r.left, right: r.right, bottom: r.bottom, top: r.top };
      }) : [];
      const frame = document.querySelector(".cu-panel__frame img");
      return {
        width: innerWidth, height: innerHeight, documentWidth: document.documentElement.scrollWidth,
        panelWidth: panel?.scrollWidth, footerBottom: bounds?.bottom, footerTop: bounds?.top,
        bodyHeight: body?.clientHeight, bodyScrollHeight: body?.scrollHeight, buttons,
        frameFit: frame ? getComputedStyle(frame).objectFit : null,
        fontFamily: getComputedStyle(document.body).fontFamily,
      };
    });
    assert.equal(errors.length, 0, JSON.stringify({ test, errors }));
    assert.match(metrics.fontFamily, /sans-serif|system-ui/i, "fixture must use the app sans-serif font stack");
    assert.ok(metrics.documentWidth <= test.width + 1, JSON.stringify({ test, metrics }));
    if (!test.page && test.state !== "off") {
      assert.ok(metrics.footerBottom <= test.height + 1 && metrics.footerTop >= 0, JSON.stringify({ test, metrics }));
      for (const button of metrics.buttons) assert.ok(button.left >= 0 && button.right <= test.width + 1 && button.bottom <= test.height + 1, JSON.stringify({ test, button }));
      assert.ok(metrics.bodyHeight > 0, JSON.stringify({ test, metrics }));
      if (metrics.frameFit) assert.equal(metrics.frameFit, "contain");
    }
    if (i % 3 === 0 || test.page || test.long || test.pairing || test.state === "initial-unknown" || test.backend === "none") {
      await page.screenshot({ path: path.join(output, `${i}-${test.pairing ? "pairing" : test.page ?? test.state}-${test.width}-${test.theme}.png`) });
    }
    if (test.state === "ready" && !test.page && test.surface !== "existing-tabs"
      && !(test.backend === "none" && test.surface === "desktop")) {
      const before = await page.evaluate(() => window.__cuUiFixture.calls.filter((name) => name === "computer_use_authorize_surface").length);
      await page.locator(".cu-panel__fields .c-select").last().locator("button").click();
      const menu = await page.locator(".c-select__menu").boundingBox();
      assert.ok(menu && menu.x >= -1 && menu.x + menu.width <= test.width + 1, "target menu must fit the panel viewport");
      await page.locator(".c-select__menu button:not(:disabled)").first().click();
      const after = await page.evaluate(() => window.__cuUiFixture.calls.filter((name) => name === "computer_use_authorize_surface").length);
      assert.equal(after, before, "selection must not authorize");
      const authorize = page.locator(".cu-panel__authorize");
      await authorize.focus();
      await page.keyboard.press("Enter");
      await page.waitForFunction(() => window.__cuUiFixture.calls.includes("computer_use_authorize_surface"));
      await page.waitForFunction(() => document.activeElement?.matches(".cu-panel__preview-section"));
    }
    const settingsReadability = test.page === "settings" ? await verifySettingsReadability(page) : undefined;
    reports.push({ test, metrics, errors, pairing, settingsShell, settingsReadability });
    console.log(`UI ${i + 1}/${cases.length} ${test.page ?? test.state} ${test.width}x${test.height} ${test.locale} ${test.theme}: PASS`);
    await page.close();
  }
  await writeFile(path.join(output, "results.json"), JSON.stringify({ evidence: "real Chromium layout, mocked Host IPC", seedBefore, reports }, null, 2));
  console.log(`UI acceptance: ${reports.length}/${cases.length} passed; ${output}`);
} catch (error) {
  failures.push(error);
  await writeFile(path.join(output, "failure.json"), JSON.stringify({
    evidence: "real Chromium layout, mocked Host IPC", test: activeCase, pageErrors: activeErrors,
    pendingRequests: [...activeRequests.values()], recentNetwork: activeNetwork,
    error: String(error),
  }, null, 2));
  if (activePage && !activePage.isClosed()) {
    try { await activePage.screenshot({ path: path.join(output, "failure.png"), timeout: 5000 }); }
    catch (captureError) { failures.push(captureError); }
  }
} finally {
  for (const cleanup of [() => browser?.close(), () => new Promise((resolve, reject) => {
    if (!server) return resolve();
    server.httpServer.close((error) => error ? reject(error) : resolve());
  })]) {
    try { await cleanup(); } catch (error) { failures.push(error); }
  }
  try {
    const seedAfter = seedFingerprint();
    await writeFile(path.join(output, "seed-integrity.json"), JSON.stringify({ seedBefore, seedAfter }, null, 2));
    assert.equal(seedAfter, seedBefore, "UI acceptance must not mutate the packaged runtime");
    console.log(`UI runtime seed unchanged: ${seedAfter}`);
  } catch (error) { failures.push(error); }
}
if (failures.length) throw new AggregateError(failures, "UI acceptance or owned cleanup/integrity failed");
