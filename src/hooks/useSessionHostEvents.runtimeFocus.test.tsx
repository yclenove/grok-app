/** @vitest-environment jsdom */
import "@/test/jsdomStubs";
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import * as api from "@/lib/api";
import * as notifications from "@/lib/desktopNotify";
import * as mirror from "@/lib/mirrorTransport";
import { createT } from "@/i18n";
import { IDLE_SNAPSHOT, type SessionSnapshot } from "@/lib/session";
import { sessionLiveMapStore } from "@/lib/sessionLiveMapStore";
import { projectHostIntoLiveMap } from "@/lib/sessionLiveStore";
import { sessionShellStore } from "@/lib/sessionShellStore";
import { sessionTranscriptStore } from "@/lib/sessionTranscriptStore";
import { createSessionImportHostFixture } from "@/test/sessionImportHostFixture";
import { useSessionHostEvents } from "./useSessionHostEvents";
import { useSessionRuntime } from "./useSessionRuntime";
import { useSendQueue } from "./useSendQueue";

const listeners = new Map<string, Set<(payload: unknown) => void>>();
const awaiting: SessionSnapshot = {
  ...IDLE_SNAPSHOT,
  sessionId: "native-viewed",
  state: "awaiting_permission",
  streamingMessageId: "中文回答",
  title: "权限回复",
};

async function tick(ms = 100) {
  await act(async () => { await vi.advanceTimersByTimeAsync(ms); });
}

function emit(event: string, snapshot: SessionSnapshot) {
  act(() => listeners.get(event)?.forEach((handler) => handler(snapshot)));
}

function mount(initial = awaiting, viewed = initial, locale: "zh" | "en" = "zh") {
  sessionShellStore.setLiveHost(initial);
  sessionShellStore.setSession(viewed);
  sessionLiveMapStore.setMap(projectHostIntoLiveMap({}, initial));
  const fixture = createSessionImportHostFixture();
  const tr = createT(locale);
  const execute = vi.fn(async () => true);
  const executeSendRef = { current: execute };
  const sendInFlightRef = { current: false };
  const showToast = vi.fn();
  const labels = { sendFailed: "失败", droppedOldest: () => "移除旧消息" };
  const hook = renderHook(() => {
    const runtime = useSessionRuntime();
    useSessionHostEvents({
      ...fixture.context,
      ...runtime,
      localeRef: { current: locale },
      trRef: { current: tr },
      tr,
    });
    const queue = useSendQueue({
      sessionId: runtime.session.sessionId,
      sessionState: runtime.session.state,
      connecting: false,
      liveHostRef: runtime.liveHostRef,
      viewingSessionIdRef: runtime.viewingSessionIdRef,
      sendInFlightRef,
      executeSendRef,
      showToast,
      acceptExternal: true,
      labels,
    });
    return { runtime, queue };
  });
  vi.mocked(api.sessionGetState).mockResolvedValue(initial);
  return { ...hook, execute, fixture };
}

beforeEach(() => {
  vi.useFakeTimers();
  localStorage.clear(); sessionStorage.clear(); listeners.clear();
  sessionShellStore.resetForTests();
  sessionLiveMapStore.resetForTests();
  sessionTranscriptStore.resetForTests();
  vi.spyOn(api, "isTauri").mockReturnValue(true);
  vi.spyOn(api, "isDesktopHost").mockReturnValue(true);
  vi.spyOn(api, "hasHost").mockReturnValue(true);
  vi.spyOn(mirror, "isMirrorClient").mockReturnValue(false);
  vi.spyOn(api, "listen").mockImplementation(async <T,>(event: string, handler: (payload: T) => void) => {
    const group = listeners.get(event) ?? new Set();
    const callback = (payload: unknown) => handler(payload as T);
    group.add(callback); listeners.set(event, group);
    return () => { group.delete(callback); };
  });
  vi.spyOn(api, "sessionGetState").mockResolvedValue(awaiting);
  vi.spyOn(api, "sessionMessages").mockResolvedValue([]);
  vi.spyOn(api, "sessionsList").mockResolvedValue([]);
  vi.spyOn(api, "trayRefresh").mockResolvedValue(undefined);
  vi.spyOn(api, "sessionQueueSnapshot").mockResolvedValue({ version: 1, items: [] });
  vi.spyOn(api, "sessionConnect").mockRejectedValue(new Error("Unexpected model connection"));
  vi.spyOn(api, "sessionSend").mockRejectedValue(new Error("Unexpected model send"));
  vi.spyOn(notifications, "showDesktopNotification").mockReturnValue(true);
});

