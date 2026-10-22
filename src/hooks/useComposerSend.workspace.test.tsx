/** @vitest-environment jsdom */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { createT } from "@/i18n";
import { IDLE_SNAPSHOT, type ChatMessage, type SessionSnapshot } from "@/lib/session";
import { WORKSPACE_REVIEW_EVENT } from "@/lib/workspaceRuntime";
import { useComposerSend, type ComposerSendHost } from "./useComposerSend";

const mocks = vi.hoisted(() => ({ send: vi.fn() }));
vi.mock("@/lib/api", () => ({ sessionSend: mocks.send, hasHost: () => false }));
afterEach(() => { cleanup(); vi.clearAllMocks(); });

function fixture() {
  let draft = "keep this message";
  let session: SessionSnapshot = { ...IDLE_SNAPSHOT, sessionId: "s1", state: "ready", title: "Existing chat" };
  const ref = <T,>(value: T) => ({ current: value });
  const live = ref(session);
  const rows = new Map<string, ChatMessage[]>([["s1", []]]);
  const host = {
    tr: createT("en"), session, sessions: [], activeProject: null,
    connecting: false, goalMode: false, attachments: [], chatAttachments: [], quotes: [], skillInfos: [],
    isSecondaryWindowRef: ref(false), viewingSessionIdRef: ref("s1"), composerInputRef: ref(null), liveHostRef: live,
    messagesBySessionRef: ref(rows), sessionJsonSchemaRef: ref(null), automationSetupDraftRef: ref(false),
    automationSetupSessionsRef: ref(new Set()), sendInFlightRef: ref(false), sendInFlightBySessionRef: ref(new Set()),
    sendEpochRef: ref(0), sendEpochBySessionRef: ref(new Map()), turnStartedAtBySessionRef: ref(new Map()),
    effortApplyRef: ref(Promise.resolve()), promptHistoryIndexRef: ref(null), quotesRef: ref([]), attachmentsRef: ref([]),
    sendQueueRef: ref({ releaseFlushHold: vi.fn() }), showToastRef: ref(vi.fn()), sendRef: ref(null),
    executeSendFromQueueRef: ref(null), executeSendLatestRef: ref(null), claimSendForSession: () => true,
    currentViewFocus: () => ({ sessionId: "s1", projectId: null }),
    patchSessionMessages: (id: string, reduce: (prev: ChatMessage[]) => ChatMessage[]) => rows.set(id, reduce(rows.get(id) ?? [])),
    ensureConnected: vi.fn().mockResolvedValue("s1"), getDraft: () => draft,
    setDraft: (value: string) => { draft = value; },
    setSession: (update: (prev: SessionSnapshot) => SessionSnapshot) => { session = update(session); },
    setLiveHost: (update: (prev: SessionSnapshot) => SessionSnapshot) => { live.current = update(live.current); },
    isPlaceholderTitle: () => false, setLocalError: vi.fn(),
  };
  const fallback = new Map<PropertyKey, unknown>();
  const complete = new Proxy(host, { get: (target, key) => {
    if (key in target) return Reflect.get(target, key);
    if (!fallback.has(key)) fallback.set(key, vi.fn());
    return fallback.get(key);
  } }) as unknown as ComposerSendHost;
  return { host: complete, rows, getDraft: () => draft };
}

test.each(["WORKSPACE_CHANGED: moved", "SEND_CANCELLED: stopped"])("actual send restores input and removes unsent optimistic rows: %s", async (error) => {
  mocks.send.mockRejectedValue(error);
  const state = fixture();
  const recovery = vi.fn();
  window.addEventListener(WORKSPACE_REVIEW_EVENT, recovery);
  try {
    const { result } = renderHook(() => useComposerSend(state.host));
    await act(async () => result.current.send());
    expect(state.getDraft()).toBe("keep this message");
    expect(state.rows.get("s1")).toEqual([]);
    expect(mocks.send).toHaveBeenCalledTimes(1);
    expect(state.host.ensureConnected).toHaveBeenCalledTimes(1);
    expect(recovery).toHaveBeenCalledTimes(error.startsWith("WORKSPACE_CHANGED") ? 1 : 0);
    expect(state.host.sendInFlightBySessionRef.current.size).toBe(0);
  } finally { window.removeEventListener(WORKSPACE_REVIEW_EVENT, recovery); }
});

test("failed connection leaves no user row and preserves the composer", async () => {
  const state = fixture();
  vi.mocked(state.host.ensureConnected).mockResolvedValue(null);
  const { result } = renderHook(() => useComposerSend(state.host));
  await act(async () => result.current.send());
  expect(state.rows.get("s1")).toEqual([]);
  expect(state.getDraft()).toBe("keep this message");
  expect(mocks.send).not.toHaveBeenCalled();
});

test("confirmed continuation rollback removes the optimistic turn and localizes the cause", async () => {
  mocks.send.mockRejectedValue(
    new Error(
      "TURN_NOT_DISPATCHED_ROLLED_BACK: HISTORY_ACCESS_UNAVAILABLE: HISTORY_TOOLS_UNVERIFIED",
    ),
  );
  const state = fixture();
  const { result } = renderHook(() => useComposerSend(state.host));
  await act(async () => result.current.send());
  expect(state.rows.get("s1")).toEqual([]);
  expect(state.getDraft()).toBe("keep this message");
  expect(state.host.setLocalError).toHaveBeenCalledWith(
    "History tool is not available",
  );
});

test("rollback-like but non-exact disposition keeps the user bubble", async () => {
  mocks.send.mockRejectedValue(
    new Error(
      "TURN_NOT_DISPATCHED_ROLLED_BACK_UNKNOWN: HISTORY_ACCESS_UNAVAILABLE: HISTORY_TOOLS_UNVERIFIED",
    ),
  );
  const state = fixture();
  const { result } = renderHook(() => useComposerSend(state.host));
  await act(async () => result.current.send());
  expect(state.rows.get("s1")?.map((message) => message.role)).toEqual([
    "user",
  ]);
  expect(state.getDraft()).toBe("keep this message");
});

test("unverified native rollback keeps the user bubble and shows safe dispatch copy", async () => {
  mocks.send.mockRejectedValue(
    new Error("DISPATCH_UNKNOWN: local rollback could not be verified"),
  );
  const state = fixture();
  const { result } = renderHook(() => useComposerSend(state.host));
  await act(async () => result.current.send());
  expect(state.rows.get("s1")?.map((message) => message.role)).toEqual([
    "user",
  ]);
  expect(state.getDraft()).toBe("keep this message");
  expect(state.host.setLocalError).toHaveBeenCalledWith(
    "The last send may already have run — not sending again",
  );
});
