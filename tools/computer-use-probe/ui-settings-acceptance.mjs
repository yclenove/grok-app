import assert from "node:assert/strict";

const copy = {
  en: { clear: "Clear activity history", cancel: "Cancel", confirm: "Confirm" },
  zh: { clear: "清除操作记录", cancel: "取消", confirm: "确认" },
  de: { clear: "Aktivitätsverlauf löschen", cancel: "Abbrechen", confirm: "Bestätigen" },
  ta: { clear: "செயல்பாட்டு வரலாற்றை அழி", cancel: "ரத்து", confirm: "உறுதிப்படுத்து" },
};

// Measure the actual compiled product colors, not token strings or a screenshot
// impression. Disabled controls are excluded; consent guidance remains readable.
export async function verifySettingsReadability(page) {
  const metrics = await page.locator(".cu-settings__label-block .settings-row__desc").evaluate(element => {
    const canvas = document.createElement("canvas");
    canvas.width = canvas.height = 1;
    const context = canvas.getContext("2d");
    const rgba = color => {
      context.clearRect(0, 0, 1, 1);
      context.fillStyle = color;
      context.fillRect(0, 0, 1, 1);
      return [...context.getImageData(0, 0, 1, 1).data];
    };
    const over = (source, backdrop) => source.slice(0, 3).map((value, i) =>
      value * source[3] / 255 + backdrop[i] * (1 - source[3] / 255));
    const ancestors = [];
    for (let current = element; current; current = current.parentElement) ancestors.unshift(current);
    let background = [255, 255, 255];
    const opacity = ancestors.map(current => getComputedStyle(current).opacity);
    for (const current of ancestors) background = over(rgba(getComputedStyle(current).backgroundColor), background);
    const color = getComputedStyle(element).color;
    const foreground = over(rgba(color), background);
    const luminance = values => values.map(value => {
      const channel = value / 255;
      return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
    }).reduce((sum, channel, i) => sum + channel * [0.2126, 0.7152, 0.0722][i], 0);
    const levels = [luminance(foreground), luminance(background)].sort((a, b) => b - a);
    return { color, foreground, background, opacity, contrast: (levels[0] + 0.05) / (levels[1] + 0.05) };
  });
  assert.ok(metrics.opacity.every(value => value === "1"), "consent guidance must not inherit disabled opacity");
  assert.ok(metrics.contrast >= 4.5, `consent guidance must remain readable: ${JSON.stringify(metrics)}`);
  return metrics;
}

export async function verifySettingsRead(page, screenshot, test) {
  const zh = test.locale === "zh";
  const message = zh ? "无法读取使用电脑的设置。请重试以确认当前状态。"
    : "Could not load Computer Use settings. Retry to check the current state.";
  const toggle = page.locator('.cu-settings [role="switch"]');
  assert.ok(await toggle.isDisabled());
  await page.getByRole("alert").filter({ hasText: message }).waitFor({ timeout: 15000 });
  await page.locator('.cu-settings__health[data-state="unknown"]').waitFor();
  assert.ok(await page.getByRole("status").filter({ hasText: zh ? "运行时状态尚未确认。" : "Runtime status not confirmed." }).isVisible());
  assert.equal(await page.evaluate(() => window.__cuUiFixture.settingsReads()), 1, "the deadline never automatically retries the Host");
  assert.ok(await toggle.isDisabled());
  assert.equal(await page.locator('.cu-settings__health[data-state="ready"]').count(), 0);
  await screenshot("read-timeout");
  await page.getByRole("button", { name: zh ? "重试" : "Retry", exact: true }).click();
  await page.waitForFunction(() => window.__cuUiFixture.settingsReads() === 2);
  await page.evaluate(() => window.__cuUiFixture.resolveExpiredSettings());
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  assert.ok(await toggle.isDisabled(), "an expired success cannot finish the new pending read");
  assert.equal(await page.getByRole("alert").count(), 0);
  await page.evaluate(() => window.__cuUiFixture.resolveSettingsRetry());
  await page.waitForFunction(() => !document.querySelector('.cu-settings [role="switch"]').disabled);
  assert.equal(await toggle.getAttribute("aria-checked"), "false");
}

