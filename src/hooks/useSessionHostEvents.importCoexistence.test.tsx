/** @vitest-environment jsdom */
import "@/test/jsdomStubs";
import { act, cleanup, fireEvent, render, renderHook, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import * as api from "@/lib/api";
import * as migration from "@/lib/api/sessionMigration";
import * as association from "@/lib/api/sessionWorkspaceAssociation";
import * as notifications from "@/lib/desktopNotify";
import { loadCostUsageSamples } from "@/lib/costRollup";
import { clearSessionSpendStore, getSessionSpend } from "@/lib/sessionSpend";
import { listUnreadSessionIds, markUnread } from "@/lib/sessionUnread";
import { IDLE_SNAPSHOT } from "@/lib/session";
import { createSessionImportHostFixture } from "@/test/sessionImportHostFixture";
import { SessionImportHost } from "@/components/session-import/SessionImportHost";
import { openImportedHistory } from "@/lib/sessionImport/open";
import { createT } from "@/i18n";
import { nativeWebviewCoverDepth } from "@/lib/nativeWebviewCover";
import { useSessionHostEvents } from "./useSessionHostEvents";
import { useSessionCatalog } from "./useSessionCatalog";
import { useImportedCatalog } from "./useImportedCatalog";
import { useSessionImportJob } from "./useSessionImportJob";
import { useImportedWorkspaceGroups } from "./useImportedWorkspaceGroups";
import { createSessionChromeBadgesHost, useSessionChromeBadges } from "./useSessionChromeBadges";

const bus = vi.hoisted(() => {
  const listeners = new Map<string, Set<(payload: unknown) => void>>();
  return {
    listeners,
    subscribe(event: string, handler: (payload: unknown) => void) {
      const group = listeners.get(event) ?? new Set();
      group.add(handler);
      listeners.set(event, group);
      return () => { group.delete(handler); };
    },
    emit(event: string, payload: unknown) {
      listeners.get(event)?.forEach((handler) => handler(payload));
    },
  };
});
const readerIpc = vi.hoisted(() => ({
  events: vi.fn(),
  context: vi.fn(),
  search: vi.fn(),
  exportMarkdown: vi.fn(),
}));
// The reader captures its live client at module load, before per-test spies.
vi.mock("@/lib/api/sessionMigration", async (importOriginal) => ({
  ...await importOriginal<typeof import("@/lib/api/sessionMigration")>(),
  sessionImportEvents: readerIpc.events,
  sessionImportEventContext: readerIpc.context,
  sessionImportSearch: readerIpc.search,
  sessionImportExportMarkdown: readerIpc.exportMarkdown,
}));
vi.mock("@tauri-apps/api/event", () => ({
  // The import API uses the Tauri envelope; api.listen exposes its payload.
  listen: async (event: string, handler: (value: { payload: unknown }) => void) =>
    bus.subscribe(event, (payload) => handler({ payload })),
}));

function job(state = "running", stateVersion = 1): migration.JobRecord {
  return { jobId: "synthetic-import", requestId: "request", planHash: "plan", state, stateVersion,
    createdAt: "2026-09-20T00:00:00Z", items: [], itemCount: 10_000 };
}
function importBurst() {
  for (let version = 1; version <= 10_000; version++) {
    bus.emit("session-import://changed", { jobId: "synthetic-import", stateVersion: version, eventSeq: version });
  }
}
async function tick(ms = 300) {
  await act(async () => { await vi.advanceTimersByTimeAsync(ms); });
}

beforeEach(() => {
  vi.useFakeTimers();
  window.history.replaceState(null, "", "/");
  localStorage.clear(); sessionStorage.clear(); clearSessionSpendStore();
  bus.listeners.clear();
  for (const method of Object.values(readerIpc)) {
    method.mockReset().mockRejectedValue(new Error("Unexpected reader IPC"));
  }
  vi.spyOn(api, "isTauri").mockReturnValue(true);
  vi.spyOn(api, "hasHost").mockReturnValue(true);
  vi.spyOn(api, "listen").mockImplementation(async <T,>(event: string, handler: (payload: T) => void) =>
    bus.subscribe(event, (payload) => handler(payload as T)));
  vi.spyOn(api, "sessionGetState").mockResolvedValue({ ...IDLE_SNAPSHOT });
  vi.spyOn(api, "sessionMessages").mockResolvedValue([]);
  vi.spyOn(api, "sessionsList").mockResolvedValue([
    { id: "native-viewed", title: "Viewed", projectId: null, modelId: null, updatedAt: "2026-09-20T00:00:00Z" },
    { id: "native-background", title: "Background", projectId: null, modelId: null, updatedAt: "2026-09-20T00:00:00Z" },
  ]);
  vi.spyOn(api, "trayRefresh").mockResolvedValue(undefined);
  vi.spyOn(api, "traySetBusyCount").mockResolvedValue(undefined);
  vi.spyOn(api, "traySetWindowsOverlay").mockResolvedValue(undefined);
  // Fail visibly if a passive import event accidentally starts real model work.
  vi.spyOn(api, "sessionConnect").mockRejectedValue(new Error("Unexpected model connection"));
  vi.spyOn(api, "sessionSend").mockRejectedValue(new Error("Unexpected model send"));
  vi.spyOn(notifications, "showDesktopNotification").mockReturnValue(true);
  vi.spyOn(migration, "sessionImportEntries").mockResolvedValue({
    items: [{ importId: "imp:synthetic", installationId: "fixture", provider: "codex", nativeSessionId: "foreign",
      title: "Imported", hidden: false, createdAt: "2026-09-20T00:00:00Z" }],
    nextCursor: null, total: 1, providers: ["codex"],
  });
  vi.spyOn(migration, "sessionImportJobGet").mockResolvedValue(job());
});
afterEach(() => {
  cleanup(); clearSessionSpendStore(); bus.listeners.clear();
  vi.clearAllTimers(); vi.useRealTimers(); vi.restoreAllMocks();
  window.history.replaceState(null, "", "/");
});

test.each([false, true])("10k import-only hints preserve native chrome and accounting (existing unread: %s)", async (existingUnread) => {
  if (existingUnread) markUnread("native-background");
  const fixture = createSessionImportHostFixture();
  const badgeHost = createSessionChromeBadgesHost();
  badgeHost.viewingSessionId = () => fixture.context.viewingSessionIdRef.current;
  const hostRef = { current: badgeHost };
  const nativeRender = vi.fn();
  const native = renderHook(() => {
    nativeRender();
    const catalog = useSessionCatalog({ projects: [], isDialogOpen: () => false });
    fixture.context.setSessions = catalog.setSessions;
    fixture.context.sessionsRef = catalog.sessionsRef;
    useSessionHostEvents(fixture.context);
    const badges = useSessionChromeBadges({ hostRef, viewedSessionId: "native-viewed",
      isSecondaryWindow: false, trayBusyBadge: true, winTaskbarOverlay: true });
    return { catalog, badges };
  });
  const group = { sourceWorkspaceId: "workspace", displayName: "Workspace", sourceId: "fixture",
    provider: "codex", sourceLocator: "/synthetic", missing: false };
  vi.spyOn(association, "importedWorkspaceGroups").mockResolvedValue({ items: [group], selected: group, nextCursor: null });
  const onError = vi.fn();
  const imported = renderHook(() => {
    const catalog = useImportedCatalog("sidebar");
    const groups = useImportedWorkspaceGroups("workspace");
    const importing = useSessionImportJob(onError);
    return { catalog, groups, importing };
  });
  await act(async () => {
    await native.result.current.catalog.refreshSessions();
    imported.result.current.importing.setJob(job());
  });
  await tick(2000);
  act(() => bus.emit("session://usage", { sessionId: "native-background", source: "turn_completed",
    inputTokens: 120, outputTokens: 30, totalTokens: 150, modelCalls: 1, costUsdTicks: 2500 }));
  const samples = loadCostUsageSamples();
  const spend = { ...getSessionSpend("native-background") };
  expect(samples).toEqual([expect.objectContaining({ sessionId: "native-background", totalTokens: 150 })]);
  expect(spend.costUsdTicks).toBe(2500);
  expect(api.traySetBusyCount).toHaveBeenLastCalledWith(existingUnread ? 1 : 0);
  expect(api.traySetWindowsOverlay).toHaveBeenLastCalledWith(existingUnread ? 1 : 0);
  const nativeRows = native.result.current.catalog.sessions;
  const unread = native.result.current.badges.unreadSessionIds;
  const messages = [...fixture.messages.entries()];
  for (const spy of [api.sessionsList, api.trayRefresh, api.traySetBusyCount, api.traySetWindowsOverlay,
    migration.sessionImportEntries, migration.sessionImportJobGet, association.importedWorkspaceGroups,
    notifications.showDesktopNotification, nativeRender, fixture.context.setContextUsage]) vi.mocked(spy).mockClear();

  vi.mocked(migration.sessionImportEntries).mockResolvedValue({
    items: Array.from({ length: 50 }, (_, index) => ({ importId: `imp:archive-${index}`, installationId: "fixture",
      provider: "codex", nativeSessionId: `foreign-${index}`, title: "Archive", hidden: false, createdAt: "2026-09-20T00:00:00Z" })),
    nextCursor: "page-2", total: 10_000, providers: ["codex"],
  });
  vi.mocked(migration.sessionImportJobGet).mockResolvedValue(job("completed", 10_001));
  // Deliver hints separately so React batching cannot hide per-item updates.
  for (let index = 1; index <= 10_000; index++) act(() => {
    bus.emit("session-import://changed", { jobId: "synthetic-import", stateVersion: index, eventSeq: index });
    bus.emit("session-import://workspace-changed", null);
  });
  await tick(1500);
  expect(imported.result.current.catalog.total).toBe(10_000);
  expect(imported.result.current.catalog.entries).toHaveLength(50);
  expect(imported.result.current.groups.selected?.sourceWorkspaceId).toBe("workspace");
  expect(imported.result.current.importing.job?.state).toBe("completed");
  expect(imported.result.current.importing.jobActive).toBe(false);
  // The burst shares a queue with the 1 s poll. Until tick's act commits
  // completion, that poll can request one more coalesced read.
  const jobReads = vi.mocked(migration.sessionImportJobGet).mock.calls.length;
  expect(jobReads).toBeGreaterThanOrEqual(1);
  expect(jobReads).toBeLessThanOrEqual(2);
  // Once completion commits, neither polling nor trailing work may keep reading.
  await tick(3000);
  expect(migration.sessionImportEntries).toHaveBeenCalledTimes(1);
  expect(association.importedWorkspaceGroups).toHaveBeenCalledTimes(1);
  expect(migration.sessionImportJobGet).toHaveBeenCalledTimes(jobReads);
  expect(nativeRender).not.toHaveBeenCalled();
  expect(native.result.current.catalog.sessions).toBe(nativeRows);
  expect(native.result.current.badges.unreadSessionIds).toBe(unread);
  expect(listUnreadSessionIds()).toEqual(existingUnread ? ["native-background"] : []);
  expect([...fixture.messages.entries()]).toEqual(messages);
  expect(fixture.context.liveHostRef.current.state).toBe("idle");
  expect(Object.keys(fixture.live.ref.current)).toEqual([]);
  expect(loadCostUsageSamples()).toEqual(samples);
  expect(getSessionSpend("native-background")).toEqual(spend);
  expect(getSessionSpend("imp:archive-0").totalTokens).toBe(0);
  for (const spy of [api.sessionsList, api.trayRefresh, api.traySetBusyCount, api.traySetWindowsOverlay,
    notifications.showDesktopNotification, fixture.context.setContextUsage, api.sessionConnect, api.sessionSend, onError]) {
    expect(spy).not.toHaveBeenCalled();
  }

  // The real unread-to-native-chrome path remains connected after the burst.
  act(() => native.result.current.badges.handleMarkSessionUnread("native-viewed"));
  expect(api.traySetBusyCount).toHaveBeenLastCalledWith(existingUnread ? 2 : 1);
  expect(api.traySetWindowsOverlay).toHaveBeenLastCalledWith(existingUnread ? 2 : 1);
  imported.unmount();
  native.unmount();
  expect([...bus.listeners.values()].every((listeners) => listeners.size === 0)).toBe(true);
});

test.each(["zh", "en"] as const)("soft-respawn notices belong to the viewed chat (%s)", async (locale) => {
  const fixture = createSessionImportHostFixture();
  const tr = createT(locale);
  fixture.context.localeRef.current = locale;
  fixture.context.tr = tr;
  fixture.context.trRef.current = tr;
  const mounted = renderHook(() => useSessionHostEvents(fixture.context));
  await tick(2000);
  vi.mocked(fixture.context.setToast).mockClear();
  act(() => bus.emit("session://agent_soft_respawn", { sessionId: "native-background", reason: "effort" }));
  expect(fixture.context.setToast).not.toHaveBeenCalled();
  act(() => bus.emit("session://agent_soft_respawn", { sessionId: "native-viewed", reason: "effort" }));
  expect(fixture.context.setToast).toHaveBeenLastCalledWith(tr("agent.softRespawnToast"));
  fixture.context.viewingSessionIdRef.current = "native-background";
  vi.mocked(fixture.context.setToast).mockClear();
  act(() => bus.emit("session://agent_soft_respawn", { sessionId: "native-viewed", reason: "effort" }));
  expect(fixture.context.setToast).not.toHaveBeenCalled();
  // Older hosts omit the id; their existing global notice still works.
  act(() => bus.emit("session://agent_soft_respawn", { reason: "settings" }));
  expect(fixture.context.setToast).toHaveBeenLastCalledWith(tr("agent.softRespawnToast"));
  mounted.unmount();
});

test("Host-finalized stream settles the viewed turn without a ready state event", async () => {
  vi.mocked(api.sessionMessages).mockRejectedValue(
    new Error("synthetic journal has not flushed yet"),
  );
  const fixture = createSessionImportHostFixture();
  const mounted = renderHook(() => useSessionHostEvents(fixture.context));
  await tick(2000);
  fixture.messages.set("native-viewed", [
    { id: "question-finalized", role: "user", content: "Run a long task" },
    {
      id: "a-pending-question-finalized",
      role: "assistant",
      content: "",
      streaming: true,
    },
  ]);

  act(() => {
    bus.emit("session://state", {
      ...IDLE_SNAPSHOT,
      sessionId: "native-viewed",
      state: "streaming",
      streamingMessageId: "answer-finalized",
    });
    bus.emit("session://stream", {
      sessionId: "native-viewed",
      messageId: "answer-finalized",
      text: "Complete long answer",
      done: false,
    });
  });
  await tick();
  expect(fixture.context.liveHostRef.current.state).toBe("streaming");
  expect(fixture.viewed.ref.current.state).toBe("streaming");
  expect(fixture.live.ref.current["native-viewed"]?.state).toBe("streaming");
  expect(
    fixture.messages
      .get("native-viewed")
      ?.find((message) => message.id === "answer-finalized")?.streaming,
  ).toBe(true);

  // Intentionally omit session://state ready: this is the dropped-ready repro.
  act(() => {
    bus.emit("session://stream", {
      sessionId: "native-viewed",
      messageId: "answer-finalized",
      text: "",
      done: true,
      hostFinalized: true,
    });
  });
  await tick();

  expect(fixture.context.liveHostRef.current).toMatchObject({
    sessionId: "native-viewed",
    state: "ready",
    streamingMessageId: null,
  });
  expect(fixture.viewed.ref.current).toMatchObject({
    sessionId: "native-viewed",
    state: "ready",
    streamingMessageId: null,
  });
  expect(fixture.live.ref.current["native-viewed"]).toMatchObject({
    state: "ready",
    streamingMessageId: null,
  });
  expect(
    fixture.messages
      .get("native-viewed")
      ?.find((message) => message.id === "answer-finalized")?.streaming,
  ).toBe(false);
  expect(fixture.context.clearTurnClock).toHaveBeenCalledWith("native-viewed");
  expect(fixture.context.setRetryStatus).toHaveBeenLastCalledWith(null);
  expect(fixture.context.setStreamStall).toHaveBeenLastCalledWith(null);
  mounted.unmount();
});

test("Host-finalized identity fence rejects stale busy events but admits the next message", async () => {
  vi.mocked(api.sessionMessages).mockRejectedValue(
    new Error("synthetic journal has not flushed yet"),
  );
  const fixture = createSessionImportHostFixture();
  const mounted = renderHook(() => useSessionHostEvents(fixture.context));
  await tick(2000);
  fixture.messages.set("native-viewed", [
    { id: "question-fence", role: "user", content: "Run the fenced turn" },
    {
      id: "a-pending-question-fence",
      role: "assistant",
      content: "",
      streaming: true,
    },
  ]);

  act(() => {
    bus.emit("session://state", {
      ...IDLE_SNAPSHOT,
      sessionId: "native-viewed",
      state: "streaming",
      streamingMessageId: "answer-fenced",
    });
    bus.emit("session://stream", {
      sessionId: "native-viewed",
      messageId: "answer-fenced",
      text: "Final answer",
      done: false,
    });
    bus.emit("session://stream", {
      sessionId: "native-viewed",
      messageId: "answer-fenced",
      text: "",
      done: true,
      hostFinalized: true,
    });
  });
  await tick();
  expect(fixture.context.liveHostRef.current.state).toBe("ready");
  const settled = fixture.messages.get("native-viewed");
  vi.mocked(fixture.context.startTurnClock).mockClear();

  act(() => {
    bus.emit("session://state", {
      ...IDLE_SNAPSHOT,
      sessionId: "native-viewed",
      state: "streaming",
      streamingMessageId: "answer-fenced",
    });
    bus.emit("session://runtime", {
      ...IDLE_SNAPSHOT,
      sessionId: "native-viewed",
      state: "awaiting_permission",
      streamingMessageId: "answer-fenced",
    });
    bus.emit("session://stream", {
      sessionId: "native-viewed",
      messageId: "answer-fenced",
      text: " stale replay",
      done: false,
    });
  });
  await tick();

  expect(fixture.context.liveHostRef.current).toMatchObject({
    state: "ready",
    streamingMessageId: null,
  });
  expect(fixture.viewed.ref.current).toMatchObject({
    state: "ready",
    streamingMessageId: null,
  });
  expect(fixture.live.ref.current["native-viewed"]).toMatchObject({
    state: "ready",
    streamingMessageId: null,
  });
  expect(fixture.messages.get("native-viewed")).toBe(settled);
  expect(fixture.context.startTurnClock).not.toHaveBeenCalled();

  act(() => {
    bus.emit("session://state", {
      ...IDLE_SNAPSHOT,
      sessionId: "native-viewed",
      state: "streaming",
      streamingMessageId: "answer-next",
    });
  });
  await tick();

  expect(fixture.context.liveHostRef.current).toMatchObject({
    state: "streaming",
    streamingMessageId: "answer-next",
  });
  expect(fixture.viewed.ref.current).toMatchObject({
    state: "streaming",
    streamingMessageId: "answer-next",
  });
  expect(fixture.live.ref.current["native-viewed"]).toMatchObject({
    state: "streaming",
    streamingMessageId: "answer-next",
  });
  expect(fixture.context.startTurnClock).toHaveBeenCalledWith("native-viewed");
  mounted.unmount();
});

test("legacy done remains non-terminal while Host still reports streaming", async () => {
  vi.mocked(api.sessionMessages).mockRejectedValue(
    new Error("synthetic journal has not flushed yet"),
  );
  const fixture = createSessionImportHostFixture();
  const mounted = renderHook(() => useSessionHostEvents(fixture.context));
  await tick(2000);
  fixture.messages.set("native-viewed", [
    { id: "question-legacy", role: "user", content: "Keep streaming" },
    {
      id: "a-pending-question-legacy",
      role: "assistant",
      content: "",
      streaming: true,
    },
  ]);

  act(() => {
    bus.emit("session://state", {
      ...IDLE_SNAPSHOT,
      sessionId: "native-viewed",
      state: "streaming",
      streamingMessageId: "answer-legacy",
    });
    bus.emit("session://stream", {
      sessionId: "native-viewed",
      messageId: "answer-legacy",
      text: "Still receiving",
      done: false,
    });
  });
  await tick();
  vi.mocked(fixture.context.clearTurnClock).mockClear();

  act(() => {
    bus.emit("session://stream", {
      sessionId: "native-viewed",
      messageId: "answer-legacy",
      text: "",
      done: true,
    });
  });
  await tick();

  expect(fixture.context.liveHostRef.current.state).toBe("streaming");
  expect(fixture.viewed.ref.current.state).toBe("streaming");
  expect(fixture.live.ref.current["native-viewed"]?.state).toBe("streaming");
  expect(
    fixture.messages
      .get("native-viewed")
      ?.find((message) => message.id === "answer-legacy")?.streaming,
  ).toBe(true);
  expect(fixture.context.clearTurnClock).not.toHaveBeenCalled();
  mounted.unmount();
});

test("Host-finalized background stream settles without stealing focused state", async () => {
  vi.mocked(api.sessionMessages).mockRejectedValue(
    new Error("synthetic journal has not flushed yet"),
  );
  const fixture = createSessionImportHostFixture();
  const mounted = renderHook(() => useSessionHostEvents(fixture.context));
  await tick(2000);

  act(() => {
    bus.emit("session://state", {
      ...IDLE_SNAPSHOT,
      sessionId: "native-viewed",
      state: "streaming",
      streamingMessageId: "focused-answer",
    });
    bus.emit("session://runtime", {
      ...IDLE_SNAPSHOT,
      sessionId: "native-background",
      state: "streaming",
      streamingMessageId: "background-answer",
    });
    bus.emit("session://stream", {
      sessionId: "native-background",
      messageId: "background-answer",
      text: "Background answer",
      done: false,
    });
  });
  await tick();

  act(() => {
    bus.emit("session://stream", {
      sessionId: "native-background",
      messageId: "background-answer",
      text: "",
      done: true,
      hostFinalized: true,
    });
  });
  await tick();

  expect(fixture.live.ref.current["native-background"]?.state).toBe("ready");
  expect(fixture.context.liveHostRef.current).toMatchObject({
    sessionId: "native-viewed",
    state: "streaming",
  });
  expect(fixture.viewed.ref.current).toMatchObject({
    sessionId: "native-viewed",
    state: "streaming",
  });
  expect(fixture.context.viewingSessionIdRef.current).toBe("native-viewed");
  expect(fixture.context.clearTurnClock).toHaveBeenCalledWith(
    "native-background",
  );
  mounted.unmount();
});

test("real reader modal keeps its anchor, filter, input focus and scroll during import and native events", async () => {
  const importId = "imp:00000000-0000-4000-8000-000000000001";
  const tr = createT("en");
  const events = [
    { id: "e1", kind: "message", role: "user", blocks: [{ text: "Reader question" }] },
    { id: "e2", kind: "message", role: "assistant", blocks: [{ text: "Reader answer" }] },
  ];
  vi.spyOn(api, "sessionImportRecovery").mockResolvedValue({
    job: job(), plan: null, prepare: null, draft: null, expired: false, failed: null,
  });
  const catalog = { items: [{ importId, installationId: "fixture", provider: "codex", nativeSessionId: "foreign",
    title: "Reader history", hidden: false, currentRevision: "r1", createdAt: "2026-09-20T00:00:00Z" }],
    total: 1, nextCursor: null, providers: ["codex"] };
  vi.mocked(migration.sessionImportEntries).mockResolvedValue(catalog);
  vi.spyOn(api, "sessionImportEntries").mockResolvedValue(catalog);
  vi.mocked(migration.sessionImportEvents).mockResolvedValue({
    events: [events[0]], total: 2, start: 0, end: 1, previousCursor: null, nextCursor: "second", revision: "r1", leaf: "leaf1",
  });
  vi.mocked(migration.sessionImportEventContext).mockResolvedValue({
    events, targetIndex: 1, ordinal: 1, position: 1, total: 2, revision: "r1", leaf: "leaf1",
    anchorCursor: "anchor", previousCursor: null, nextCursor: null,
  });
  vi.spyOn(association, "importedWorkspaceGet").mockResolvedValue({
    importId, version: 0, sourceId: "source", provider: "codex", sourceReady: false, choice: null, status: "unassigned",
  });
  const scroll = vi.fn();
  const oldScroll = Object.getOwnPropertyDescriptor(Element.prototype, "scrollIntoView");
  Object.defineProperty(Element.prototype, "scrollIntoView", { configurable: true, value: scroll });
  const fixture = createSessionImportHostFixture();
  function Harness() {
    useSessionHostEvents(fixture.context);
    return <><button type="button">Outside focus</button><SessionImportHost locale="en" /></>;
  }
  const mounted = render(<Harness />);
  try {
    await tick(2000);
    const outside = screen.getByRole("button", { name: "Outside focus" });
    outside.focus();
    await act(async () => { openImportedHistory({ importId, revision: "r1", leaf: "leaf1", anchorEventId: "e2" }); });
    await tick();
    const dialog = screen.getByRole("dialog", { name: "Reader history" });
    expect(migration.sessionImportEvents).toHaveBeenCalledWith(importId, "r1", null, 100, "leaf1");
    expect(migration.sessionImportEventContext).toHaveBeenCalledWith(importId, "e2", "r1", "leaf1", { before: 8, after: 8 });
    const anchor = document.getElementById("import-ev-e2")!;
    expect(anchor).not.toBeNull();
    expect(anchor.textContent).toContain("Reader answer");
    expect(anchor.getAttribute("data-highlight")).toBe("1");
    expect(document.activeElement).toBe(anchor);
    fireEvent.click(within(dialog).getByRole("button", { name: tr("sessionImport.roleAssistant") }));
    await tick();
    const input = within(dialog).getByRole("searchbox");
    fireEvent.change(input, { target: { value: "尚未提交的搜索" } });
    input.focus();
    const body = dialog.querySelector<HTMLElement>(".modal-body")!;
    body.scrollTop = 344;
    const readerReads = vi.mocked(migration.sessionImportEvents).mock.calls.length;
    const contextReads = vi.mocked(migration.sessionImportEventContext).mock.calls.length;
    const scrollCalls = scroll.mock.calls.length;
    vi.mocked(migration.sessionImportJobGet).mockResolvedValue(job("completed", 10001));
    act(() => {
      bus.emit("session://runtime", { ...IDLE_SNAPSHOT, sessionId: "native-background", state: "streaming" });
      importBurst();
      bus.emit("session://stream", { sessionId: "native-background", messageId: "answer", text: "Background kept running", done: false });
      bus.emit("session://usage", { sessionId: "native-background", source: "turn_completed", inputTokens: 120,
        outputTokens: 30, totalTokens: 150, modelCalls: 1, costUsdTicks: 2500 });
      bus.emit("session://runtime", { ...IDLE_SNAPSHOT, sessionId: "native-background", state: "ready" });
    });
    await tick(1500);
    expect(screen.getByRole("dialog", { name: "Reader history" })).toBe(dialog);
    expect(document.activeElement).toBe(input);
    expect((input as HTMLInputElement).value).toBe("尚未提交的搜索");
    expect(body.scrollTop).toBe(344);
    expect(scroll).toHaveBeenCalledTimes(scrollCalls);
    expect(document.getElementById("import-ev-e2")).toBe(anchor);
    expect(document.getElementById("import-ev-e1")).toBeNull();
    expect(anchor.getAttribute("data-highlight")).toBe("1");
    expect(window.location.hash).toContain("/event/e2");
    expect(migration.sessionImportEvents).toHaveBeenCalledTimes(readerReads);
    expect(migration.sessionImportEventContext).toHaveBeenCalledTimes(contextReads);
    expect(fixture.context.viewingSessionIdRef.current).toBe("native-viewed");
    expect(fixture.messages.get("native-background")?.some((message) => message.content === "Background kept running")).toBe(true);
    expect(listUnreadSessionIds()).toEqual(["native-background"]);
    expect(getSessionSpend("native-background").totalTokens).toBe(150);
    expect(notifications.showDesktopNotification).toHaveBeenCalledOnce();
    expect(api.sessionConnect).not.toHaveBeenCalled(); expect(api.sessionSend).not.toHaveBeenCalled();
    expect(migration.sessionImportSearch).not.toHaveBeenCalled();
    expect(migration.sessionImportExportMarkdown).not.toHaveBeenCalled();
    expect(nativeWebviewCoverDepth()).toBe(1);
    fireEvent.keyDown(input, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(document.activeElement).toBe(outside);
    expect(nativeWebviewCoverDepth()).toBe(0);
  } finally {
    mounted.unmount();
    if (oldScroll) Object.defineProperty(Element.prototype, "scrollIntoView", oldScroll);
    else Reflect.deleteProperty(Element.prototype, "scrollIntoView");
  }
  expect([...bus.listeners.values()].every((listeners) => listeners.size === 0)).toBe(true);
});

test("import progress coexists with real Host runtime, stream, unread and usage handlers", async () => {
  const fixture = createSessionImportHostFixture();
  const onError = vi.fn();
  const { result, unmount } = renderHook(() => {
    const native = useSessionCatalog({ projects: [], isDialogOpen: () => false });
    fixture.context.setSessions = native.setSessions;
    fixture.context.sessionsRef = native.sessionsRef;
    useSessionHostEvents(fixture.context);
    const imported = useImportedCatalog("sidebar");
    const importing = useSessionImportJob(onError);
    return { native, imported, importing };
  });
  await act(async () => {
    await result.current.native.refreshSessions();
    result.current.importing.setJob(job());
  });
  await tick(2000); // Finish initial Host snapshot/journal hydration before measuring.
  await act(async () => { result.current.imported.setProvider("codex"); });
  vi.mocked(api.sessionsList).mockClear(); vi.mocked(api.trayRefresh).mockClear();
  vi.mocked(migration.sessionImportEntries).mockClear();
  const oldHistory = fixture.messages.get("native-viewed");

  act(() => {
    bus.emit("session://runtime", { ...IDLE_SNAPSHOT, sessionId: "native-background", state: "streaming" });
    importBurst();
    bus.emit("session://stream", { sessionId: "native-background", messageId: "answer", text: "Native output", done: false });
    bus.emit("session://usage", { sessionId: "native-background", source: "turn_completed", inputTokens: 120,
      outputTokens: 30, totalTokens: 150, modelCalls: 1, costUsdTicks: 2500 });
  });
  await tick();
  expect(fixture.live.ref.current["native-background"].state).toBe("streaming");
  expect(fixture.messages.get("native-background")?.some((message) => message.content === "Native output")).toBe(true);
  expect(fixture.messages.get("native-viewed")).toEqual(oldHistory);
  expect(listUnreadSessionIds()).toEqual([]);
  expect(notifications.showDesktopNotification).not.toHaveBeenCalled();
  expect(result.current.imported.provider).toBe("codex");
  expect(migration.sessionImportEntries).toHaveBeenCalledTimes(1);
  expect(result.current.native.sessions.map((row) => row.id)).toEqual(["native-viewed", "native-background"]);
  expect(api.sessionsList).not.toHaveBeenCalled();
  expect(api.trayRefresh).not.toHaveBeenCalled();

  vi.mocked(migration.sessionImportJobGet).mockResolvedValue(job("completed", 10_001));
  act(() => {
    bus.emit("session://runtime", { ...IDLE_SNAPSHOT, sessionId: "native-background", state: "ready" });
    importBurst();
    // A repeated Host ready must observe the updated live-map ref.
    bus.emit("session://runtime", { ...IDLE_SNAPSHOT, sessionId: "native-background", state: "ready" });
  });
  await tick(1500);
  expect(result.current.importing.job?.state).toBe("completed");
  expect(listUnreadSessionIds()).toEqual(["native-background"]);
  expect(notifications.showDesktopNotification).toHaveBeenCalledTimes(1);
  expect(notifications.showDesktopNotification).toHaveBeenCalledWith(expect.objectContaining({
    sessionId: "native-background", tag: "turn-done-native-background", force: true,
  }));
  expect(loadCostUsageSamples()).toEqual([expect.objectContaining({ sessionId: "native-background", totalTokens: 150 })]);
  expect(getSessionSpend("native-background")).toMatchObject({ totalTokens: 150, modelCalls: 1, costUsdTicks: 2500 });
  expect(getSessionSpend("imp:synthetic").totalTokens).toBe(0);
  expect(fixture.context.setContextUsage).not.toHaveBeenCalled();
  expect(Object.keys(fixture.live.ref.current)).toEqual(["native-background"]);
  expect(fixture.context.viewingSessionIdRef.current).toBe("native-viewed");
  expect(fixture.viewed.ref.current.sessionId).toBe("native-viewed");
  expect(fixture.context.setLocalError).not.toHaveBeenCalled();
  expect(onError).not.toHaveBeenCalled();
  expect(api.sessionConnect).not.toHaveBeenCalled(); expect(api.sessionSend).not.toHaveBeenCalled();
  expect(api.sessionsList).not.toHaveBeenCalled(); expect(api.trayRefresh).not.toHaveBeenCalled();

  // Positive control: a native catalog event still refreshes the tray/catalog.
  act(() => bus.emit("sessions://changed", { reason: "synthetic-native-update" }));
  await tick();
  expect(api.sessionsList).toHaveBeenCalledTimes(1); expect(api.trayRefresh).toHaveBeenCalledTimes(1);
  unmount();
  const reads = vi.mocked(migration.sessionImportEntries).mock.calls.length;
  act(importBurst);
  await tick(3000);
  expect(migration.sessionImportEntries).toHaveBeenCalledTimes(reads);
  expect([...bus.listeners.values()].every((listeners) => listeners.size === 0)).toBe(true);
});
