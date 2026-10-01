// Compiled product UI in isolated Chromium; mocked IPC is NOT installed GNOME acceptance.
import assert from "node:assert/strict";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { mkdir, writeFile } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { build, preview } from "vite";
import { chromium } from "../computer-use-browser/node_modules/playwright-core/index.mjs";
import { pinnedChromeLaunchPath } from "../computer-use-browser/immutable-chrome.mjs";

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const output = path.resolve(process.argv[2] ?? path.join(repo, "tools/computer-use-probe/.run", `helper-ui-${Date.now()}`));
await mkdir(output, { recursive: true });
const seed = path.join(repo, "src-tauri/resources/computer-use/seed");
function fingerprint() {
  const report = JSON.parse(execFileSync(process.execPath, [path.join(repo, "scripts/audit-computer-use-bundle.mjs"),
    seed, "--target", "x86_64-windows", "--seed-only"], { cwd: repo, encoding: "utf8", timeout: 30000, maxBuffer: 1024 * 1024, windowsHide: true }));
  assert.deepEqual(report.hits, []); return report.digest;
}
const seedBefore = fingerprint();
let server, browser, activePage, activeCase;
const reports = [];
let failure = null;
const layouts = [
  { width: 320, height: 480, locale: "en", theme: "light", scale: 1 },
  { width: 400, height: 600, locale: "zh", theme: "dark", scale: 2 },
  { width: 320, height: 480, locale: "de", theme: "dark", scale: 1.5 },
  { width: 400, height: 800, locale: "ta", theme: "light", scale: 2, text: "200" },
];
async function layout(page, selector, screenshot) {
  const report = await page.locator(selector).evaluate(element => {
    const box = element.getBoundingClientRect();
    const problems = [...element.querySelectorAll("button, p, h3, h2")].flatMap(node => {
      const r = node.getBoundingClientRect();
      return r.width > 0 && (r.left < -1 || r.right > innerWidth + 1 || node.scrollWidth > node.clientWidth + 1
        || (node.tagName === "BUTTON" && node.scrollHeight > node.clientHeight + 1))
        ? [{ tag: node.tagName, text: node.textContent, left: r.left, right: r.right, scroll: node.scrollWidth, width: node.clientWidth,
          height: node.clientHeight, scrollHeight: node.scrollHeight, fontSize: getComputedStyle(node).fontSize,
          lineHeight: getComputedStyle(node).lineHeight }] : [];
    });
    return { left: box.left, right: box.right, top: box.top, bottom: box.bottom,
      viewport: { width: innerWidth, height: innerHeight }, documentWidth: document.documentElement.scrollWidth, problems };
  });
  assert.ok(report.documentWidth <= report.viewport.width + 1, JSON.stringify(report));
  assert.deepEqual(report.problems, [], "action/guidance text must wrap without clipping");
  if (selector === '[role="dialog"]') {
    assert.ok(report.top >= 0 && report.bottom <= report.viewport.height + 1, "confirmation fits viewport and keeps footer reachable");
    const material = await page.locator(selector).evaluate(element => {
      const canvas = document.createElement("canvas"); canvas.width = canvas.height = 1;
      const ctx = canvas.getContext("2d"); ctx.fillStyle = getComputedStyle(element).backgroundColor; ctx.fillRect(0, 0, 1, 1);
      return { alpha: ctx.getImageData(0, 0, 1, 1).data[3], opacity: getComputedStyle(element).opacity };
    });
    assert.deepEqual(material, { alpha: 255, opacity: "1" }, "confirmation uses opaque product reading surface");
  }
  await page.screenshot({ path: path.join(output, screenshot), fullPage: selector !== '[role="dialog"]' });
  return report;
}
try {
  const site = path.join(output, "site");
  await build({ root: repo, configFile: path.join(repo, "vite.config.ts"), build: { outDir: site, emptyOutDir: false,
    target: "esnext", rollupOptions: { input: path.join(repo, "tools/computer-use-probe/ui-helper-fixture.html") } } });
  server = await preview({ root: repo, configFile: false, build: { outDir: site },
    preview: { host: "127.0.0.1", port: 0, open: false } });
  const origin = `http://127.0.0.1:${server.httpServer.address().port}`;
  const executablePath = pinnedChromeLaunchPath(path.join(seed, "chromium/chrome-win/chrome.exe"), path.join(output, "browser-runtime"));
  browser = await chromium.launch({ headless: true, executablePath });
  const cases = [];
  for (const locale of ["de", "en", "es", "fil", "fr", "id", "it", "ja", "ko", "pt-BR", "ru", "ta", "uk", "zh", "zh-TW"])
    for (const theme of ["light", "dark"]) cases.push({ ...layouts[0], locale, theme, state: "missing" });
  for (const state of ["repair_required", "restart_required", "ready", "disabled", "global_disabled", "blocked", "unconfirmed", "conflict", "unsupported", "unavailable"])
    cases.push({ ...layouts[1], state });
  for (const action of ["install", "repair", "enable", "disable"]) for (const l of layouts)
    cases.push({ ...l, action, state: { install: "missing", repair: "repair_required", enable: "disabled", disable: "ready" }[action] });
  for (const l of layouts.slice(0, 2)) cases.push({ ...l, action: "install", state: "missing", fail: true });
  cases.push({ ...layouts[0], state: "missing", feature: "on" });
  for (let index = 0; index < cases.length; index++) {
    const test = cases[index]; activeCase = test;
    const page = await browser.newPage({ viewport: { width: test.width, height: test.height }, deviceScaleFactor: test.scale, reducedMotion: "reduce" });
    activePage = page; const errors = [];
    page.on("pageerror", error => errors.push(error.message));
    await page.route("**/*", route => route.request().url().startsWith(origin) || route.request().url().startsWith("data:") ? route.continue() : route.abort());
    await page.goto(`${origin}/tools/computer-use-probe/ui-helper-fixture.html?${new URLSearchParams(test)}`, { waitUntil: "networkidle" });
    const data = await page.evaluate(() => ({ labels: window.__cuHelperFixture.labels, availableActions: window.__cuHelperFixture.availableActions }));
    const group = page.locator("#settings-anchor-ext-computer-helper");
    await group.getByRole("status").filter({ hasText: data.labels.state }).waitFor();
    const prefix = `${index}-${test.locale}-${test.theme}-${test.width}`;
    const metrics = [await layout(page, "#fixture", `${prefix}-status.png`)];
    for (const action of ["install", "repair", "enable", "disable"])
      assert.equal(await group.getByRole("button", { name: data.labels[action], exact: true }).isEnabled(), data.availableActions.includes(action));
    const mutations = () => page.evaluate(() => window.__cuHelperFixture.calls.filter(x => x.command === "computer_use_helper_action"));
    assert.deepEqual(await mutations(), [], "render/read never install, enable or grant control");
    if (test.action) {
      const trigger = group.getByRole("button", { name: data.labels[test.action], exact: true });
      await trigger.click(); const dialog = page.getByRole("dialog", { name: data.labels[test.action], exact: true });
      await dialog.waitFor(); metrics.push(await layout(page, '[role="dialog"]', `${prefix}-${test.action}-confirm.png`));
      const cancel = dialog.getByRole("button", { name: data.labels.cancel, exact: true });
      const confirm = dialog.getByRole("button", { name: data.labels.confirm, exact: true });
      await cancel.focus(); await page.keyboard.press("Tab");
      assert.ok(await confirm.evaluate(node => node === document.activeElement));
      await page.keyboard.press("Tab"); assert.ok(await dialog.evaluate(node => node.contains(document.activeElement)));
      await page.keyboard.press("Escape"); await dialog.waitFor({ state: "hidden" });
      assert.deepEqual(await mutations(), []);
      assert.ok(await trigger.evaluate(node => node === document.activeElement), "cancel restores trigger focus");
      await trigger.click(); await dialog.waitFor(); await confirm.focus(); await page.keyboard.press("Enter");
      await page.waitForFunction(() => window.__cuHelperFixture.calls.some(x => x.command === "computer_use_helper_action"));
      assert.equal((await mutations()).length, 1);
      await page.keyboard.press("Escape"); assert.ok(await dialog.isVisible(), "busy Escape cannot forget original mutation");
      assert.ok(await cancel.isDisabled()); assert.ok(await group.getByRole("button", { name: data.labels.refresh, exact: true }).isDisabled());
      if (test.fail) await page.evaluate(() => window.__cuHelperFixture.failReads(true));
      await page.evaluate(fail => window.__cuHelperFixture.settle(fail), !!test.fail);
      await dialog.waitFor({ state: "hidden" });
      if (test.fail) {
        await group.getByRole("alert").filter({ hasText: data.labels.failed }).waitFor();
        for (const action of ["install", "repair", "enable", "disable"])
          assert.ok(await group.getByRole("button", { name: data.labels[action], exact: true }).isDisabled());
        await page.evaluate(() => window.__cuHelperFixture.failReads(false));
        await group.getByRole("button", { name: data.labels.refresh, exact: true }).click();
        await group.getByRole("status").filter({ hasText: data.labels.state }).waitFor();
      } else {
        const expected = test.action === "enable" ? data.labels.ready : test.action === "disable" ? data.labels.disabled : data.labels.restart;
        await group.getByRole("status").filter({ hasText: expected }).waitFor();
      }
      assert.equal((await mutations()).length, 1, "read recovery never replays a mutation");
      metrics.push(await layout(page, "#fixture", `${prefix}-${test.action}-readback.png`));
    }
    assert.deepEqual(errors, [], "no browser runtime error");
    assert.equal(await page.evaluate(() => window.__cuHelperFixture.calls.some(x => x.command === "computer_use_authorize_surface")), false);
    reports.push({ test, metrics, errors, mutations: await mutations() });
    await page.close(); activePage = null;
    console.log(`helper UI ${index + 1}/${cases.length}: ${test.locale} ${test.state} ${test.action ?? "read"} PASS`);
  }
} catch (error) {
  failure = { case: activeCase, error: String(error?.stack ?? error) };
  if (activePage) await activePage.screenshot({ path: path.join(output, "failure.png"), fullPage: true }).catch(() => {});
  process.exitCode = 1;
} finally {
  if (browser) await browser.close();
  if (server) await new Promise(resolve => server.httpServer.close(resolve));
  const seedAfter = fingerprint(); assert.equal(seedAfter, seedBefore, "immutable browser seed unchanged");
  await writeFile(path.join(output, "result.json"), JSON.stringify({ evidence: "compiled component/product CSS, mocked Host IPC, isolated Chromium",
    installedNativeApp: false, actualGnome: false, fullGoalComplete: false, seedBefore, seedAfter, passed: reports.length, failure, reports }, null, 2) + "\n");
  console.log(JSON.stringify({ output, passed: reports.length, failure }));
}