export async function verifySettingsMutationReadback(page, screenshot, test) {
  const zh = test.locale === "zh";
  const repair = test.settingsMutation === "repair";
  const label = repair ? (zh ? "修复运行时" : "Repair runtime") : (zh ? "回退运行时" : "Roll back runtime");
  const command = repair ? "computer_use_runtime_repair" : "computer_use_runtime_rollback";
  const trigger = page.getByRole("button", { name: label, exact: true });
  await page.waitForFunction(() => !document.querySelector('.cu-settings [role="switch"]').disabled);
  await trigger.click();
  await page.waitForFunction(() => window.__cuUiFixture.runtimeMutationStarted());
  assert.ok(await trigger.isDisabled());
  assert.equal(await page.locator('.cu-settings__health[data-state="ready"]').count(), 0);
  assert.equal(await page.locator(".cu-settings__runtime").getAttribute("aria-busy"), "true");
  assert.equal(await page.evaluate(() => window.__cuUiFixture.runtimeReadbacks()), 0, "no readback before the mutation finishes");
  await screenshot("mutation-pending");
  await page.evaluate(() => window.__cuUiFixture.resolveRuntimeMutation());
  await page.waitForFunction(() => window.__cuUiFixture.runtimeReadbacks() === 1);
  await page.getByRole("alert").filter({ hasText: zh ? "无法读取使用电脑的设置" : "Could not load Computer Use settings" }).waitFor({ timeout: 15000 });
  await page.locator('.cu-settings__health[data-state="unknown"]').waitFor();
  assert.ok(await page.getByRole("status").filter({ hasText: zh ? "运行时状态尚未确认。" : "Runtime status not confirmed." }).isVisible());
  const retry = page.getByRole("button", { name: zh ? "重试" : "Retry", exact: true });
  assert.ok(await retry.isEnabled());
  assert.ok(await trigger.isDisabled());
  assert.ok(await page.getByRole("switch").isDisabled());
  await screenshot("readback-timeout");
  await retry.click();
  await page.locator('.cu-settings__health[data-state="ready"]').waitFor();
  await page.evaluate(() => window.__cuUiFixture.resolveExpiredRuntime());
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  assert.equal(await page.getByRole("alert").count(), 0);
  assert.equal(await page.locator('.cu-settings__health[data-state="warning"]').count(), 0, "expired health must not replace the successful retry");
  assert.ok(await trigger.isEnabled());
  const mutations = await page.evaluate(() => window.__cuUiFixture.calls.filter(name => [
    "computer_use_runtime_repair", "computer_use_runtime_rollback", "computer_use_set_enabled", "computer_use_authorize_surface",
  ].includes(name)));
  assert.deepEqual(mutations, [command], "read recovery never replays a mutation or grants control");
  await screenshot("readback-recovered");
}

export async function verifySettingsShell(page, screenshot, test) {
  const labels = copy[test.locale];
  assert.ok(labels, "the acceptance case needs explicit locale expectations");
  await page.locator('[data-testid="extensions-panel"]').waitFor();
  const card = page.locator("#settings-anchor-ext-computer");
  assert.equal(await page.locator(".settings-card").count(), 1, "the production extensions shell must not nest settings cards");
  const parent = await card.evaluate(element => {
    const style = getComputedStyle(element.parentElement);
    return { cardAncestor: !!element.parentElement.closest(".settings-card"),
      border: style.borderTopWidth, padding: style.paddingLeft, minWidth: style.minWidth };
  });
  assert.deepEqual(parent, { cardAncestor: false, border: "0px", padding: "0px", minWidth: "0px" });
  const toggle = card.getByRole("switch");
  await page.waitForFunction(() => !document.querySelector('.cu-settings [role="switch"]').disabled);
  assert.equal(await toggle.getAttribute("aria-checked"), "false");
  const maintenance = card.locator(".cu-settings__maintenance");
  assert.equal(await maintenance.getAttribute("open"), null);
  await maintenance.locator("summary").click();
  const trigger = maintenance.getByRole("button", { name: labels.clear, exact: true });
  await trigger.click();
  const dialog = page.getByRole("dialog", { name: labels.clear, exact: true });
  await dialog.waitFor();
  const material = await dialog.evaluate(element => {
    const style = getComputedStyle(element);
    const canvas = document.createElement("canvas");
    canvas.width = canvas.height = 1;
    const context = canvas.getContext("2d");
    context.fillStyle = style.backgroundColor;
    context.fillRect(0, 0, 1, 1);
    return { alpha: context.getImageData(0, 0, 1, 1).data[3], opacity: style.opacity };
  });
  assert.deepEqual(material, { alpha: 255, opacity: "1" }, "destructive confirmation must have an opaque reading surface");
  const cancel = dialog.getByRole("button", { name: labels.cancel, exact: true });
  const confirm = dialog.getByRole("button", { name: labels.confirm, exact: true });
  assert.ok(await cancel.isVisible());
  assert.ok(await confirm.isVisible());
  const bounds = await dialog.boundingBox();
  assert.ok(bounds.x >= 0 && bounds.y >= 0 && bounds.x + bounds.width <= test.width + 1
    && bounds.y + bounds.height <= test.height + 1, "confirmation fits the viewport");
  await cancel.focus();
  await page.keyboard.press("Tab");
  assert.ok(await confirm.evaluate(element => element === document.activeElement), "Tab reaches confirmation within the dialog");
  await page.keyboard.press("Tab");
  assert.ok(await dialog.evaluate(element => element.contains(document.activeElement)), "Tab wraps inside the dialog");
  await screenshot("confirmation");
  await page.keyboard.press("Escape");
  await dialog.waitFor({ state: "hidden" });
  assert.ok(await trigger.evaluate(element => element === document.activeElement), "Escape restores focus to the maintenance trigger");
  const mutationCalls = await page.evaluate(() => window.__cuUiFixture.calls.filter(name => [
    "computer_use_clear_traces", "computer_use_clear_staging", "computer_use_clear_managed_profiles",
    "computer_use_set_enabled", "computer_use_runtime_repair", "computer_use_authorize_surface",
  ].includes(name)));
  assert.deepEqual(mutationCalls, [], "reviewing and dismissing maintenance does not mutate or authorize");
  const unrelatedInspectionCalls = await page.evaluate(() => window.__cuUiFixture.calls.filter(name => [
    "skills_list", "inspect_mcp", "plugins_list",
  ].includes(name)));
  assert.deepEqual(unrelatedInspectionCalls, [], "Computer Use does not launch unrelated CLI extension inspection");
  await maintenance.locator("summary").click();
  await card.scrollIntoViewIfNeeded();
  await screenshot("settings-shell");
  return { parent, material, confirmationBounds: bounds, mutationCalls, unrelatedInspectionCalls };
}
