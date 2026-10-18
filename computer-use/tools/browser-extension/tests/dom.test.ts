// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { pathToFileURL } from "node:url";
import { ContentSession, installContent } from "../content";
import { SharingController, installBackground, type BrowserDriver, type TrustedRouterChannel } from "../background";
import type { DomRequest, DomResult, DomSnapshot, Port, WireMessage } from "../shared";

function event<T extends (...args: never[]) => unknown>() {
  const listeners = new Set<T>();
  return { addListener: (fn: T) => { listeners.add(fn); }, removeListener: (fn: T) => { listeners.delete(fn); },
    fire: (...args: Parameters<T>) => { for (const fn of [...listeners]) fn(...args); } };
}
function ports() {
  const left = { name: "gcu.dom.v2", sender: undefined as Port["sender"], onMessage: event<(message: unknown) => void>(), onDisconnect: event<() => void>(), postMessage: (_message: WireMessage) => {}, disconnect: () => {} };
  const right = { ...left, onMessage: event<(message: unknown) => void>(), onDisconnect: event<() => void>() };
  left.postMessage = message => right.onMessage.fire(message);
  right.postMessage = message => left.onMessage.fire(message);
  left.disconnect = right.disconnect = () => { left.onDisconnect.fire(); right.onDisconnect.fire(); };
  return { left, right };
}
let sequence = 0;
const request = (shareId = "share", operation: DomRequest["operation"] = { type: "observe" }): DomRequest =>
  ({ requestId: `request-${++sequence}`, shareId, sessionId: "fixture-session", operation });
function snapshot(result: DomResult): DomSnapshot {
  expect(result.kind).toBe("snapshot");
  if (result.kind !== "snapshot") throw new Error("expected snapshot");
  return result.snapshot;
}
function nodes(tree: DomSnapshot["root"]): DomSnapshot["root"][] {
  return [tree, ...tree.children.flatMap(nodes)];
}
function harness(authorized = true) {
  const session = new ContentSession(window);
  const wire = ports();
  wire.right.onMessage.addListener(message => {
    const reply = session.handle(message);
    if (reply) wire.right.postMessage(reply);
  });
  wire.right.onDisconnect.addListener(() => session.revoke());
  const browser: BrowserDriver = { inject: vi.fn(async () => ({ documentId: "doc-1", port: wire.left })), mark: vi.fn() };
  let dispatch!: (req: DomRequest) => Promise<DomResult>;
  let invalidate!: () => void;
  const router: TrustedRouterChannel = {
    authorize: vi.fn(() => authorized),
    listen: (onRequest, onInvalidate) => { dispatch = onRequest; invalidate = onInvalidate; return () => {}; },
    shared: vi.fn(), revoked: vi.fn(),
  };
  const controller = new SharingController(browser, router);
  return { session, wire, browser, controller, router, dispatch: (req: DomRequest) => dispatch(req), invalidate: () => invalidate() };
}

beforeEach(() => {
  document.body.innerHTML = '<button id="button">Continue</button><label for="field">Message</label><input id="field">';
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({ x: 10, y: 20, left: 10, top: 20, right: 110, bottom: 50, width: 100, height: 30, toJSON() {} });
});
afterEach(() => { vi.restoreAllMocks(); });

