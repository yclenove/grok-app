/** @vitest-environment jsdom */
import "@/test/jsdomStubs";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { createT } from "@/i18n";
import * as notifications from "@/lib/desktopNotify";
import type { PermissionPayload } from "@/lib/session";
import { createSessionImportHostFixture } from "@/test/sessionImportHostFixture";
import { registerGateAndMetaSubscriptions } from "./sessionHostGateSubscriptions";

const permission: PermissionPayload = {
  sessionId: "native-viewed", rpcId: 92, toolCallId: "中文工具", toolName: "shell",
  title: "python 示例.py", preview: "python 示例.py", scopeKey: "shell:python",
  options: [{ optionId: "reject-once", kind: "reject_once" }],
};

function mount(locale: "zh" | "en" = "zh") {
  const fixture = createSessionImportHostFixture();
  fixture.context.localeRef.current = locale;
  fixture.context.trRef.current = createT(locale);
  fixture.context.notifyPrefsRef.current = { notifyOnPermission: false, notifyOnTurnDone: false };
  const listeners = new Map<string, (payload: unknown) => void>();
  let cancelled = false;
  registerGateAndMetaSubscriptions(fixture.context, {
    isCancelled: () => cancelled,
    track: () => {},
    listenWithRetry: async <T,>(event: string, handler: (payload: T) => void) => {
      listeners.set(event, (payload) => handler(payload as T));
      return () => { listeners.delete(event); };
    },
  });
  return {
    ...fixture,
    emit: (payload: PermissionPayload) => listeners.get("session://permission")!(payload),
    cancel: () => { cancelled = true; },
  };
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.spyOn(notifications, "showDesktopNotification").mockReturnValue(true);
});
afterEach(() => { vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks(); });

test.each(["zh", "en"] as const)("failed automatic reply restores its card and localized error (%s)", (locale) => {
  const view = mount(locale);
  const payload = { ...permission, responseError: "synthetic writer closed" };
  view.emit(payload);
  expect(view.context.pendingPermBySessionRef.current.get(permission.sessionId)).toBe(payload);
  expect(view.context.setPerm).toHaveBeenCalledWith(payload);
  expect(view.context.setToast).toHaveBeenCalledWith(createT(locale)("perm.replyFailed", { error: payload.responseError }));
  expect(view.context.clearPendingGatesRef.current).not.toHaveBeenCalled();
  expect(view.context.setSession).not.toHaveBeenCalled();
  expect(notifications.showDesktopNotification).not.toHaveBeenCalled();
});

test.each(["zh", "en"] as const)("background failure is parked without replacing another chat's bar (%s)", (locale) => {
  const view = mount(locale);
  const payload = { ...permission, sessionId: "native-background", responseError: "synthetic writer closed" };
  view.emit(payload);
  expect(view.context.pendingPermBySessionRef.current.get(payload.sessionId)).toBe(payload);
  expect(view.context.setPerm).not.toHaveBeenCalled();
  expect(view.context.setToast).toHaveBeenLastCalledWith(createT(locale)("perm.replyFailed", { error: payload.responseError }));
  expect(view.context.viewingSessionIdRef.current).toBe(permission.sessionId);
  expect(view.context.clearPendingGatesRef.current).not.toHaveBeenCalled();
});

test("ordinary permission cards keep the existing notification behavior", () => {
  const view = mount();
  view.emit(permission);
  expect(view.context.setPerm).toHaveBeenCalledWith(permission);
  expect(view.context.setToast).not.toHaveBeenCalled();
  view.emit({ ...permission, sessionId: "native-background", responseError: " " });
  expect(view.context.setToast).toHaveBeenLastCalledWith(createT("zh")("session.backgroundPermission"));
});

test("a cancelled event subscription never restores a failed gate", () => {
  const view = mount();
  view.cancel();
  view.emit({ ...permission, responseError: "synthetic writer closed" });
  expect(view.context.pendingPermBySessionRef.current.size).toBe(0);
  expect(view.context.setPerm).not.toHaveBeenCalled();
  expect(view.context.setToast).not.toHaveBeenCalled();
});
