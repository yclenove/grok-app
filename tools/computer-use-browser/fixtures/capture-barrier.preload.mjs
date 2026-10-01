// Private native-test barrier, NOT shipped in the runtime. Retain a genuine
// screenshot's bytes after capture; do not substitute a page, image or action.
import { basename } from "node:path";
import { createRequire } from "node:module";
import { pathToFileURL } from "node:url";
import { createHash } from "node:crypto";

if (!process.send) throw new Error("capture fixture requires private parent IPC");
const requireWorker = createRequire(pathToFileURL(process.argv[1]));
const { default: playwright } = await import(pathToFileURL(requireWorker.resolve("playwright-core")).href);
const contexts = new Map(), gates = new Map();
const launch = playwright.chromium.launchPersistentContext.bind(playwright.chromium);
playwright.chromium.launchPersistentContext = async (dir, options) => {
  const context = await launch(dir, options);
  const profile = basename(dir);
  contexts.set(profile, context);
  const attach = page => {
    const screenshot = page.screenshot.bind(page);
    page.screenshot = async (...args) => {
      const gate = gates.get(page);
      if (!gate) return screenshot(...args);
      if (gate.started) throw new Error("fixture capture gate reused");
      gate.started = true;
      try {
        const image = await screenshot(...args);
        process.send({ kind: "capture-held", id: gate.id, bytes: image.length,
          sha256: createHash("sha256").update(image).digest("hex") });
        await gate.pending;
        return image;
      } finally {
        clearTimeout(gate.timer);
        gates.delete(page);
      }
    };
  };
  context.on("page", attach);
  for (const page of context.pages()) attach(page);
  context.once("close", () => contexts.delete(profile));
  return context;
};

process.on("message", async message => {
  try {
    if (message?.kind === "release-all") {
      for (const gate of gates.values()) gate.release();
    } else {
      const context = contexts.get(message?.profile);
      if (!context || !Number.isSafeInteger(message.page) || message.page < 0) throw new Error("unknown fixture page");
      const page = context.pages()[message.page];
      if (!page) throw new Error("missing fixture page");
      if (message.kind === "arm") {
        if (gates.has(page)) throw new Error("fixture gate occupied");
        let release, reject;
        const pending = new Promise((resolve, fail) => { release = resolve; reject = fail; });
        // Observe rejection even if a native screenshot has not yet returned.
        pending.catch(() => {});
        const timer = setTimeout(() => reject(new Error("fixture gate watchdog")), 15000);
        gates.set(page, { id: message.id, release, pending, timer, started: false });
      } else if (message.kind === "release") {
        if (!gates.has(page)) throw new Error("missing fixture gate");
        gates.get(page).release();
      } else if (message.kind === "state") {
        // Read only the owned fixture's independently maintained postconditions.
        const state = await page.evaluate(() => ({
          clicks: Number(document.getElementById("count")?.textContent),
          value: document.getElementById("value")?.value,
        }));
        process.send({ kind: "reply", id: message.id, state });
        return;
      } else throw new Error("unknown fixture command");
    }
    process.send({ kind: "reply", id: message.id, ok: true });
  } catch {
    process.send({ kind: "reply", id: message?.id, error: "fixture command failed" });
  }
});
