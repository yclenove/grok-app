import test from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, realpathSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, relative, sep } from "node:path";
import { chromium } from "playwright-core";
import { captureManagedObservation } from "./observation-extract.mjs";
import { disposeObservationState, resolveObservationTarget, withObservationLease } from "./observation-state.mjs";
import { currentPageObservation, registerPageState, runPageMutation } from "./page-state.mjs";
import { prepareManagedPreferences } from "./profile.mjs";
import { prepareTypedAct } from "./typed-act.mjs";

function deferred() {
  let resolve;
  const promise = new Promise(done => { resolve = done; });
  return { promise, resolve };
}

// Observe real handles without adding a diagnostic/eval route to the worker.
// The production collector and typed action implementation run unchanged.
function trackHandles(page) {
  const rows = [];
  const locator = page.locator.bind(page);
  page.locator = (...args) => {
    const result = locator(...args);
    const nth = result.nth.bind(result);
    result.nth = index => {
      const item = nth(index);
      const elementHandle = item.elementHandle.bind(item);
      item.elementHandle = async () => {
        const handle = await elementHandle();
        if (!handle) return handle;
        const row = { handle, calls: 0, disposed: false };
        const dispose = handle.dispose.bind(handle);
        handle.dispose = async () => {
          row.calls += 1;
          await dispose();
          row.disposed = true;
        };
        rows.push(row);
        return handle;
      };
      return item;
    };
    return result;
  };
  return rows;
}

test("real Chromium reclaims model handles without disposing an admitted action early", { timeout: 30_000 }, async (t) => {
  assert.ok(process.env.GROK_CU_TEST_CHROME, "explicit isolated test browser required");
  const root = mkdtempSync(join(tmpdir(), "cu-handle-lifetime-"));
  let context;
  t.after(async () => {
    if (context) await context.close();
    const base = realpathSync(tmpdir());
    const target = realpathSync(root);
    assert(target.toLowerCase().startsWith(`${base}${sep}`.toLowerCase()));
    assert(!relative(base, target).startsWith(".."));
    rmSync(target, { recursive: true });
  });
  prepareManagedPreferences(root);
  context = await chromium.launchPersistentContext(root, {
    executablePath: process.env.GROK_CU_TEST_CHROME, headless: true,
    args: ["--headless=new", "--disable-gpu", "--disable-crash-reporter", `--log-file=${join(root, "chrome-debug.log")}`],
  });
  const page = context.pages()[0];
  await page.setContent(`<button id="increment" onclick="document.getElementById('count').textContent=String(Number(document.getElementById('count').textContent)+1)">Increment</button>
    <input aria-label="Name"><output id="count">0</output>`);
  const rows = trackHandles(page);
  const state = registerPageState({ pages: new Map(), pageIds: new WeakMap() }, page);
  const live = () => rows.filter(row => !row.disposed);
  let model;

  await t.test("25 model replacements and previews retain only the current model", async () => {
    for (let index = 0; index < 25; index += 1) {
      model = await captureManagedObservation(state, { screenshot: false });
      assert.equal(live().length, 2, `model iteration ${index} leaked handles`);
      await captureManagedObservation(state, { preview: true, screenshot: false });
      assert.equal(live().length, 2, `preview iteration ${index} leaked handles`);
    }
    assert(rows.filter(row => row.disposed).every(row => row.calls === 1));
    // Playwright 1.48 reports an already disposed handle as TargetClosedError.
    // Prove the page/current handle are alive and the old channel was removed,
    // so a crashed browser cannot accidentally satisfy this assertion.
    const old = rows[0].handle;
    assert.equal(page.isClosed(), false);
    assert.equal(await live()[0].handle.evaluate(element => element.isConnected), true);
    assert.equal(old._connection._objects.has(old._guid), false);
    await assert.rejects(old.evaluate(element => element.isConnected), /disposed|has been closed/);
  });

  await t.test("mutation retires authority immediately but physical click retains its handles", async () => {
    const observation = currentPageObservation(state);
    const body = { pageGeneration: model.pageGeneration, snapshotId: model.snapshotId,
      elementRef: model.nodes.find(node => node.name === "Increment").elementRef, actionId: "pinned-click" };
    const entered = deferred(); const release = deferred();
    const pending = withObservationLease(observation, body, async () => {
      const prepared = await prepareTypedAct({ page, observation, kind: "click", body });
      return runPageMutation(state, async () => {
        entered.resolve();
        await release.promise;
        return prepared.dispatch();
      }, observation);
    });
    await entered.promise;
    const during = live().length;
    const before = await page.locator("#count").textContent();
    try {
      assert.throws(() => resolveObservationTarget(observation, body), error => error.workerCode === "observation_required");
    } finally { release.resolve(); }
    await pending;
    assert.equal(during, 2, "admitted click handles were disposed before physical dispatch");
    assert.equal(before, "0");
    assert.equal(await page.locator("#count").textContent(), "1", "independent page oracle");
    assert.equal(live().length, 0);
  });

  await t.test("failed operation releases retired real handles", async () => {
    const fresh = await captureManagedObservation(state, { screenshot: false });
    const observation = currentPageObservation(state);
    await assert.rejects(withObservationLease(observation, fresh, async () => runPageMutation(state,
      async () => { throw new Error("fixture cancellation"); }, observation)), /fixture cancellation/);
    assert.equal(live().length, 0);
    assert.equal(await page.locator("#count").textContent(), "1");
  });

  await t.test("navigation and close dispose the last published handles", async () => {
    await captureManagedObservation(state, { screenshot: false });
    const previous = currentPageObservation(state);
    await page.reload();
    await disposeObservationState(previous);
    assert.equal(live().length, 0);
    await page.setContent("<button>Close fixture</button>");
    await captureManagedObservation(state, { screenshot: false });
    const last = currentPageObservation(state);
    await page.close();
    await disposeObservationState(last);
    assert.equal(live().length, 0);
    assert(rows.every(row => row.calls === 1));
  });
});
