// Real browser keyboard/layout coverage; Host IPC and clipboard are test doubles.
// Never touch the workstation clipboard or approve a native permission prompt.
import assert from "node:assert/strict";

export async function verifyPairing(page, captureSteps) {
  await page.evaluate(() => {
    const clipboard = { writes: [], denyNext: false };
    window.__cuPairingClipboard = clipboard;
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: {
      async writeText(value) {
        if (clipboard.denyNext) {
          clipboard.denyNext = false;
          throw new DOMException("Fixture denied", "NotAllowedError");
        }
        clipboard.writes.push(value);
      },
    } });
  });
  const pairing = page.locator(".cu-panel__pairing");
  const steps = pairing.locator(".cu-pairing__steps li");
  assert.equal(await steps.count(), 3);
  const layout = await pairing.evaluate(section => {
    const panel = section.closest(".cu-panel");
    const fontSize = Number.parseFloat(getComputedStyle(panel).fontSize);
    const rows = [...section.querySelectorAll(".cu-pairing__steps li")].map(step => step.getBoundingClientRect());
    return { narrow: panel.getBoundingClientRect().width <= 27 * fontSize,
      vertical: rows[1].top >= rows[0].bottom && rows[2].top >= rows[1].bottom };
  });
  assert.equal(layout.vertical, layout.narrow, "narrow panels and large text stack complete steps instead of fragmenting labels");
  assert.equal(await steps.nth(0).getAttribute("aria-current"), "step");
  assert.equal(await pairing.locator(".cu-pairing__copy").count(), 0);

  await pairing.locator(".cu-panel__actions button").first().focus();
  await page.keyboard.press("Enter");
  const confirm = pairing.locator(".btn--solid");
  await confirm.waitFor();
  assert.equal(await steps.nth(1).getAttribute("aria-current"), "step");
  assert.equal(await pairing.locator(".cu-pairing__copy").count(), 0,
    "a challenge is not an App-side confirmation");
  await confirm.focus();
  await page.keyboard.press("Enter");
  const code = pairing.getByTestId("cu-pairing-code");
  await code.waitFor();
  assert.equal(await steps.nth(2).getAttribute("aria-current"), "step",
    "confirmation does not claim a tab is already shared");
  assert.equal(await code.inputValue(), "ABCDE-12345-ABCDE-12345");
  assert.equal(await code.getAttribute("readonly"), "");
  await page.waitForFunction(() => document.activeElement === document.querySelector(".cu-pairing__fields input"));
  await steps.first().scrollIntoViewIfNeeded();
  await captureSteps();

  const copies = pairing.locator(".cu-pairing__copy");
  for (let index = 0; index < 2; index++) {
    if (index === 1) await page.keyboard.press("Tab"); // code input
    await page.keyboard.press("Tab"); // copy button for the focused field
    assert.ok(await copies.nth(index).evaluate(button => document.activeElement === button),
      "confirmation and copy controls preserve natural keyboard order");
    await page.keyboard.press("Enter");
    await page.waitForFunction(expected => window.__cuPairingClipboard.writes.length === expected, index + 1);
  }
  assert.deepEqual(await page.evaluate(() => window.__cuPairingClipboard.writes),
    ["http://127.0.0.1:12345", "ABCDE-12345-ABCDE-12345"],
    "copy only the requested public field, never the pairing nonce");
  await page.evaluate(() => { window.__cuPairingClipboard.denyNext = true; });
  await copies.last().focus();
  await page.keyboard.press("Enter");
  await pairing.locator('.cu-pairing__feedback[data-state="error"]').waitFor();
  const feedback = pairing.getByRole("status");
  assert.ok((await feedback.textContent()).trim().length > 0);
  assert.ok(!(await feedback.textContent()).includes("Fixture denied"), "do not expose a raw clipboard error");
  await code.focus();
  assert.deepEqual(await code.evaluate(input => [input.selectionStart, input.selectionEnd]),
    [0, "ABCDE-12345-ABCDE-12345".length], "manual copy remains available after denial");
  const bounds = await copies.evaluateAll(buttons => buttons.map(button => {
    const rect = button.getBoundingClientRect();
    return { left: rect.left, right: rect.right, width: rect.width, height: rect.height };
  }));
  const viewport = page.viewportSize();
  for (const rect of bounds) {
    assert.ok(rect.left >= 0 && rect.right <= viewport.width + 1, "copy buttons must not overflow");
    assert.ok(rect.width >= 24 && rect.height >= 24, "copy controls retain a usable hit target");
  }
  assert.deepEqual(await page.evaluate(() => window.__cuUiFixture.calls.filter(name =>
    ["computer_use_authorize_surface", "computer_use_observe", "computer_use_set_enabled"].includes(name))), [],
  "pairing and copying never authorize a surface, capture, or enable Computer Use");
  return { steps: 3, copiedFields: 2, layout, manualSelectionAfterDenial: true,
    evidence: "real browser keyboard/layout; mocked Host IPC and clipboard, no native authorization" };
}
