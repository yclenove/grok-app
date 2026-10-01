import { ExtensionActionTransport } from "./action-transport.mjs";

const uuid = value => typeof value === "string" && /^[0-9a-f-]{36}$/i.test(value);
const positive = value => Number.isSafeInteger(value) && value > 0;
const boundedId = value => typeof value === "string" && value.length > 0 && value.length <= 256;
function keys(value, allowed) {
  return value && typeof value === "object" && !Array.isArray(value)
    && Object.keys(value).length === allowed.length && allowed.every(key => Object.hasOwn(value, key));
}

export function validRequest(request, sequence, now = Date.now()) {
  return keys(request, ["protocol", "requestId", "sequence", "deadlineMs", "connection", "session", "runId",
    "tabId", "documentGeneration", "grantGeneration", "command"])
    && request.protocol === 1 && uuid(request.requestId) && positive(request.sequence) && request.sequence > sequence
    && positive(request.deadlineMs) && request.deadlineMs > now && request.deadlineMs <= now + 11000
    && keys(request.connection, ["instanceId", "connectionNonce", "generation"])
    && boundedId(request.connection.instanceId) && uuid(request.connection.connectionNonce) && positive(request.connection.generation)
    && boundedId(request.session) && boundedId(request.runId) && typeof request.tabId === "string"
    && /^(0|[1-9]\d{0,15})$/.test(request.tabId) && Number.isSafeInteger(Number(request.tabId))
    && positive(request.documentGeneration) && positive(request.grantGeneration)
    && keys(request.command, ["kind", "snapshotId", "screenshot", "preview"]) && request.command.kind === "observe"
    && uuid(request.command.snapshotId) && typeof request.command.screenshot === "boolean" && typeof request.command.preview === "boolean";
}

function pause(signal) {
  return new Promise(resolve => {
    if (signal.aborted) { resolve(); return; }
    const finish = () => { clearTimeout(timer); signal.removeEventListener("abort", finish); resolve(); };
    const timer = setTimeout(finish, 250);
    signal.addEventListener("abort", finish, { once: true });
  });
}

export class ExtensionTransport {
  #controller = null;
  #actions = null;
  constructor({ client, observe, act, journal, recovered, wait = pause }) {
    this.client = client; this.observe = observe; this.wait = wait;
    if (act) this.#actions = new ExtensionActionTransport({ client, act, wait, journal, recovered });
  }
  reset() { this.#controller?.abort(); this.#controller = null; this.#actions?.reset(); }
  retryCleanup(tabId) { return this.#actions?.retryCleanup(tabId); }
  start() {
    this.reset();
    const epoch = this.client.connectionEpoch;
    if (epoch === null) return;
    const controller = new AbortController(); this.#controller = controller;
    const retire = source => {
      if (!controller.signal.aborted) return this.client.retireTransport(epoch, source);
    };
    // Both loops retain their original promises; a reset cannot orphan a browser action.
    this.task = Promise.allSettled([
      this.#run(epoch, controller.signal).catch(() => retire("observation")),
      this.#actions?.run(epoch, controller.signal).catch(() => retire("action")),
    ]).then(() => {});
  }
  async #run(epoch, signal) {
    let sequence = 0;
    while (!signal.aborted && this.client.connectionEpoch === epoch) {
      const request = await this.client.pollExtension(epoch, signal);
      if (signal.aborted || this.client.connectionEpoch !== epoch) return;
      if (request !== null) {
        if (!validRequest(request, sequence)) throw new Error("invalid extension request");
        sequence = request.sequence;
        let outcome;
        try { outcome = { kind: "observation", observation: await this.observe(request) }; }
        catch { outcome = { kind: "rejected", reason: "documentChanged" }; }
        if (signal.aborted || this.client.connectionEpoch !== epoch) return;
        if (Date.now() >= request.deadlineMs) outcome = { kind: "rejected", reason: "deadline" };
        // A rejected/ambiguous result is never resent. Next poll rechecks the connection.
        try { await this.client.completeExtension(epoch, { request, outcome }, signal); }
        catch { if (signal.aborted || this.client.connectionEpoch !== epoch) return; }
      }
      await this.wait(signal);
    }
  }
}