describe("DOM consent and trusted transport", () => {
  it("request_before_sharing_reads_no_tree_and_performs_no_action", () => {
    const session = new ContentSession(window);
    const read = vi.spyOn(document, "documentElement", "get");
    const click = vi.spyOn(HTMLElement.prototype, "click");
    expect(session.handle({ kind: "request", request: request() })?.result).toMatchObject({ kind: "error", code: "app_denied" });
    expect(session.handle({ kind: "request", request: request("share", { type: "click", snapshotId: "old", nodeId: "old" }) })?.result).toMatchObject({ code: "app_denied" });
    expect(read).not.toHaveBeenCalled();
    expect(click).not.toHaveBeenCalled();
  });

  it("share_without_router_or_approval_keeps_tree_and_actions_disabled", async () => {
    const h = harness(false);
    const read = vi.spyOn(document, "documentElement", "get");
    await h.controller.toggle(7, document.URL);
    expect(h.browser.inject).toHaveBeenCalledTimes(1);
    expect(read).not.toHaveBeenCalled();
    const scope = vi.mocked(h.router.shared).mock.calls[0][0];
    expect(await h.dispatch(request(scope.shareId))).toMatchObject({ code: "approval_denied" });
    expect(read).not.toHaveBeenCalled();
    const disabled = new SharingController(h.browser);
    expect(await disabled.dispatch(request())).toMatchObject({ code: "capability_unavailable" });
  });

  it.each(["revoke", "tab_close", "navigation", "stop", "disconnect"])("%s_after_sharing_clears_scope_and_cached_handles", async boundary => {
    const h = harness();
    await h.controller.toggle(7, document.URL);
    const scope = vi.mocked(h.router.shared).mock.calls[0][0];
    const state = snapshot(await h.dispatch(request(scope.shareId)));
    if (boundary === "revoke") await h.controller.toggle(7, document.URL);
    if (boundary === "tab_close" || boundary === "navigation") h.controller.clearTab(7);
    if (boundary === "stop") h.invalidate();
    if (boundary === "disconnect") h.wire.left.disconnect();
    expect(await h.dispatch(request(scope.shareId, { type: "click", snapshotId: state.id, nodeId: state.root.id }))).toMatchObject({ code: "app_denied" });
    expect(h.session.handle({ kind: "request", request: request(scope.shareId) })?.result).toMatchObject({ code: "app_denied" });
  });

  it("navigation_during_injection_cannot_resurrect_sharing", async () => {
    const h = harness();
    let finish!: (value: Awaited<ReturnType<BrowserDriver["inject"]>>) => void;
    vi.mocked(h.browser.inject).mockReturnValue(new Promise(resolve => { finish = resolve; }));
    const pending = h.controller.toggle(7, document.URL);
    h.controller.clearTab(7);
    finish({ documentId: "late-doc", port: h.wire.left });
    await pending;
    expect(h.router.shared).not.toHaveBeenCalled();
  });

  it("late_message_from_old_port_cannot_revoke_a_new_share", async () => {
    const first = ports();
    const second = ports();
    const mark = vi.fn();
    const inject = vi.fn<BrowserDriver["inject"]>()
      .mockResolvedValueOnce({ documentId: "old", port: first.left })
      .mockResolvedValueOnce({ documentId: "new", port: second.left });
    const controller = new SharingController({ inject, mark });
    await controller.toggle(7, document.URL);
    controller.clearTab(7);
    await controller.toggle(7, document.URL);
    mark.mockClear();
    first.right.postMessage({ kind: "revoked" });
    expect(mark).not.toHaveBeenCalled();
    controller.dispose();
  });

  it("content_rejects_page_and_other_tab_ports_without_reading", () => {
    const onConnect = event<(port: Port) => void>();
    installContent({ id: "fixture-extension", onConnect }, window);
    const wire = ports();
    const disconnect = vi.spyOn(wire.right, "disconnect");
    wire.right.sender = { id: "fixture-extension", tab: { id: 8 } };
    onConnect.fire(wire.right);
    expect(disconnect).toHaveBeenCalledOnce();
    wire.right.sender = { id: "another-extension" };
    onConnect.fire(wire.right);
    expect(disconnect).toHaveBeenCalledTimes(2);
  });

  it("browser_navigation_events_revoke_shared_tabs_including_child_frames", async () => {
    const wire = ports();
    const api: Parameters<typeof installBackground>[0] = {
      runtime: { id: "fixture", onConnect: event() }, i18n: { getMessage: key => key },
      action: { onClicked: event(), setTitle: vi.fn(async () => {}), setBadgeText: vi.fn(async () => {}) },
      tabs: { onRemoved: event(), onUpdated: event(), onReplaced: event(), connect: vi.fn(() => wire.left), query: vi.fn(async () => [{ id: 7 }]) },
      scripting: { executeScript: vi.fn(async () => [{ frameId: 0, documentId: "doc-1" }]) },
      webNavigation: { onBeforeNavigate: event(), onCommitted: event(), onHistoryStateUpdated: event(), onReferenceFragmentUpdated: event() },
    };
    const controller = installBackground(api);
    const read = vi.spyOn(document, "documentElement", "get");
    expect(api.scripting.executeScript).not.toHaveBeenCalled();
    await controller.toggle(7, document.URL);
    expect(api.scripting.executeScript).toHaveBeenCalledWith({ target: { tabId: 7, frameIds: [0] }, files: ["content.js"], world: "ISOLATED" });
    expect(api.tabs.connect).toHaveBeenCalledWith(7, { name: "gcu.dom.v2", documentId: "doc-1" });
    const disconnect = vi.spyOn(wire.left, "disconnect");
    (api.webNavigation.onBeforeNavigate as ReturnType<typeof event<(details: { tabId: number }) => void>>).fire({ tabId: 7 });
    expect(disconnect).toHaveBeenCalledOnce();
    expect(read).not.toHaveBeenCalled();
  });

  it("stop_with_a_request_in_flight_returns_unknown_without_replay", async () => {
    const h = harness();
    await h.controller.toggle(7, document.URL);
    const scope = vi.mocked(h.router.shared).mock.calls[0][0];
    const state = snapshot(await h.dispatch(request(scope.shareId)));
    const send = vi.spyOn(h.wire.left, "postMessage").mockImplementation(() => {});
    const pending = h.dispatch(request(scope.shareId, { type: "click", snapshotId: state.id, nodeId: state.root.id }));
    h.invalidate();
    expect(await pending).toMatchObject({ kind: "action", outcome: "unknown" });
    expect(send).toHaveBeenCalledOnce();
    expect(await h.dispatch(request(scope.shareId))).toMatchObject({ code: "app_denied" });
  });

  it("router_session_mismatch_and_replayed_requests_never_reach_content", async () => {
    const h = harness();
    await h.controller.toggle(7, document.URL);
    const scope = vi.mocked(h.router.shared).mock.calls[0][0];
    const req = request(scope.shareId);
    expect((await h.dispatch(req)).kind).toBe("snapshot");
    expect(await h.dispatch(req)).toMatchObject({ code: "stale_element" });
    expect(await h.dispatch({ ...request(scope.shareId), sessionId: "other-session" })).toMatchObject({ code: "app_denied" });
    h.controller.dispose();
  });
});

