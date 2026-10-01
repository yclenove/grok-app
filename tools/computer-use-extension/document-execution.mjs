// Serialized into the exact isolated main-frame document by explicit Share, never by observation.
export function fenceDocument(identity) {
  const valid = value => value && typeof value === "object" && !Array.isArray(value)
    && Object.keys(value).length === 2 && Number.isSafeInteger(value.epoch) && value.epoch > 0
    && Number.isSafeInteger(value.sequence) && value.sequence > 0;
  if (!valid(identity)) throw new Error("executionUnavailable");
  const previous = globalThis.__grokComputerUseExecution;
  if (previous && (previous.protocol !== 1 || !valid({ epoch: previous.epoch, sequence: previous.sequence })
    || identity.epoch < previous.epoch
    || (identity.epoch === previous.epoch && (previous.closed || identity.sequence <= previous.sequence)))) {
    throw new Error("stale execution");
  }
  if (globalThis.__grokComputerUseSnapshot?.operation) throw new Error("document busy");
  globalThis.__grokComputerUseSnapshot?.retire?.();
  const state = { protocol: 1, epoch: identity.epoch, sequence: identity.sequence, closed: false };
  state.current = () => globalThis.__grokComputerUseExecution === state && !state.closed;
  state.admit = next => {
    if (!state.current() || !valid(next) || next.epoch !== state.epoch || next.sequence <= state.sequence) {
      throw new Error("stale execution");
    }
    state.sequence = next.sequence;
  };
  state.close = () => { state.closed = true; };
  globalThis.__grokComputerUseExecution = state;
  return { epoch: state.epoch, sequence: state.sequence };
}
