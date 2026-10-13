/** Internal native identity check; no product pack, model or user profile. */
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import process from "node:process";
import { readFileSync, writeFileSync, lstatSync } from "node:fs";
import { isAbsolute, join, relative, resolve } from "node:path";

const directory = resolve(process.argv[2] || "");
assert.ok(process.argv.length === 3 && isAbsolute(process.argv[2]), "explicit diagnostic root required");
const inputs = JSON.parse(readFileSync(join(directory, "diagnostic-inputs.json"), "utf8"));
assert.equal(inputs.scope, "INTERNAL native compatibility diagnostic only, not a runtime pack");
assert.equal(inputs.stagingComplete, true);
assert.equal(inputs.runtimePrepared, false);
assert.equal(inputs.promoted, false);
assert.equal(inputs.target, process.platform === "win32" ? "windows" : "linux");
assert.equal(process.version, "v22.23.3");
const require = createRequire(join(directory, "worker", "package.json"));
const packagePath = require.resolve("playwright-core/package.json");
const packageRelative = relative(join(directory, "worker/node_modules/playwright-core"), packagePath);
assert.ok(packageRelative && !packageRelative.startsWith("..") && !isAbsolute(packageRelative));
assert.equal(JSON.parse(readFileSync(packagePath, "utf8")).version, "1.64.0");
const browserPath = join(directory, inputs.browserExecutable);
assert.ok(lstatSync(browserPath).isFile() && !lstatSync(browserPath).isSymbolicLink());
const { chromium } = require("playwright-core");
const report = { scope: "internal native binary/SDK identity only, not App or installed acceptance",
  node: process.version, playwright: "1.64.0", platform: process.platform,
  browser: null, browserClosed: false, passed: false, promoted: false };
let browser;
try {
  browser = await chromium.launch({ executablePath: browserPath, headless: true,
    args: ["--disable-gpu", "--disable-dev-shm-usage", "--disable-crash-reporter"] });
  report.browser = browser.version();
  assert.equal(report.browser, "157.0.8101.0");
  const page = await browser.newPage();
  await page.setContent("<!doctype html><title>CU internal version fixture</title><p>owned fixture</p>");
  assert.equal(await page.title(), "CU internal version fixture");
  await browser.close();
  report.browserClosed = true;
  report.passed = true;
} finally {
  if (browser?.isConnected()) await browser.close();
  writeFileSync(join(directory, "native-identity.json"), `${JSON.stringify(report, null, 2)}\n`, { flag: "wx" });
}
process.stdout.write(`${JSON.stringify(report)}\n`);