describe("unified DOM nodes and guarded actions", () => {
  function shared() {
    const session = new ContentSession(window);
    session.handle({ kind: "activate", shareId: "share", url: document.URL });
    const observe = () => snapshot(session.handle({ kind: "request", request: request() })!.result);
    const act = (operation: DomRequest["operation"]) => session.handle({ kind: "request", request: request("share", operation) })!.result;
    return { session, observe, act };
  }

  it("observe_maps_roles_names_values_states_and_physical_frames", () => {
    const state = shared().observe();
    expect(nodes(state.root).find(n => n.role === "text_field")).toMatchObject({ name: "Message", value: { text: "" }, states: expect.arrayContaining(["enabled", "editable", "settable"]), frame: { x: 10, y: 20, w: 100, h: 30 } });
    expect(state.frameSpace).toBe("tab_viewport_physical");
  });

  it("password_observation_never_reads_or_leaks_value", () => {
    const input = document.querySelector("input")!;
    input.type = "password";
    Object.defineProperty(input, "value", { get: vi.fn(() => { throw new Error("secret must not be read"); }) });
    const state = shared().observe();
    expect(nodes(state.root).find(n => n.states.includes("password"))?.value).toEqual({ text: "•••" });
    expect(JSON.stringify(state)).not.toContain("secret must not be read");
  });

  it("same_origin_frame_and_open_shadow_nodes_preserve_paths_and_geometry", () => {
    const host = document.createElement("section");
    host.attachShadow({ mode: "open" }).innerHTML = '<button aria-label="Shadow action"></button>';
    document.body.append(host);
    const frame = document.createElement("iframe");
    document.body.append(frame);
    frame.contentDocument!.body.innerHTML = '<input aria-label="Frame input" value="fixture">';
    vi.spyOn(frame.contentDocument!.querySelector("input")!, "getBoundingClientRect").mockReturnValue(document.querySelector("input")!.getBoundingClientRect());
    const h = shared();
    const state = h.observe();
    expect(nodes(state.root).find(n => n.name === "Shadow action")?.role).toBe("button");
    const input = nodes(state.root).find(n => n.name === "Frame input")!;
    expect(input.frame?.x).toBe(20);
    expect(h.act({ type: "set_value", snapshotId: state.id, nodeId: input.id, value: "changed" })).toMatchObject({ kind: "action", outcome: "done" });
    expect(frame.contentDocument!.querySelector("input")!.value).toBe("changed");
  });

  it.each(["click", "set_value", "scroll", "select"] as const)("%s_unknown_execution_surface_hands_off_without_side_effect", type => {
    document.body.insertAdjacentHTML("beforeend", '<div class="xterm"><textarea aria-label="Remote console"></textarea></div>');
    const h = shared();
    const state = h.observe();
    const click = vi.spyOn(HTMLElement.prototype, "click");
    expect(h.act({ type, snapshotId: state.id, nodeId: state.root.id, value: "ignored", dx: 0, dy: 10 } as DomRequest["operation"])).toMatchObject({ code: "capability_unavailable" });
    const operation = type === "set_value" || type === "select" ? { type, value: "ignored" } : type === "scroll" ? { type, dx: 0, dy: 10 } : { type };
    expect(h.act({ ...operation, snapshotId: state.id, nodeId: state.root.id })).toMatchObject({ code: "handoff_required" });
    expect(click).not.toHaveBeenCalled();
  });

  it("opaque_frames_and_canvas_report_incomplete_and_handoff", () => {
    document.body.insertAdjacentHTML("beforeend", "<iframe></iframe><canvas></canvas>");
    Object.defineProperty(document.querySelector("iframe"), "contentDocument", { get: () => null });
    const h = shared();
    const state = h.observe();
    expect(state.incomplete?.reason).toBe("partial_error");
    expect(h.act({ type: "click", snapshotId: state.id, nodeId: state.root.id })).toMatchObject({ code: "handoff_required" });
  });

  it("semantic_actions_deliver_and_consumed_snapshots_cannot_replay", () => {
    document.body.insertAdjacentHTML("beforeend", '<select aria-label="Choice"><option value="a">A</option><option value="b">B</option></select><div aria-label="Scroller" style="overflow:auto"></div>');
    const h = shared();
    let state = h.observe();
    const button = nodes(state.root).find(n => n.role === "button")!;
    const click = vi.spyOn(document.querySelector("button")!, "click");
    expect(h.act({ type: "click", snapshotId: state.id, nodeId: button.id })).toMatchObject({ outcome: "done" });
    expect(click).toHaveBeenCalledOnce();
    expect(h.act({ type: "click", snapshotId: state.id, nodeId: button.id })).toMatchObject({ code: "stale_element" });
    state = h.observe();
    const field = nodes(state.root).find(n => n.role === "text_field")!;
    const change = vi.fn();
    document.querySelector("input")!.addEventListener("change", change);
    expect(h.act({ type: "set_value", snapshotId: state.id, nodeId: field.id, value: "fixture value" })).toMatchObject({ outcome: "done" });
    expect(document.querySelector("input")!.value).toBe("fixture value");
    expect(change).toHaveBeenCalledOnce();
    state = h.observe();
    expect(h.act({ type: "select", snapshotId: state.id, nodeId: nodes(state.root).find(n => n.role === "combo_box")!.id, value: "b" })).toMatchObject({ outcome: "done" });
    expect(document.querySelector("select")!.value).toBe("b");
    const scroller = document.querySelector("div")!;
    scroller.scrollTo = vi.fn();
    state = h.observe();
    expect(h.act({ type: "scroll", snapshotId: state.id, nodeId: nodes(state.root).find(n => n.name === "Scroller")!.id, dx: 0, dy: 50 })).toMatchObject({ outcome: "done" });
    expect(scroller.scrollTo).toHaveBeenCalledWith({ left: 0, top: 50, behavior: "instant" });
  });

  it("changed_or_replaced_target_is_stale_without_name_based_fallback", () => {
    const h = shared();
    const state = h.observe();
    const button = nodes(state.root).find(n => n.role === "button")!;
    document.querySelector("button")!.outerHTML = '<button>Continue</button>';
    const click = vi.spyOn(HTMLElement.prototype, "click");
    expect(h.act({ type: "click", snapshotId: state.id, nodeId: button.id })).toMatchObject({ code: "stale_element" });
    expect(click).not.toHaveBeenCalled();
  });

  it("changed_then_restored_attributes_cannot_reuse_old_approval", () => {
    const h = shared();
    const state = h.observe();
    const button = nodes(state.root).find(n => n.role === "button")!;
    const el = document.querySelector("button")!;
    el.setAttribute("aria-label", "Changed");
    el.removeAttribute("aria-label");
    const click = vi.spyOn(el, "click");
    expect(h.act({ type: "click", snapshotId: state.id, nodeId: button.id })).toMatchObject({ code: "stale_element" });
    expect(click).not.toHaveBeenCalled();
  });

  it("password_default_text_is_not_copied_through_labels_or_names", () => {
    document.body.innerHTML = '<label>Secret<textarea id="password">secret-fixture</textarea></label><input type="hidden" value="hidden-fixture">';
    const state = shared().observe();
    expect(JSON.stringify(state)).not.toContain("secret-fixture");
    expect(JSON.stringify(state)).not.toContain("hidden-fixture");
  });

  it("input_handler_scope_change_stops_followup_events_and_reports_unknown", () => {
    const h = shared();
    const state = h.observe();
    const field = nodes(state.root).find(n => n.role === "text_field")!;
    const el = document.querySelector("input")!;
    const change = vi.fn();
    el.addEventListener("input", () => h.session.revoke(), { once: true });
    el.addEventListener("change", change);
    expect(h.act({ type: "set_value", snapshotId: state.id, nodeId: field.id, value: "changed" })).toMatchObject({ outcome: "unknown" });
    expect(change).not.toHaveBeenCalled();
  });

  it("child_document_pagehide_clears_all_shared_handles", () => {
    document.body.insertAdjacentHTML("beforeend", "<iframe></iframe>");
    const h = shared();
    h.observe();
    document.querySelector("iframe")!.contentWindow!.dispatchEvent(new Event("pagehide"));
    expect(h.session.handle({ kind: "request", request: request() })?.result).toMatchObject({ code: "app_denied" });
  });

  it("closed_custom_shadow_and_depth_cap_are_explicit_and_cannot_mutate", () => {
    const custom = document.createElement("fixture-opaque");
    custom.attachShadow({ mode: "closed" }).innerHTML = '<button>Uninspectable</button>';
    document.body.append(custom);
    const h = shared();
    let state = h.observe();
    expect(state.incomplete?.reason).toBe("partial_error");
    custom.remove();
    let parent: Element = document.body;
    for (let i = 0; i < 70; i++) { const next = document.createElement("div"); parent.append(next); parent = next; }
    state = h.observe();
    expect(state.incomplete?.reason).toBe("depth_cap");
    expect(h.act({ type: "click", snapshotId: state.id, nodeId: state.root.id })).toMatchObject({ code: "handoff_required" });
  });

  it("compatibility_unicode_execution_marker_requires_handoff", () => {
    document.body.insertAdjacentHTML("beforeend", '<div aria-label="Ｔｅｒｍｉｎａｌ"></div>');
    const h = shared();
    const state = h.observe();
    expect(state.handoff).toBe(true);
    expect(h.act({ type: "click", snapshotId: state.id, nodeId: state.root.id })).toMatchObject({ code: "handoff_required" });
  });
});

