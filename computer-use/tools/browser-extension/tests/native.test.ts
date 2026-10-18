// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { webcrypto } from "node:crypto";
import { NativeChannel, NATIVE_HOST, fixedExtensionId, type NativePort } from "../native";
import type { DomRequest, DomResult } from "../shared";
import { SharingController } from "../background";

function event<T extends (...args: never[]) => unknown>() {
  const listeners = new Set<T>();
  return { addListener: (fn: T) => { listeners.add(fn); }, removeListener: (fn: T) => { listeners.delete(fn); },
    fire: (...args: Parameters<T>) => { for (const fn of [...listeners]) fn(...args); } };
}
const key = "AQIDBA=="; // Public fixture bytes; never a production signing key.
const connectionId = "a".repeat(64);
const scope = { tabId: 7, documentId: "document-1", shareId: "share-1" };
const request: DomRequest = { requestId: "native-1", sessionId: "session-a", shareId: "share-1", operation: { type: "observe" } };
async function harness(publicKey: unknown = key, id?: string) {
  const port = { onMessage: event<(message: unknown) => void>(), onDisconnect: event<() => void>(), postMessage: vi.fn(), disconnect: vi.fn() };
  const runtime = { id: id ?? (await fixedExtensionId(key))!, connectNative: vi.fn((): NativePort => port) };
  const channel = new NativeChannel(runtime, publicKey);
  const invalidate = vi.fn();
  const dispatch = vi.fn(async (req: DomRequest): Promise<DomResult> => channel.authorize(req, scope) ? { kind: "action", outcome: "done" } : { kind: "error", code: "approval_denied" });
  channel.listen(dispatch, invalidate);
  channel.shared(scope);
  await vi.waitFor(() => expect(runtime.connectNative.mock.calls.length + invalidate.mock.calls.length).toBeGreaterThan(0));
  const hello = () => port.onMessage.fire({ kind: "hello", protocol: 1, connectionId });
  const ack = () => port.onMessage.fire({ kind: "sharedAck", connectionId, shareId: scope.shareId });
  return { port, runtime, channel, invalidate, dispatch, hello, ack };
}
beforeEach(() => { vi.stubGlobal("crypto", webcrypto); });
afterEach(() => { vi.unstubAllGlobals(); vi.restoreAllMocks(); });

describe("native messaging background boundary", () => {
  it("missing_key_or_wrong_fixed_id_never_connects", async () => {
    expect(await fixedExtensionId(undefined)).toBeUndefined();
    const cases: Array<[unknown, string | undefined]> = [[null, undefined], ["__MAINTAINER_PUBLIC_KEY__", undefined], [key, "wrong-extension"]];
    for (const [publicKey, id] of cases) {
      const h = await harness(publicKey, id);
      expect(h.runtime.connectNative).not.toHaveBeenCalled();
      expect(h.invalidate).toHaveBeenCalledOnce();
    }
  });
  it("handshake_announces_browser_scope_and_only_authorizes_native_one_shot", async () => {
    const h = await harness();
    expect(h.runtime.connectNative).toHaveBeenCalledWith(NATIVE_HOST);
    expect(h.channel.authorize(request, scope)).toBe(false);
    h.hello();
    expect(h.port.postMessage).toHaveBeenNthCalledWith(1, { kind: "ready", protocol: 1, connectionId });
    expect(h.port.postMessage).toHaveBeenNthCalledWith(2, { kind: "shared", connectionId, scope: { ...scope, frameId: 0 } });
    h.ack();
    h.port.onMessage.fire({ kind: "request", connectionId, request });
    await vi.waitFor(() => expect(h.port.postMessage).toHaveBeenCalledWith({ kind: "response", connectionId, requestId: "native-1", result: { kind: "action", outcome: "done" } }));
    expect(h.channel.authorize(request, scope)).toBe(false);
    h.port.onMessage.fire({ kind: "request", connectionId, request: { ...request, requestId: "native-2", sessionId: "session-b" } });
    await vi.waitFor(() => expect(h.port.postMessage).toHaveBeenCalledWith(expect.objectContaining({ requestId: "native-2", result: { kind: "error", code: "approval_denied" } })));
    h.channel.revoked(scope);
  });
  it.each(["disconnect", "wrong_connection", "unshared", "page_identity"])("%s_invalidates_immediately_without_retry", async condition => {
    const h = await harness(); h.hello(); h.ack();
    if (condition === "disconnect") h.port.onDisconnect.fire();
    if (condition === "wrong_connection") h.port.onMessage.fire({ kind: "request", connectionId: "old", request });
    if (condition === "unshared") h.port.onMessage.fire({ kind: "request", connectionId, request: { ...request, shareId: "not-shared" } });
    if (condition === "page_identity") h.port.onMessage.fire({ kind: "request", connectionId, request, windowId: 4 });
    expect(h.invalidate).toHaveBeenCalledOnce();
    expect(h.channel.authorize(request, scope)).toBe(false);
    expect(h.runtime.connectNative).toHaveBeenCalledOnce();
    expect(h.dispatch).not.toHaveBeenCalled();
  });
  it("late_result_after_disconnect_is_discarded_and_new_share_needs_new_connection", async () => {
    const h = await harness(); h.hello(); h.ack();
    let finish!: (result: DomResult) => void;
    h.dispatch.mockImplementation(() => new Promise(resolve => { finish = resolve; }));
    h.port.onMessage.fire({ kind: "request", connectionId, request });
    h.port.onDisconnect.fire();
    finish({ kind: "action", outcome: "unknown" });
    await Promise.resolve(); await Promise.resolve();
    expect(h.port.postMessage.mock.calls.some(([message]) => message.kind === "response")).toBe(false);
    expect(h.runtime.connectNative).toHaveBeenCalledOnce();
  });
  it("native_disconnect_clears_real_sharing_controller_content_port", async () => {
    const id = (await fixedExtensionId(key))!;
    const port = { onMessage: event<(message: unknown) => void>(), onDisconnect: event<() => void>(), postMessage: vi.fn(), disconnect: vi.fn() };
    const channel = new NativeChannel({ id, connectNative: () => port }, key);
    const content = { name: "gcu.dom.v2", onMessage: event<(message: unknown) => void>(), onDisconnect: event<() => void>(), postMessage: vi.fn(), disconnect: vi.fn() };
    const controller = new SharingController({ inject: async () => ({ documentId: "doc", port: content }), mark: vi.fn() }, channel);
    await controller.toggle(7, "https://fixture.test/");
    await vi.waitFor(() => expect(port.onDisconnect).toBeDefined());
    // Allow the asynchronous fixed-key digest to finish before delivering disconnect.
    await vi.waitFor(() => { port.onMessage.fire({ kind: "hello", protocol: 1, connectionId }); expect(port.postMessage).toHaveBeenCalled(); });
    port.onDisconnect.fire();
    expect(content.disconnect).toHaveBeenCalledOnce();
    expect(await controller.dispatch(request)).toMatchObject({ code: "app_denied" });
    controller.dispose();
  });
});
