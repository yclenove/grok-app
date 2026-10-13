import { ActionCompletions, copyBoundAction } from "./action-completions.mjs";
import { ExecutionSuspended, waitExecutionReady } from "./pairing-client.mjs";

// One owner survives transport resets. Cleanup scopes never depend on current pairing.
export class ExtensionActionTransport {
  #client;
  #completions;
  #wait;
  #epoch = null;
  #sequence = 0;
  constructor({ client, act, wait, journal, recovered }) {
    if (!journal) throw new Error("actionJournalRequired");
    if (typeof recovered !== "function") throw new Error("actionRecoveryRequired");
    this.#client = client;
    this.#wait = wait;
    this.#completions = new ActionCompletions({ client, act, journal, recovered });
  }

  reset() { this.#completions.reset(); }
  idle() { return this.#completions.idle(); }
  retryCleanup(tabId) { return this.#completions.retryCleanup(tabId); }

  async run(epoch, signal) {
    // Failed or legacy negotiation leaves the existing observation channel intact.
    for (;;) {
      try { await this.#client.negotiateActions(epoch, signal); break; }
      catch (error) {
        if (!(error instanceof ExecutionSuspended)
          || !await waitExecutionReady(this.#client, epoch, signal, this.#wait)) return;
      }
    }
    if (signal.aborted || this.#client.connectionEpoch !== epoch) return;
    if (this.#epoch !== epoch) { this.#epoch = epoch; this.#sequence = 0; }
    while (!signal.aborted && this.#client.connectionEpoch === epoch) {
      let dispatch;
      try { dispatch = await this.#client.pollAction(epoch, signal); }
      catch (error) {
        if (!(error instanceof ExecutionSuspended)) throw error;
        if (!await waitExecutionReady(this.#client, epoch, signal, this.#wait)) return;
        continue;
      }
      if (signal.aborted || this.#client.connectionEpoch !== epoch) return;
      if (dispatch !== null) {
        const { request, proof } = copyBoundAction(dispatch.request, dispatch.proof);
        if (request.sequence <= this.#sequence) throw new Error("actionSequenceChanged");
        this.#sequence = request.sequence;
        const outcome = await this.#completions.run(epoch, request, proof, signal);
        if (signal.aborted || this.#client.connectionEpoch !== epoch) return;
        // Unknown delivery is never resent. Only independent cleanup can retry.
        try { await this.#client.completeAction(epoch, { request, proof, outcome }, signal); }
        catch { if (signal.aborted || this.#client.connectionEpoch !== epoch) return; }
      }
      await this.#wait(signal);
    }
  }
}
