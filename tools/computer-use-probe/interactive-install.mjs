import { createRequire } from "node:module";
import { createInterface } from "node:readline";

const require = createRequire(new URL("../computer-use-browser/package.json", import.meta.url));
const { chromium } = require("playwright-core");
const args = process.argv.slice(2);
const valueAfter = (name, fallback) => {
  const index = args.indexOf(name);
  return index >= 0 ? args[index + 1] : fallback;
};
const profile = valueAfter("--profile", ".cu-probe-installed-profile");
const extension = valueAfter("--extension", "tools/computer-use-extension");
const browser = valueAfter(
  "--browser",
  "C:\\Users\\Administrator\\AppData\\Local\\ms-playwright\\chromium-1223\\chrome-win64\\chrome.exe",
);

const context = await chromium.launchPersistentContext(profile, {
  executablePath: browser,
  headless: false,
  viewport: { width: 1280, height: 800 },
  args: ["--no-first-run", "--no-default-browser-check", "--disable-gpu"],
});
const page = context.pages()[0] ?? (await context.newPage());
await page.goto("chrome://extensions");
await page.waitForTimeout(1_000);
const devMode = page.locator("#devMode");
if ((await devMode.getAttribute("aria-pressed")) !== "true") {
  await devMode.click();
  await page.waitForTimeout(500);
}
await page.locator("#loadUnpacked").click();
process.stdout.write(`FILE_CHOOSER_READY ${extension}\n`);

const input = createInterface({ input: process.stdin });
await new Promise((resolve) => input.once("line", resolve));
process.stdout.write(`WORKERS ${JSON.stringify(context.serviceWorkers().map((worker) => worker.url()))}\n`);
await context.close();
