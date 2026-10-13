import { ExtensionActionTransport } from "./action-transport.mjs";
import { ExecutionSuspended, waitExecutionReady } from "./pairing-client.mjs";
import { ObservationCompletions, copyBoundObservation } from "./observation-completions.mjs";

export function validRequest(request, sequence, now = Date.now()) {
  try { return copyBoundObservation(request, now).sequence > sequence; } catch { return false; }
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
  #observations;
  constructor({ client, observe, act, journal, observationJournal, recovered, wait = pause }) {
    this.client = client; this.wait = wait;
    this.#observations = new ObservationCompletions({ client, observe, journal: observationJournal });
    if (act) this.#actions = new ExtensionActionTransport({ client, act, wait, journal, recovered });
  }
  reset() { this.#controller?.abort(); this.#controller = null; this.#actions?.reset(); this.#observations.reset(); }
  retryCleanup(tabId) { return Promise.allSettled([this.#actions?.retryCleanup(tabId), this.#observations.retryCleanup(tabId)]); }
  start() {
    this.reset();
    const epoch = this.client.connectionEpoch;
    if (epoch === null) return;
    const controller = new AbortController(); this.#controller = controller;
    const retire = source => {
      if (!controller.signal.aborted) return this.client.retireTransport(epoch, source);
    };
    // Both loops retain their original promises; a reset cannot orphan a browser action.
    this.task = waitExecutionReady(this.client, epoch, controller.signal, this.wait).then(ready => {
      if (!ready) return;
      return Promise.allSettled([
        this.#run(epoch, controller.signal).catch(() => retire("observation")),
        this.#actions?.run(epoch, controller.signal).catch(() => retire("action")),
      ]);
    }).catch(() => retire("transport")).then(() => {});
  }

  async #run(epoch, signal) {
    for (;;) {
      try { await this.client.negotiateObservations(epoch, signal); break; }
      catch (error) {
        if (!(error instanceof ExecutionSuspended)) throw error;
        if (!await waitExecutionReady(this.client, epoch, signal, this.wait)) return;
      }
    }
    let sequence = 0;
    while (!signal.aborted && this.client.connectionEpoch === epoch) {
      let request;
      try { request = await this.client.pollExtension(epoch, signal); }
      catch (error) {
        if (!(error instanceof ExecutionSuspended)) throw error;
        if (!await waitExecutionReady(this.client, epoch, signal, this.wait)) return;
        continue;
      }
      if (signal.aborted || this.client.connectionEpoch !== epoch) return;
      if (request !== null) {
        if (!validRequest(request, sequence)) throw new Error("invalid extension request");
        sequence = request.sequence;
        const outcome = await this.#observations.run(epoch, request, signal);
        if (signal.aborted || this.client.connectionEpoch !== epoch) return;
        // A rejected/ambiguous result is never resent. Next poll rechecks the connection.
        try { if (outcome !== null) await this.client.completeExtension(epoch, { request, outcome }, signal); }
        catch { if (signal.aborted || this.client.connectionEpoch !== epoch) return; }
      }
      await this.wait(signal);
    }
  }
}
