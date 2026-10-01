import { workerException } from "./worker-errors.mjs";

// Transfer the admitted request's occupancy rather than acquiring another slot:
// capacity exhaustion must not allow unconfirmed physical cleanup to look idle.
export class InputCleanup {
  #closed = false;
  #pending = false;
  #requests = new Set();

  constructor(context) {
    context.once("close", () => {
      this.#closed = true;
      for (const request of this.#requests) request.finish();
      this.#requests.clear();
    });
  }

  retain(request) {
    this.#pending = true;
    if (this.#closed) request.finish();
    else this.#requests.add(request);
  }

  check() {
    if (this.#pending) {
      throw workerException(409, "input_quarantined",
        "close the owned profile before admitting more input");
    }
  }
}
