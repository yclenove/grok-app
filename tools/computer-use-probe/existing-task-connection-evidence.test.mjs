import assert from "node:assert/strict";
import test from "node:test";
import { EventEmitter } from "node:events";
import { NativeConnectionEvidence, sanitizeConnectionEvidence } from "./existing-task-connection-evidence.mjs";
import { attachNativeWorker, nativeEvaluationFailure } from "./extension-native-worker.mjs";

const canary = "never-record-this-private-payload";
const request = (requestId, route) => ({ method: "Network.requestWillBeSent", params: { requestId,
  request: { url: `http://127.0.0.1:12345${route}?secret=${canary}`, headers: { Authorization: canary }, postData: canary } } });
const response = (requestId, status) => ({ method: "Network.responseReceived", params: { requestId,
  response: { status, url: canary, headers: { "set-cookie": canary } } } });

test("exact owned request metadata survives worker retirement without retaining secret payloads", () => {
  const evidence = new NativeConnectionEvidence();
  evidence.accept(request("pair-secret-id", "/cu/pairing-confirm"));
  evidence.accept(response("pair-secret-id", 200));
  evidence.accept(response("pair-secret-id", 200));
  evidence.accept(request("poll-secret-id", "/cu/extension-actions/poll"));
  evidence.accept(response("poll-secret-id", 403));
  evidence.accept(request("disconnect-secret-id", "/cu/extension-disconnect"));
  evidence.accept({ method: "Network.loadingFailed", params: { requestId: "disconnect-secret-id", errorText: canary } });
  evidence.accept({ method: "Runtime.executionContextsCleared" });
  evidence.markWorkerStopped(); evidence.markDetached();
  const snapshot = evidence.snapshot();
  assert.equal(snapshot.pairingConfirmed, 1);
  assert.equal(snapshot.disconnectRequests, 1);
  assert.equal(snapshot.contextsCleared, 1);
  assert.equal(snapshot.workerStopped, true);
  assert.equal(snapshot.detached, true);
  assert.deepEqual(snapshot.http.map(row => [row.route, row.status, row.outcome]), [
    ["/cu/pairing-confirm", 200, "response"], ["/cu/extension-actions/poll", 403, "response"],
    ["/cu/extension-disconnect", null, "failed"],
  ]);
  assert.equal(JSON.stringify(snapshot).includes(canary), false);
  assert.equal(JSON.stringify(snapshot).includes("secret-id"), false);
});

test("original native evaluation distinguishes protocol and exception without retaining payloads", async () => {
  for (const protocol of [false, true]) {
    const cdp = new EventEmitter();
    cdp.send = async (method, input) => {
      if (method === "Target.attachToTarget") return { sessionId: "original-session" };
      if (method === "Target.sendMessageToTarget") {
        const command = JSON.parse(input.message);
        const failure = protocol ? { error: { code: -32000, message: "private-binding-and-url" } }
          : { result: { exceptionDetails: { exception: { className: "TypeError", description: "private-binding-and-url" } } } };
        queueMicrotask(() => cdp.emit("Target.receivedMessageFromTarget", {
          sessionId: "original-session", message: JSON.stringify({ id: command.id, ...failure }),
        }));
      }
    };
    cdp.detach = async () => {};
    const worker = await attachNativeWorker(cdp, "owned-target", "fixed-source-worker");
    try {
      await assert.rejects(worker.evaluate(() => {}), error => {
        assert.equal(error.message, "native worker evaluation failed");
        assert.deepEqual(nativeEvaluationFailure(error), { kind: protocol ? "protocol" : "exception",
          exceptionClass: protocol ? "other" : "TypeError" });
        assert.equal(JSON.stringify(error).includes("private-binding-and-url"), false);
        return true;
      });
    } finally { await worker.dispose(); }
  }
  assert.deepEqual(nativeEvaluationFailure({ nativeFailure: "private-binding-and-url",
    nativeExceptionClass: "private-binding-and-url" }), { kind: "other", exceptionClass: "other" });
});

test("network metadata remains bounded and ignores unrelated origins and routes", () => {
  const evidence = new NativeConnectionEvidence();
  for (let index = 0; index < 100; index++) evidence.accept(request(`request-${index}`, "/cu/extension-status"));
  evidence.accept(response("request-0", 200));
  evidence.accept(response("request-99", 401));
  evidence.accept(request("unrelated", "/fixture"));
  const remote = request("remote", "/cu/pairing-confirm");
  remote.params.request.url = "https://example.invalid/cu/pairing-confirm";
  evidence.accept(remote); evidence.accept(response("remote", 200));
  const snapshot = evidence.snapshot();
  assert.equal(snapshot.http.length, 32);
  assert.equal(snapshot.http.at(-1).status, 401);
  assert.equal(snapshot.http.every(row => row.route === "/cu/extension-status"), true);
  assert.equal(snapshot.pairingConfirmed, 0);
  const sanitized = sanitizeConnectionEvidence({ ...snapshot, secret: canary,
    http: [...snapshot.http, { route: "/cu/extension-status", status: 600, elapsedMs: -1, headers: canary }] });
  assert.equal(sanitized.http.at(-1).status, null);
  assert.equal(sanitized.http.at(-1).elapsedMs, null);
  assert.equal(JSON.stringify(sanitized).includes(canary), false);
});

