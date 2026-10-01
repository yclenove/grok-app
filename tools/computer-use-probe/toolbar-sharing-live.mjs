// Explicitly interactive: the operator clicks the browser toolbar and real extension popup.
// No test API grants activeTab; unrelated origins are fulfilled locally, without Internet data.
import assert from "node:assert/strict";

export async function verifyToolbarSharing({ context, popup, rpc, until, stage, passed }) {
  const worker = context.serviceWorkers()[0];
  await worker.evaluate(() => {
    const execute = chrome.scripting.executeScript.bind(chrome.scripting);
    globalThis.__cuToolbarInspections = [];
    chrome.scripting.executeScript = request => {
      globalThis.__cuToolbarInspections.push(request.target.tabId);
      return execute(request);
    };
  });
  const title = "CU Toolbar Fixture - share this test tab";
  const url = "http://toolbar.cu.test/fixture";
  await context.route("http://*.cu.test/**", route => route.fulfill({ contentType: "text/html",
    body: `<title>${route.request().url() === url ? title : "CU Toolbar Fixture - unshared"}</title><main>Local Computer Use test fixture. No account or private data.</main>` }));
  const unrelated = await context.newPage(); await unrelated.goto("http://unshared.cu.test/fixture");
  const fixture = await context.newPage(); await fixture.goto(url); await fixture.bringToFront();
  const id = await worker.evaluate(async () => (await chrome.tabs.query({ active: true, lastFocusedWindow: true }))[0].id);

  stage("toolbar-grant-required");
  await until(async () => await popup.locator("#share-current").isEnabled());
  await popup.locator("#share-current").click();
  await until(async () => await popup.locator("#tab-status").getAttribute("data-state") === "shareUnavailable");
  assert.equal((await rpc("shared")).length, 0);
  passed("toolbar-grant-required");
  await popup.close(); await fixture.bringToFront();

  stage("toolbar-user-share");
  console.error("toolbar probe: open Grok Computer Use from the fixture browser toolbar, then Share current tab");
  await until(async () => (await rpc("shared")).length === 1, 300000);
  const [candidate] = await rpc("shared");
  assert.equal(candidate.url, url); assert.equal(candidate.title, title);
  const inspected = await worker.evaluate(() => globalThis.__cuToolbarInspections);
  assert(inspected.length >= 4 && inspected.every(tabId => tabId === id));
  assert(!unrelated.isClosed());
  passed("toolbar-user-share");

  stage("toolbar-user-unshare");
  console.error("toolbar probe: share verified; click Stop sharing this tab in the same popup");
  await until(async () => (await rpc("shared")).length === 0, 120000);
  assert(!fixture.isClosed()); assert.equal(fixture.url(), url);
  assert((await rpc("status")).paired === true);
  passed("toolbar-user-unshare");
  await fixture.close(); await unrelated.close();
}
