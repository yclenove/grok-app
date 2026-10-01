import test from "node:test";
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { mkdtempSync, readFileSync, realpathSync, rmSync, existsSync, readdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fetchWithRedirectChecks, streamToPart, uniquePartPath, removePart } from "./download.mjs";

function listen(server) {
  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => resolve(server.address().port));
  });
}

test("streamToPart counts bytes and refuses over-limit without leaving dest", async (t) => {
  const dir = mkdtempSync(join(realpathSync(tmpdir()), "grok-cu-dl-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const dest = join(dir, "out.bin");
  const part = uniquePartPath(dest);
  const chunks = {
    async *[Symbol.asyncIterator]() {
      yield Buffer.from("abc");
      yield Buffer.from("def");
    },
  };
  const bytes = await streamToPart(chunks, part, 100, undefined);
  assert.equal(bytes, 6);
  assert.equal(readFileSync(part, "utf8"), "abcdef");
  removePart(part);
  assert.equal(existsSync(part), false);

  const over = uniquePartPath(dest);
  await assert.rejects(
    () =>
      streamToPart(
        {
          async *[Symbol.asyncIterator]() {
            yield Buffer.alloc(50);
            yield Buffer.alloc(50);
          },
        },
        over,
        60,
        undefined,
      ),
    (error) => error.workerCode === "download_too_large",
  );
  removePart(over);
  assert.equal(existsSync(over), false);
});

test("fetchWithRedirectChecks re-checks every hop and deletes .part on reject", async () => {
  const dir = mkdtempSync(join(realpathSync(tmpdir()), "grok-cu-dlr-"));
  const dest = join(dir, "file.bin");
  const part = uniquePartPath(dest);
  const seenCookies = [];
  const server = createServer((req, res) => {
    const url = new URL(req.url || "/", "http://127.0.0.1");
    seenCookies.push([url.pathname, req.headers.cookie || ""]);
    if (url.pathname === "/go") {
      res.writeHead(302, { location: "/file.bin" });
      res.end();
      return;
    }
    if (url.pathname === "/evil") {
      res.writeHead(302, { location: "javascript:alert(1)" });
      res.end();
      return;
    }
    res.writeHead(200, { "content-type": "application/octet-stream" });
    res.end(Buffer.from("staged-by-run"));
  });
  const port = await listen(server);
  const allowed = (raw) => {
    try {
      const parsed = new URL(raw);
      return parsed.protocol === "http:" && parsed.hostname === "127.0.0.1";
    } catch {
      return false;
    }
  };
  const ok = await fetchWithRedirectChecks(`http://127.0.0.1:${port}/go`, {
    urlAllowed: allowed,
    cookiesForUrl: async url => new URL(url).pathname === "/go"
      ? [{ name: "initial", value: "fixture", domain: "127.0.0.1" }] : [],
    signal: undefined,
    maxBytes: 1024,
    partPath: part,
  });
  assert.equal(ok.bytes, 13);
  assert.equal(readFileSync(part, "utf8"), "staged-by-run");
  assert.deepEqual(seenCookies, [["/go", "initial=fixture"], ["/file.bin", ""]]);
  removePart(part);

  const evilPart = uniquePartPath(dest);
  await assert.rejects(
    () =>
      fetchWithRedirectChecks(`http://127.0.0.1:${port}/evil`, {
        urlAllowed: allowed,
        cookiesForUrl: async () => [],
        signal: undefined,
        maxBytes: 1024,
        partPath: evilPart,
      }),
    (error) => error.workerCode === "download_redirect_rejected",
  );
  assert.equal(existsSync(evilPart), false);
  const leftover = readdirSync(dir).filter((name) => name.endsWith(".part"));
  assert.deepEqual(leftover, []);
  await new Promise((resolve) => server.close(resolve));
  rmSync(dir, { recursive: true, force: true });
});
