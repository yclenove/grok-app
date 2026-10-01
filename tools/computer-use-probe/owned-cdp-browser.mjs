// Browser-level CDP over a private pipe, for disposable probe profiles only.
import { spawn } from "node:child_process";
import { once } from "node:events";
import { readFile } from "node:fs/promises";
import { readFileSync } from "node:fs";
import path from "node:path";
import { setTimeout as delay } from "node:timers/promises";

export class OwnedCdpBrowser {
  #child;
  #pending = new Map();
  #nextId = 0;
  #buffer = Buffer.alloc(0);
  #exit;
  #profile;
  #previousEndpoint;

  constructor(executablePath, profile) {
    this.#profile = profile;
    try { this.#previousEndpoint = readFileSync(path.join(profile, "DevToolsActivePort"), "utf8"); }
    catch (error) { if (error.code !== "ENOENT") throw error; }
    this.#child = spawn(executablePath, [
      `--user-data-dir=${profile}`, "--remote-debugging-pipe", "--headless=new",
      "--remote-debugging-port=0", "--remote-debugging-address=127.0.0.1",
      "--enable-unsafe-extension-debugging", "--disable-background-networking",
      "--no-first-run", "--no-default-browser-check", "about:blank",
    ], { stdio: ["ignore", "ignore", "ignore", "pipe", "pipe"], windowsHide: true });
    this.#exit = once(this.#child, "exit");
    void this.#exit.catch(() => {});
    const fail = () => {
      for (const request of this.#pending.values()) request.reject(new Error("owned browser pipe closed"));
      this.#pending.clear();
    };
    this.#child.once("error", fail);
    this.#child.once("exit", fail);
    this.#child.stdio[3].on("error", fail);
    this.#child.stdio[4].on("data", data => {
      this.#buffer = Buffer.concat([this.#buffer, data]);
      if (this.#buffer.length > 1024 * 1024) { fail(); return; }
      let end;
      while ((end = this.#buffer.indexOf(0)) !== -1) {
        const message = this.#buffer.subarray(0, end).toString("utf8");
        this.#buffer = this.#buffer.subarray(end + 1);
        if (!message) continue;
        let reply;
        try { reply = JSON.parse(message); } catch { fail(); continue; }
        const request = this.#pending.get(reply.id);
        if (!request) continue;
        this.#pending.delete(reply.id);
        if (reply.error) request.reject(new Error(`${request.method}: ${reply.error.message}`));
        else request.resolve(reply.result);
      }
    });
  }

  async send(method, params = {}, sessionId) {
    const id = ++this.#nextId;
    let timer;
    const result = new Promise((resolve, reject) => {
      this.#pending.set(id, { method, resolve, reject });
      timer = setTimeout(() => {
        this.#pending.delete(id); reject(new Error(`owned browser ${method} timeout`));
      }, 10000);
      this.#child.stdio[3].write(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }) + "\0");
    });
    try { return await result; } finally { clearTimeout(timer); }
  }

  async page(url) {
    const { targetId } = await this.send("Target.createTarget", { url });
    const { sessionId } = await this.send("Target.attachToTarget", { targetId, flatten: true });
    return { sessionId, targetId };
  }

  async playwrightEndpoint() {
    // Chrome writes this only inside our freshly owned disposable profile.
    // The random loopback port carries no ordinary user's browser session.
    const deadline = Date.now() + 10000;
    while (Date.now() < deadline) {
      let value;
      try { value = await readFile(path.join(this.#profile, "DevToolsActivePort"), "utf8"); }
      catch (error) { if (error.code !== "ENOENT") throw error; }
      if (value && value !== this.#previousEndpoint) {
        const [portText, browserPath] = value.trim().split(/\r?\n/);
        const port = Number(portText);
        if (!Number.isInteger(port) || port < 1 || port > 65535
          || !/^\/devtools\/browser\/[a-f0-9-]+$/i.test(browserPath)) throw new Error("invalid owned browser debugging endpoint");
        return `ws://127.0.0.1:${port}${browserPath}`;
      }
      await delay(25);
    }
    throw new Error("owned browser debugging endpoint did not rotate");
  }

  async evaluate(page, expression) {
    const result = await this.send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true }, page.sessionId);
    if (result.exceptionDetails) throw new Error("owned probe page evaluation failed");
    return result.result.value;
  }

  async close() {
    if (this.#child.exitCode !== null || this.#child.signalCode !== null) return;
    try { await this.send("Browser.close"); } catch { /* Exit can precede the close response. */ }
    this.#child.stdio[3].end();
    this.#child.stdio[4].destroy();
    let timer;
    try {
      await Promise.race([this.#exit, new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error("owned browser did not exit")), 10000);
      })]);
    } finally { clearTimeout(timer); }
  }
}
