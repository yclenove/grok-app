// Chrome keeps a service-worker target across stop/start, while Playwright may cache its dead context.
// A fresh native attachment is only for fault injection in this owned probe; it is not product code.
export async function attachNativeWorker(cdp, targetId, url) {
  const { sessionId } = await cdp.send("Target.attachToTarget", { targetId, flatten: false });
  let sequence = 0; const pending = new Map();
  const onMessage = event => {
    if (event.sessionId !== sessionId) return;
    const message = JSON.parse(event.message);
    const request = pending.get(message.id);
    if (!request) return;
    pending.delete(message.id); clearTimeout(request.timer);
    if (message.error || message.result?.exceptionDetails) request.reject(new Error("native worker evaluation failed"));
    else request.resolve(message.result?.result?.value);
  };
  cdp.on("Target.receivedMessageFromTarget", onMessage);
  return {
    url: () => url,
    evaluate(fn, argument) {
      const id = ++sequence;
      return new Promise((resolve, reject) => {
        const timer = setTimeout(() => { pending.delete(id); reject(new Error("native worker evaluation timeout")); }, 12000);
        pending.set(id, { resolve, reject, timer });
        void cdp.send("Target.sendMessageToTarget", { sessionId, message: JSON.stringify({ id, method: "Runtime.evaluate",
          params: { expression: `(${fn.toString()})(${JSON.stringify(argument) ?? "undefined"})`,
            awaitPromise: true, returnByValue: true } }) }).catch(() => {
          pending.delete(id); clearTimeout(timer); reject(new Error("native worker attachment unavailable"));
        });
      });
    },
    async dispose() {
      cdp.off("Target.receivedMessageFromTarget", onMessage);
      for (const request of pending.values()) { clearTimeout(request.timer); request.reject(new Error("native worker detached")); }
      pending.clear();
      await cdp.send("Target.detachFromTarget", { sessionId }).catch(() => {});
      await cdp.detach();
    },
  };
}
