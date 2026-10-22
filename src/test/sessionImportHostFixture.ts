import type { SetStateAction } from "react";
import { vi } from "vitest";
import { createT } from "@/i18n";
import type { SessionHostEventsCtx } from "@/hooks/useSessionHostEvents";
import type { SessionRow } from "@/lib/app/sidebarModels";
import { INITIAL_CONTEXT_USAGE } from "@/lib/contextUsage";
import { IDLE_SNAPSHOT, type ChatMessage, type SessionSnapshot } from "@/lib/session";
import type { SessionLiveMap } from "@/lib/sessionLiveStore";
import { createStopLatchState } from "@/lib/stopLatch";
import { SessionGateCache } from "@/lib/sessionGateCache";

function cell<T>(initial: T) {
  const ref = { current: initial };
  const set = vi.fn((next: SetStateAction<T>) => {
    ref.current = typeof next === "function" ? (next as (prev: T) => T)(ref.current) : next;
  });
  return { ref, set };
}

/** Actual mutable Host projections; functional setters must update the refs so
 * duplicate runtime events observe the preceding transition, just as in App. */
export function createSessionImportHostFixture() {
  const host = cell<SessionSnapshot>({ ...IDLE_SNAPSHOT });
  const viewed = cell<SessionSnapshot>({ ...IDLE_SNAPSHOT, sessionId: "native-viewed", state: "ready" });
  const live = cell<SessionLiveMap>({});
  const stop = cell(createStopLatchState());
  const rows = cell<SessionRow[]>([]);
  const usage = cell({ ...INITIAL_CONTEXT_USAGE });
  const messages = new Map<string, ChatMessage[]>([
    ["native-viewed", [{ id: "old", role: "assistant", content: "Existing native history" }]],
  ]);
  const tr = createT("en");
  const context: SessionHostEventsCtx = {
    patchSessionMessages: vi.fn((id, reduce) => {
      if (id) messages.set(id, reduce(messages.get(id) ?? []));
    }),
    tryApplyAutomationFromSession: vi.fn(async () => {}),
    setLiveHost: host.set, liveHostRef: host.ref,
    setLiveMap: live.set, liveMapRef: live.ref,
    setSession: viewed.set,
    setMessages: vi.fn((next) => {
      const id = context.viewingSessionIdRef.current;
      if (id) messages.set(id, typeof next === "function" ? next(messages.get(id) ?? []) : next);
    }),
    messagesBySessionRef: { current: messages },
    viewingSessionIdRef: { current: "native-viewed" },
    isSecondaryWindowRef: { current: false },
    secondaryFocusSessionIdRef: { current: null },
    openingSessionIdRef: { current: null },
    setStopLatch: stop.set, stopLatchRef: stop.ref,
    setLocalError: vi.fn(), setToast: vi.fn(),
    setSessions: rows.set, sessionsRef: rows.ref,
    projectsRef: { current: [] },
    setSessionChangesById: vi.fn(),
    setContextUsage: usage.set,
    setRetryStatus: vi.fn(), setStreamStall: vi.fn(),
    startTurnClock: vi.fn(), restartTurnClock: vi.fn(), clearTurnClock: vi.fn(),
    setRecentStallSignals: vi.fn(), setGoalOrchEvents: vi.fn(), setLastProcessLimit: vi.fn(),
    setAskUser: vi.fn(), setPerm: vi.fn(), setPlan: vi.fn(), setPlanFocusKey: vi.fn(),
    planBySessionRef: { current: new SessionGateCache() },
    markPlanPendingBadge: vi.fn(), planOpenedAsideRef: { current: false },
    planCompletedRecordedRef: { current: new Set() },
    openAsidePane: vi.fn(), openAsidePaneRef: { current: vi.fn() },
    setResourceOpenTarget: vi.fn(), navigateWorkbench: vi.fn(),
    pendingAskUserBySessionRef: { current: new SessionGateCache() },
    pendingPermBySessionRef: { current: new SessionGateCache() },
    pendingCompactBeforeRef: { current: null },
    clearPendingGatesRef: { current: vi.fn() },
    notifyPrefsRef: { current: { notifyOnTurnDone: true, notifyOnPermission: true } },
    localeRef: { current: "en" }, trRef: { current: tr }, tr,
    modeRef: { current: "chat" }, streamStallSeconds: 60,
  };
  return { context, viewed, live, usage, messages };
}