afterEach(() => {
  cleanup(); listeners.clear(); vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks();
  sessionShellStore.resetForTests(); sessionLiveMapStore.resetForTests(); sessionTranscriptStore.resetForTests();
});

test.each(["zh", "en"] as const)("permission continuation keeps the live projection and refs current (%s)", async (locale) => {
  const view = mount(awaiting, awaiting, locale);
  await tick(2000);
  emit("session://runtime", { ...awaiting, state: "streaming" });
  expect(view.result.current.runtime.session.state).toBe("streaming");
  expect(view.result.current.runtime.liveHost.state).toBe("streaming");
  expect(view.result.current.runtime.liveHostRef.current.state).toBe("streaming");
  expect(view.result.current.runtime.busyIds.has(awaiting.sessionId!)).toBe(true);
  expect(view.result.current.runtime.effectiveCanStop).toBe(true);
  expect(notifications.showDesktopNotification).not.toHaveBeenCalled();
});

test("deferred permission completion clears the real shell busy membership", async () => {
  const view = mount();
  await tick(2000);
  expect(view.result.current.runtime.busyIds.has(awaiting.sessionId!)).toBe(true);
  emit("session://runtime", { ...awaiting, state: "ready", streamingMessageId: null });
  expect(sessionLiveMapStore.getSnapshot(awaiting.sessionId)?.state).toBe("ready");
  expect(view.result.current.runtime.session.state).toBe("ready");
  expect(view.result.current.runtime.busyIds.has(awaiting.sessionId!)).toBe(false);
  expect(view.result.current.runtime.effectiveCanSend).toBe(true);
});

test("a runtime-only permission finish releases an actual queued follow-up once", async () => {
  const view = mount();
  await tick(2000);
  act(() => view.result.current.queue.enqueue({ storedDisplay: "接着处理中文任务", attachments: [], goalMode: false }));
  await tick();
  expect(view.execute).not.toHaveBeenCalled();
  const ready = { ...awaiting, state: "ready" as const, streamingMessageId: null };
  emit("session://runtime", ready);
  await tick();
  expect(view.execute).toHaveBeenCalledTimes(1);
  expect(view.execute).toHaveBeenCalledWith(expect.objectContaining({
    storedDisplay: "接着处理中文任务", targetSessionId: awaiting.sessionId, fromQueue: true,
  }));
  emit("session://runtime", ready);
  await tick();
  expect(view.execute).toHaveBeenCalledTimes(1);
  expect(view.result.current.queue.activeQueue).toEqual([]);
});

test("a viewed background finish releases only its queue and preserves a busy foreign live host", async () => {
  const live = { ...awaiting, sessionId: "native-live", state: "streaming" as const };
  const view = mount(live, awaiting);
  await tick(2000);
  emit("session://runtime", awaiting);
  act(() => view.result.current.queue.enqueue({ storedDisplay: "背景会话继续", attachments: [], goalMode: false }));
  emit("session://runtime", { ...awaiting, state: "ready", streamingMessageId: null });
  await tick();
  expect(view.result.current.runtime.liveHost).toEqual(live);
  expect(view.result.current.runtime.liveHostRef.current).toEqual(live);
  expect(view.result.current.runtime.viewingSessionIdRef.current).toBe(awaiting.sessionId);
  expect(view.result.current.runtime.busyIds).toEqual(new Set([live.sessionId]));
  expect(view.execute).toHaveBeenCalledTimes(1);
  expect(view.execute).toHaveBeenCalledWith(expect.objectContaining({ targetSessionId: awaiting.sessionId }));
});

