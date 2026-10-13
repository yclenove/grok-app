// Chrome keeps a service-worker target across stop/start, while Playwright may cache its dead context.
// A fresh native attachment is only for fault injection in this owned probe; it is not product code.
import { NativeConnectionEvidence } from "./existing-task-connection-evidence.mjs";

export function nativeEvaluationFailure(error) {
  const kind = ["protocol", "exception"].includes(error?.nativeFailure) ? error.nativeFailure : "other";
  const exceptionClass = ["Error", "TypeError", "ReferenceError", "SyntaxError", "RangeError"].includes(error?.nativeExceptionClass)
    ? error.nativeExceptionClass : "other";
  return { kind, exceptionClass };
}

export async function attachNativeWorker(cdp, targetId, url, { connectionEvidence = false } = {}) {
  const { sessionId } = await cdp.send("Target.attachToTarget", { targetId, flatten: false });
  let sequence = 0; const pending = new Map();
  let moduleCapture = null;
  const evidence = connectionEvidence ? new NativeConnectionEvidence() : null;
  const onMessage = event => {
    if (event.sessionId !== sessionId) return;
    const message = JSON.parse(event.message);
    if (message.method) {
      evidence?.accept(message);
      if (message.method === "Debugger.paused" && moduleCapture) moduleCapture(message.params);
      return;
    }
    const request = pending.get(message.id);
    if (!request) return;
    pending.delete(message.id); clearTimeout(request.timer);
    if (message.error || message.result?.exceptionDetails) {
      const error = new Error("native worker evaluation failed");
      const failure = nativeEvaluationFailure({ nativeFailure: message.error ? "protocol" : "exception",
        nativeExceptionClass: message.result?.exceptionDetails?.exception?.className });
      error.nativeFailure = failure.kind;
      error.nativeExceptionClass = failure.exceptionClass;
      request.reject(error);
    }
    else request.resolve(request.protocolResult ? message.result : message.result?.result?.value);
  };
  const onDetached = event => { if (event.sessionId === sessionId) evidence?.markDetached(); };
  cdp.on("Target.receivedMessageFromTarget", onMessage);
  cdp.on("Target.detachedFromTarget", onDetached);
  const sendNative = (method, params, protocolResult = false) => {
    const id = ++sequence;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => { pending.delete(id); reject(new Error("native worker evaluation timeout")); }, 12000);
      pending.set(id, { resolve, reject, timer, protocolResult });
      void cdp.send("Target.sendMessageToTarget", { sessionId, message: JSON.stringify({ id, method, params }) }).catch(() => {
        pending.delete(id); clearTimeout(timer); reject(new Error("native worker attachment unavailable"));
      });
    });
  };
  const attached = {
    url: () => url,
    evaluate(fn, argument) {
      return sendNative("Runtime.evaluate", {
        expression: `(${fn.toString()})(${JSON.stringify(argument) ?? "undefined"})`,
        awaitPromise: true, returnByValue: true,
      });
    },
    async captureActionModuleBinding(binding, moduleUrl, lineNumber, onArmed) {
      const modules = { SharedTabs: "shared-tabs.mjs", PairingClient: "pairing-client.mjs",
        ActionCompletions: "action-transport.mjs", ObservationCompletions: "extension-transport.mjs" };
      if (!Object.hasOwn(modules, binding) || moduleUrl !== new URL(modules[binding], url).href
        || !Number.isSafeInteger(lineNumber) || lineNumber < 0 || moduleCapture) {
        throw new Error("invalid original action module capture");
      }
      let breakpointId, timer, armedTask, queuedPause;
      let resolveCapture, rejectCapture;
      const captured = new Promise((resolve, reject) => { resolveCapture = resolve; rejectCapture = reject; });
      captured.catch(() => {});
      try {
        await sendNative("Debugger.enable", {});
        moduleCapture = params => {
          if (!breakpointId) { queuedPause = params; return; }
          if (!params.hitBreakpoints?.includes(breakpointId)) return;
          moduleCapture = null;
          void (async () => {
            try {
              const frame = params.callFrames?.[0];
              if (!frame) throw new Error("original module frame missing");
              const value = await sendNative("Debugger.evaluateOnCallFrame", {
                callFrameId: frame.callFrameId, returnByValue: true,
                expression: `(() => { if (typeof ${binding} !== "function") return false; globalThis.__jointActionTypes ??= {}; globalThis.__jointActionTypes.${binding} = ${binding}; return true; })()`,
              });
              if (value !== true) throw new Error("original module binding missing");
              resolveCapture(true);
            } catch (error) { rejectCapture(error); }
            finally { await sendNative("Debugger.resume", {}).catch(() => {}); }
          })();
        };
        const breakpoint = await sendNative("Debugger.setBreakpointByUrl", { url: moduleUrl, lineNumber }, true);
        breakpointId = breakpoint.breakpointId;
        if (typeof breakpointId !== "string") throw new Error("original module breakpoint unavailable");
        if (queuedPause) moduleCapture(queuedPause);
        if (onArmed) {
          armedTask = Promise.resolve().then(onArmed);
          armedTask.catch(rejectCapture);
        }
        timer = setTimeout(() => rejectCapture(new Error("original module capture deadline")), 12000);
        return await captured;
      } finally {
        clearTimeout(timer); moduleCapture = null;
        if (breakpointId) await sendNative("Debugger.removeBreakpoint", { breakpointId }).catch(() => {});
        // Ensure the same original worker continues; no replacement target or module load.
        await sendNative("Debugger.resume", {}).catch(() => {});
        await sendNative("Debugger.disable", {}).catch(() => {});
        await armedTask?.catch(() => {});
      }
    },
    connectionEvidence: () => evidence?.snapshot() ?? null,
    markWorkerStopped: () => evidence?.markWorkerStopped(),
    async dispose() {
      evidence?.markDetached();
      cdp.off("Target.receivedMessageFromTarget", onMessage);
      cdp.off("Target.detachedFromTarget", onDetached);
      for (const request of pending.values()) { clearTimeout(request.timer); request.reject(new Error("native worker detached")); }
      pending.clear();
      await cdp.send("Target.detachFromTarget", { sessionId }).catch(() => {});
      await cdp.detach();
    },
  };
  try {
    // Observe existing requests only. Never fetch, wake, renew or read response bodies.
    if (evidence) await sendNative("Network.enable", { maxPostDataSize: 0 });
    return attached;
  } catch (error) { await attached.dispose(); throw error; }
}
