// Fixture-only CDP pause of the original owned MV3 worker. No clock or token edits.
import assert from "node:assert/strict";
import { setTimeout as delay } from "node:timers/promises";

export async function pauseOwnedWorker(context, popup, workerUrl) {
  const session = await context.newCDPSession(popup);
  let workerSession, pending, paused = false, nextId = 0;
  const wait = async predicate => {
    const deadline = performance.now() + 15000;
    while (performance.now() < deadline) {
      if (predicate()) return;
      await delay(25);
    }
    throw new Error("original owned worker debugger did not settle");
  };
  session.on("Target.receivedMessageFromTarget", event => {
    if (event.sessionId !== workerSession) return;
    const message = JSON.parse(event.message);
    if (message.method === "Debugger.paused") paused = true;
    if (message.method === "Debugger.resumed") paused = false;
    if (pending && message.id === pending.id) {
      pending.done = true;
      pending.failed = message.error !== undefined;
    }
  });
  const command = async method => {
    assert(["Debugger.enable", "Debugger.pause", "Debugger.resume"].includes(method));
    assert.equal(pending, undefined, "original worker debugger commands are serial");
    pending = { id: ++nextId, done: false, failed: false };
    try {
      await session.send("Target.sendMessageToTarget", {
        sessionId: workerSession, message: JSON.stringify({ id: pending.id, method, params: {} }),
      });
      await wait(() => pending.done);
      assert.equal(pending.failed, false, "original worker debugger command rejected");
    } finally { pending = undefined; }
  };
  const resume = async () => {
    if (paused) { await command("Debugger.resume"); await wait(() => !paused); }
    if (workerSession) {
      await session.send("Target.detachFromTarget", { sessionId: workerSession });
      workerSession = undefined;
    }
    await session.detach();
  };
  try {
    const { targetInfos } = await session.send("Target.getTargets");
    const targets = targetInfos.filter(row => row.type === "service_worker" && row.url === workerUrl);
    assert.equal(targets.length, 1, "exactly one original owned source worker");
    ({ sessionId: workerSession } = await session.send("Target.attachToTarget", {
      targetId: targets[0].targetId, flatten: false,
    }));
    await command("Debugger.enable");
    await command("Debugger.pause");
    await wait(() => paused);
    return { resume };
  } catch (error) {
    try { await resume(); } catch { /* Original context/Job owner still owns teardown. */ }
    throw error;
  }
}
