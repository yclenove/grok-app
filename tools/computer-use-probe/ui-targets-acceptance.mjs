// Product UI in a compiled browser fixture. Host discovery remains mocked;
// keyboard selection and retries must never grant control by themselves.
import assert from "node:assert/strict";

const failureCopy = {
  en: "Could not load targets. Refresh to try again.",
  zh: "无法加载目标，请刷新重试。",
  de: "Ziele konnten nicht geladen werden. Bitte aktualisieren.",
  ta: "இலக்குகளை ஏற்ற முடியவில்லை. புதுப்பித்து மீண்டும் முயற்சிக்கவும்.",
};

export async function verifyTargetRefresh(page, captureFailure, locale, mode) {
  assert.ok(failureCopy[locale], `missing failure-copy expectation for ${locale}`);
  const fields = page.locator(".cu-panel__fields");
  const picker = fields.locator(".c-select").last().locator("button");
  const refresh = page.locator(".cu-panel__refresh");
  const authorize = page.locator(".cu-panel__authorize");
  await page.waitForFunction(() => window.__cuUiFixture.targetListReads() === 1);
  await picker.click();
  await page.locator(".c-select__menu button:not(:disabled)").first().click();
  assert.ok(await authorize.isEnabled());
  await refresh.focus();
  await page.keyboard.press("Enter");
  await page.waitForFunction(() => window.__cuUiFixture.targetListReads() === 2);
  assert.equal(await fields.getAttribute("aria-busy"), "true");
  assert.ok(await authorize.isDisabled());
  assert.ok(await picker.isDisabled());
  if (mode !== "timeout") await page.evaluate(() => window.__cuUiFixture.rejectTargetList());
  // The timeout scenario uses real browser time and leaves Host discovery
  // pending; it must not fake a native cancellation or speed up product timers.
  await page.locator('.cu-panel__card [role="alert"]').waitFor({ timeout: 15000 });
  assert.equal(await page.locator('.cu-panel__card [role="alert"]').textContent(),
    failureCopy[locale]);
  assert.equal(await fields.getAttribute("aria-busy"), "false");
  assert.ok(await authorize.isDisabled(), "failed discovery cannot reuse the old target choice");
  assert.equal(await page.locator(".cu-panel__consent").count(), 0);
  assert.equal(await page.locator(".cu-panel__card .cu-panel__hint").count(), 0,
    "failed discovery should offer recovery rather than an unrelated selection hint");
  assert.equal(await page.locator(".cu-panel__card .cu-panel__empty").count(), 0,
    "a failed request is not evidence of zero targets");
  const footer = await page.locator(".cu-panel__footer").boundingBox();
  assert.ok(footer && footer.y >= 0 && footer.y + footer.height <= page.viewportSize().height + 1);
  await captureFailure();
  await refresh.focus();
  await page.keyboard.press("Enter");
  await page.waitForFunction(() => window.__cuUiFixture.targetListReads() === 3);
  assert.ok(await authorize.isDisabled());
  if (mode === "timeout") {
    await page.evaluate(async () => {
      window.__cuUiFixture.resolveExpiredTargetList();
      await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    });
    assert.equal(await fields.getAttribute("aria-busy"), "true",
      "a retired reply cannot finish the current request");
    assert.equal(await page.locator(".cu-panel__consent").count(), 0,
      "an expired reply cannot revive prior consent");
    assert.equal(await page.locator('[role="alert"]').count(), 0);
    assert.ok(await authorize.isDisabled());
  }
  await page.evaluate(() => window.__cuUiFixture.resolveTargetList());
  await page.waitForFunction(() => document.querySelector('.cu-panel__fields')?.getAttribute('aria-busy') === 'false');
  assert.equal(await page.locator('[role="alert"]').count(), 0, "a successful retry clears only the discovery failure");
  assert.ok(await authorize.isDisabled(), "recovery still requires a new explicit choice");
  await picker.click();
  await page.locator(".c-select__menu button:not(:disabled)").first().click();
  assert.ok(await authorize.isEnabled());
  assert.deepEqual(await page.evaluate(() => window.__cuUiFixture.calls.filter(name =>
    name === "computer_use_authorize_surface" || name === "computer_use_observe")), [],
  "target discovery and recovery must not authorize or capture");
}