it("manifest_permissions_are_fixed_and_all_fifteen_locales_match_en", () => {
  const root = pathToFileURL(`${process.cwd()}/computer-use/tools/browser-extension/`);
  const manifest = JSON.parse(readFileSync(new URL("manifest.json", root), "utf8"));
  expect(manifest.permissions).toEqual(["activeTab", "scripting", "webNavigation", "nativeMessaging"]);
  expect(manifest.host_permissions).toBeUndefined();
  expect(manifest.content_scripts).toBeUndefined();
  expect(manifest.externally_connectable).toBeUndefined();
  const locales = readdirSync(new URL("_locales/", root));
  expect(locales.sort()).toEqual(["de", "en", "es", "fr", "id", "it", "ja", "ko", "pt_BR", "ru", "ta", "tl", "uk", "zh_CN", "zh_TW"].sort());
  const en = JSON.parse(readFileSync(new URL("_locales/en/messages.json", root), "utf8"));
  for (const locale of locales) {
    const catalog = JSON.parse(readFileSync(new URL(`_locales/${locale}/messages.json`, root), "utf8"));
    expect(Object.keys(catalog).sort()).toEqual(Object.keys(en).sort());
    for (const value of Object.values(catalog) as { message: string }[]) expect(value.message.trim()).not.toBe("");
  }
});
