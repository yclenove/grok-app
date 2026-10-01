import assert from "node:assert/strict";
import path from "node:path";

/** Real browser events through the product hook/handle, in a probe-only shell. */
export async function verifyResize(page, origin, output, test) {
  await page.goto(`${origin}/tools/computer-use-probe/ui-fixture.html?${new URLSearchParams(test)}`,
    { waitUntil: "networkidle", timeout: 45000 });
  const handle = page.getByRole("separator");
  const width = async (expected) => {
    await page.waitForFunction((value) => {
      const separator = document.querySelector(".aside-resizer");
      const pane = document.querySelector(".workbench > .aside");
      return separator?.getAttribute("aria-valuenow") === String(value)
        && Math.abs(pane.getBoundingClientRect().width - value) <= 1;
    }, expected);
    assert.equal(await handle.getAttribute("aria-valuenow"), String(expected));
  };
  const persisted = () => page.evaluate(() => JSON.parse(localStorage.getItem("grok-app.layout")));
  const maximum = test.width - 268 - 360;
  // Product cold start deliberately closes the pane; only width persists.
  await page.getByTestId("open-pane").click();
  await width(500);
  assert.equal(await handle.getAttribute("aria-valuemin"), "458");
  assert.equal(await handle.getAttribute("aria-valuemax"), String(maximum));
  await page.getByTestId("close-pane").focus();
  await page.keyboard.press("Tab");
  assert.ok(await handle.evaluate((element) => element === document.activeElement));
  assert.ok(await handle.evaluate((element) => element.matches(":focus-visible")));
  assert.equal(await handle.evaluate((element) => getComputedStyle(element, "::after").width), "3px");
  for (const [key, expected] of [
    ["ArrowLeft", 510], ["Shift+ArrowLeft", 560], ["ArrowRight", 550],
    ["Shift+ArrowRight", 500], ["Home", 458], ["End", maximum],
    ["End", maximum], ["Control+ArrowLeft", maximum],
  ]) {
    await page.keyboard.press(key);
    await width(expected);
    assert.equal((await persisted()).asideWidth, expected, `persist ${key}`);
  }
  await page.emulateMedia({ forcedColors: "active" });
  assert.equal(await handle.evaluate((element) => getComputedStyle(element).outlineStyle), "solid");
  assert.equal(await handle.evaluate((element) => getComputedStyle(element).outlineWidth), "2px");
  await page.emulateMedia({ forcedColors: "none" });
  await page.screenshot({ path: path.join(output, `resize-focus-${test.width}-${test.theme}.png`) });
  await page.keyboard.press("Tab");
  assert.ok(await page.getByTestId("after-separator").evaluate((element) => element === document.activeElement));
  await page.reload({ waitUntil: "networkidle" });
  await page.getByTestId("open-pane").click();
  await width(maximum);
  const bounds = await handle.boundingBox();
  assert.ok(bounds);
  await page.mouse.move(bounds.x + bounds.width / 2, 400);
  await page.mouse.down();
  await page.mouse.move(test.width - 520, 400, { steps: 4 });
  await width(520);
  assert.equal(await page.evaluate(() => document.body.style.cursor), "col-resize");
  await page.mouse.up();
  await page.waitForFunction(() => document.body.style.cursor === "");
  assert.equal((await persisted()).asideWidth, 520);
  await page.reload({ waitUntil: "networkidle" });
  await page.getByTestId("open-pane").click();
  await width(520);
  const nextBounds = await handle.boundingBox();
  await page.mouse.move(nextBounds.x + nextBounds.width / 2, 400);
  await page.mouse.down();
  await page.keyboard.press("Shift+Tab");
  assert.ok(await page.getByTestId("close-pane").evaluate((element) => element === document.activeElement));
  await page.keyboard.press("Enter");
  await handle.waitFor({ state: "hidden" });
  await page.mouse.up();
  assert.equal((await persisted()).asideCollapsed, true, "late pointer-up never reopens the pane");
  assert.equal(await page.evaluate(() => document.body.style.cursor), "");
  assert.equal(await page.evaluate(() => document.body.style.userSelect), "");
  await page.getByTestId("open-pane").click();
  await width(520);
  const unsafe = await page.evaluate(() => window.__cuUiFixture.calls.filter((name) =>
    ["computer_use_authorize_surface", "computer_use_set_enabled", "computer_use_stop"].includes(name)));
  assert.deepEqual(unsafe, [], "resizing never changes Computer Use authorization or control");
  return { minimum: 458, maximum, pointerWidth: 520, keyboard: true,
    visibleFocus: true, forcedColors: true, persistence: true, lateRelease: true };
}