test("a live completion while imported history is viewed updates host status without changing the reader focus", async () => {
  const imported = { ...IDLE_SNAPSHOT, sessionId: "imp:synthetic", state: "ready" as const, title: "导入记录" };
  const view = mount(awaiting, imported);
  await tick(2000);
  emit("session://runtime", { ...awaiting, state: "ready", streamingMessageId: null });
  expect(view.result.current.runtime.liveHost.state).toBe("ready");
  expect(view.result.current.runtime.busyIds.size).toBe(0);
  expect(view.result.current.runtime.session).toEqual(imported);
  expect(view.result.current.runtime.viewingSessionIdRef.current).toBe(imported.sessionId);
  expect(view.execute).not.toHaveBeenCalled();
});

test("late old-focus runtime events never replace the new live focus", async () => {
  const view = mount();
  await tick(2000);
  const replacement = { ...awaiting, sessionId: "native-new-focus", state: "streaming" as const, title: "新焦点" };
  act(() => { view.result.current.runtime.viewingSessionIdRef.current = replacement.sessionId; });
  emit("session://state", replacement);
  emit("session://runtime", { ...awaiting, state: "ready", streamingMessageId: null });
  expect(view.result.current.runtime.liveHost).toEqual(replacement);
  expect(view.result.current.runtime.liveHostRef.current).toEqual(replacement);
  expect(view.result.current.runtime.session.sessionId).toBe(replacement.sessionId);
  expect(sessionLiveMapStore.getSnapshot(awaiting.sessionId)?.state).toBe("ready");
  expect(view.result.current.runtime.busyIds).toEqual(new Set([replacement.sessionId]));
});

test("a stopped live chat stays ready when a late runtime streaming snapshot arrives", async () => {
  const view = mount();
  await tick(2000);
  act(() => view.result.current.runtime.setStopLatch({ phase: "force_idle", sessionId: awaiting.sessionId, startedAt: Date.now() }));
  emit("session://runtime", { ...awaiting, state: "streaming" });
  expect(view.result.current.runtime.liveHost.state).toBe("ready");
  expect(view.result.current.runtime.liveHost.streamingMessageId).toBeNull();
  expect(view.result.current.runtime.liveHostRef.current.state).toBe("ready");
  expect(view.result.current.runtime.busyIds.size).toBe(0);
  expect(view.result.current.runtime.effectiveCanSend).toBe(true);
});

test("mirror runtime delivery settles the same live owner without a model call", async () => {
  vi.mocked(api.isTauri).mockReturnValue(false);
  vi.mocked(api.isDesktopHost).mockReturnValue(false);
  vi.mocked(mirror.isMirrorClient).mockReturnValue(true);
  const view = mount();
  await tick(2000);
  emit("session://runtime", { ...awaiting, state: "ready", streamingMessageId: null });
  expect(view.result.current.runtime.liveHost.state).toBe("ready");
  expect(view.result.current.runtime.busyIds.size).toBe(0);
  expect(api.sessionConnect).not.toHaveBeenCalled();
  expect(api.sessionSend).not.toHaveBeenCalled();
});

test("unbound runtime snapshots and late delivery after unmount have no projection effects", async () => {
  const view = mount();
  await tick(2000);
  const before = sessionShellStore.getLiveHost();
  emit("session://runtime", { ...IDLE_SNAPSHOT });
  expect(sessionShellStore.getLiveHost()).toBe(before);
  view.unmount();
  expect(listeners.get("session://runtime")?.size).toBe(0);
  emit("session://runtime", { ...awaiting, state: "ready", streamingMessageId: null });
  expect(sessionShellStore.getLiveHost()).toBe(before);
  expect(view.execute).not.toHaveBeenCalled();
});