test("original failed pairing request distinguishes network, policy and CORS without private parameters", () => {
  const evidence = new NativeConnectionEvidence();
  const failures = [
    { errorText: "net::ERR_CONNECTION_REFUSED" },
    { errorText: "net::ERR_BLOCKED_BY_CSP", blockedReason: "csp" },
    { errorText: "net::ERR_FAILED", corsErrorStatus: { corsError: "LocalNetworkAccessPermissionDenied", failedParameter: canary } },
    { errorText: "net::ERR_ABORTED", canceled: true },
  ];
  for (const [index, failure] of failures.entries()) {
    evidence.accept(request(`private-failed-${index}`, "/cu/pairing-challenge"));
    evidence.accept({ method: "Network.loadingFailed", params: { requestId: `private-failed-${index}`, ...failure } });
  }
  const snapshot = evidence.snapshot();
  assert.deepEqual(snapshot.http.map(row => [row.status, row.outcome, row.failure]), [
    [null, "failed", { networkError: "net::ERR_CONNECTION_REFUSED", blockedReason: null, corsError: null }],
    [null, "failed", { networkError: "net::ERR_BLOCKED_BY_CSP", blockedReason: "csp", corsError: null }],
    [null, "failed", { networkError: "net::ERR_FAILED", blockedReason: null, corsError: "LocalNetworkAccessPermissionDenied" }],
    [null, "cancelled", { networkError: "net::ERR_ABORTED", blockedReason: null, corsError: null }],
  ]);
  assert.equal(JSON.stringify(snapshot).includes(canary), false);
  assert.equal(JSON.stringify(snapshot).includes("private-failed"), false);
});

test("unrecognized loading failures and persisted failure fields cannot leak raw diagnostics", () => {
  const evidence = new NativeConnectionEvidence();
  evidence.accept(request("private-failed-id", "/cu/pairing-challenge"));
  evidence.accept({ method: "Network.loadingFailed", params: { requestId: "private-failed-id",
    errorText: `net::ERR_FAILED ${canary}`, blockedReason: canary,
    corsErrorStatus: { corsError: canary, failedParameter: canary } } });
  const snapshot = evidence.snapshot();
  assert.deepEqual(snapshot.http[0].failure, { networkError: "other", blockedReason: "other", corsError: "other" });
  snapshot.http[0].failure.private = canary;
  snapshot.http[0].failure.networkError = canary;
  const sanitized = sanitizeConnectionEvidence(snapshot);
  assert.deepEqual(sanitized.http[0].failure, { networkError: "other", blockedReason: "other", corsError: "other" });
  assert.equal(JSON.stringify(sanitized).includes(canary), false);
});

test("native attachment records only its own session, without querying a failed worker", async () => {
  const cdp = new EventEmitter();
  const commands = [];
  cdp.send = async (method, input) => {
    commands.push(method === "Target.sendMessageToTarget" ? JSON.parse(input.message).method : method);
    if (method === "Target.attachToTarget") return { sessionId: "original-session" };
    if (method === "Target.sendMessageToTarget") {
      const command = JSON.parse(input.message);
      queueMicrotask(() => cdp.emit("Target.receivedMessageFromTarget", {
        sessionId: "original-session", message: JSON.stringify({ id: command.id, result: {} }),
      }));
    }
  };
  cdp.detach = async () => {};
  const worker = await attachNativeWorker(cdp, "owned-target", "fixed-source-worker", { connectionEvidence: true });
  cdp.emit("Target.receivedMessageFromTarget", { sessionId: "other-session", message: JSON.stringify(request("foreign", "/cu/pairing-confirm")) });
  cdp.emit("Target.receivedMessageFromTarget", { sessionId: "original-session", message: JSON.stringify(request("original", "/cu/extension-status")) });
  cdp.emit("Target.receivedMessageFromTarget", { sessionId: "original-session", message: JSON.stringify(response("original", 429)) });
  cdp.emit("Target.detachedFromTarget", { sessionId: "original-session" });
  assert.equal(worker.connectionEvidence().http.length, 1);
  assert.equal(worker.connectionEvidence().http[0].status, 429);
  assert.equal(worker.connectionEvidence().detached, true);
  assert.deepEqual(commands, ["Target.attachToTarget", "Network.enable"]);
  await worker.dispose();
  assert.equal(cdp.listenerCount("Target.receivedMessageFromTarget"), 0);
  assert.equal(cdp.listenerCount("Target.detachedFromTarget"), 0);
});
