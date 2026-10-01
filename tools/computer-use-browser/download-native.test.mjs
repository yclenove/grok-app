import test from "node:test";
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { mkdtempSync, realpathSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, relative, sep } from "node:path";
import { chromium } from "playwright-core";
import { downloadViaHandle } from "./download.mjs";
import { prepareManagedPreferences } from "./profile.mjs";

function deferred() {
  let resolve;
  const promise = new Promise(done => { resolve = done; });
  return { promise, resolve };
}

for (const { firstBytes, native } of [
  { firstBytes: 5, native: true },
  { firstBytes: 2048, native: true },
  { firstBytes: 5, native: false },
]) {
test(`${native ? "native" : "plain-link"} download cancellation owns cleanup with ${firstBytes} response bytes`, { timeout: 20_000 }, async () => {
  assert.ok(process.env.GROK_CU_TEST_CHROME, "explicit isolated test browser required");
  const root = mkdtempSync(join(tmpdir(), "cu-native-download-"));
  const entered = deferred();
  const closed = deferred();
  const trace = [];
  const receivedCookies = [];
  let hits = 0;
  let response;
  let context;
  let pending;
  let timer;
  const server = createServer((req, res) => {
    if (req.url !== "/slow.bin") {
      res.end(`<a href='/slow.bin' ${native ? "download='native.bin'" : ""}>Download</a>`);
      return;
    }
    hits += 1;
    receivedCookies.push(req.headers.cookie || "");
    response = res;
    res.on("close", () => { trace.push("http-closed"); closed.resolve(); });
    res.writeHead(200, { "content-type": "application/octet-stream", "content-disposition": "attachment; filename=native.bin", "content-length": String(firstBytes + 3) });
    res.write(Buffer.alloc(firstBytes, 65));
    trace.push("http-body-held");
    entered.resolve();
  });
  try {
    await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
    prepareManagedPreferences(join(root, "profile"));
    context = await chromium.launchPersistentContext(join(root, "profile"), {
      executablePath: process.env.GROK_CU_TEST_CHROME, headless: true,
      args: ["--headless=new", "--disable-gpu", "--disable-crash-reporter", `--log-file=${join(root, "chrome-debug.log")}`],
    });
    const page = context.pages()[0];
    await context.addCookies([{ name: "private-only", value: "owned-fixture", domain: "127.0.0.1", path: "/private" }]);
    await page.goto(`http://127.0.0.1:${server.address().port}`);
    const handle = await page.$("a");
    assert.ok(handle);
    const click = handle.click.bind(handle);
    handle.click = async (...args) => {
      trace.push("click-entered");
      try { return await click(...args); } finally { trace.push("click-settled"); }
    };
    page.on("download", download => {
      trace.push("download-event");
      const cancel = download.cancel.bind(download);
      download.cancel = async () => {
        trace.push("cancel-entered");
        try { return await cancel(); } finally { trace.push("cancel-settled"); }
      };
    });
    const controller = new AbortController();
    pending = downloadViaHandle({ page, target: { handle }, destPath: join(root, "native.bin"),
      maxBytes: 1024, urlAllowed: url => new URL(url).hostname === "127.0.0.1", signal: controller.signal })
      .then(value => ({ value }), error => ({ error }));
    await entered.promise;
    controller.abort();
    trace.push("aborted");
    const outcome = await Promise.race([pending, new Promise(resolve => { timer = setTimeout(() => resolve(null), native && firstBytes === 5 ? 6000 : 2000); })]);
    assert.ok(outcome, `native operation did not settle: ${trace.join(", ")}`);
    assert.ok(outcome.error, "cancelled native download cannot report success");
    await Promise.race([closed.promise, new Promise((_, reject) => {
      clearTimeout(timer);
      timer = setTimeout(() => reject(new Error("native HTTP transfer remained open")), 1000);
    })]);
    assert.equal(hits, 1, "cancel must not replay the transfer through fetch");
    assert.deepEqual(receivedCookies, [""], "a plain transfer must honor the browser's cookie path rules");
    if (!native) {
      assert.equal(page.isClosed(), false, "plain-link cancellation preserves the page");
      assert.equal(trace.includes("click-entered"), false, "plain links must not click and then fetch again");
      assert.equal(trace.includes("download-event"), false);
    } else if (firstBytes === 2048) {
      assert.equal(page.isClosed(), false, "an identified download cancels without closing its page");
      assert.ok(trace.includes("download-event"), "this case must exercise a real native Download");
    } else {
      assert.equal(page.isClosed(), true, "unidentified native work must be closed before it can report idle");
    }
    assert.deepEqual(readdirSync(root).filter(name => name.endsWith(".part") || name === "native.bin"), []);
  } finally {
    clearTimeout(timer);
    response?.end("end");
    if (context) await context.close();
    if (pending) await pending;
    server.closeAllConnections();
    await new Promise(resolve => server.close(resolve));
    const base = realpathSync(tmpdir());
    const target = realpathSync(root);
    assert(target.toLowerCase().startsWith(`${base}${sep}`.toLowerCase()));
    assert(!relative(base, target).startsWith(".."));
    rmSync(target, { recursive: true });
  }
});
}
