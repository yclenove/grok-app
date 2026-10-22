/** @vitest-environment jsdom */
import "@/test/jsdomStubs";
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import * as api from "@/lib/api";
import { getDraft, setDraft } from "@/lib/composerDraftStore";
import { loadComposerSessionDraft, saveComposerSessionDraft } from "@/lib/composerSessionDraft";
import { loadComposerProjectDraft, projectDraftKey, saveComposerProjectDraft } from "@/lib/composerProjectDraft";
import { IDLE_SNAPSHOT } from "@/lib/session";
import { INITIAL_CONTEXT_USAGE } from "@/lib/contextUsage";
import { sessionShellStore } from "@/lib/sessionShellStore";
import { sessionTranscriptStore } from "@/lib/sessionTranscriptStore";
import { sessionLiveMapStore } from "@/lib/sessionLiveMapStore";
import { hydrateSessionJournal } from "@/lib/sessionJournalHydrate";
import { useFocusedSession } from "./useSessionShell";
import { createSessionNavHost, useSessionNavigation } from "./useSessionNavigation";
import type { Project, SessionRow } from "@/lib/app/sidebarModels";

vi.mock("@/lib/sessionJournalHydrate", () => ({ hydrateSessionJournal: vi.fn() }));
const old: SessionRow = { id: "old-chat", title: "Old", projectId: null, updatedAt: "" };
const id = "97e20f19-42f4-47c1-b5e2-8e36dc7699af";
const saved = { id, title: "中文续聊", projectId: "project-1", updatedAt: "2026-10-03T00:00:00Z", modelId: "grok-4.6" };
const project: Project = { id: "project-1", name: "中文 多根", path: "C:/synthetic/cwd", trusted: false, pathOk: true };
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (cause: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

function setup(onDraft = false) {
  const host = createSessionNavHost();
  for (const group of Object.values(host)) {
    for (const name of Object.keys(group)) Object.assign(group, { [name]: vi.fn() });
  }
  let activeProject: Project | null = null;
  host.catalog.resolveProject = (_row, hint) => hint ?? null;
  host.catalog.setActiveProject = vi.fn((value) => { activeProject = value; });
  host.catalog.getActiveProject = () => activeProject;
  host.catalog.listLiveIds = () => [];
  host.catalog.findRow = () => null;
  host.composer.stashLeaving = (sid) => {
    if (sid) saveComposerSessionDraft(sid, { text: getDraft(), goalMode: true });
    else saveComposerProjectDraft(projectDraftKey(activeProject?.id), { text: getDraft() });
  };
  host.composer.restoreForSession = (sid) => setDraft(loadComposerSessionDraft(sid)?.text ?? "");
  host.composer.restoreForNewChat = (proj) => setDraft(loadComposerProjectDraft(projectDraftKey(proj?.id))?.text ?? "");
  host.draft.newChatTitle = () => "New";
  host.connect.isSecondaryWindow = () => false;
  host.connect.isProjectWarmable = (proj) => !!proj?.trusted;
  const viewing = { current: onDraft ? null : old.id };
  sessionShellStore.setSession({ ...IDLE_SNAPSHOT, sessionId: viewing.current });
  setDraft("iau");
  const hostRef = { current: host };
  const hook = renderHook(() => {
    const focused = useFocusedSession();
    return useSessionNavigation({ hostRef, viewingSessionIdRef: viewing,
      focusedSessionId: focused.sessionId, bumpViewEpoch: vi.fn() });
  });
  return { ...hook, host, viewing };
}

beforeEach(() => {
  vi.useFakeTimers();
  localStorage.clear(); sessionStorage.clear();
  sessionShellStore.resetForTests(); sessionTranscriptStore.resetForTests(); sessionLiveMapStore.resetForTests();
  vi.spyOn(api, "isTauri").mockReturnValue(true);
  vi.spyOn(api, "sessionsList").mockResolvedValue([{ ...saved, modelId: saved.modelId }]);
  vi.spyOn(api, "projectsList").mockResolvedValue([project]);
  vi.spyOn(api, "sessionConnect").mockResolvedValue({ ...IDLE_SNAPSHOT });
  vi.mocked(hydrateSessionJournal).mockResolvedValue({ status: "applied", painted: [],
    changesFromHistory: [], scheduledFromJournal: false, usage: { ...INITIAL_CONTEXT_USAGE } });
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.useRealTimers(); });

test("opens the fresh durable project and retains the prior session draft without warming an untrusted project", async () => {
  const { result, host, viewing } = setup();
  await act(async () => { await result.current.openSession(id); });
  expect(viewing.current).toBe(id);
  expect(host.catalog.setActiveProject).toHaveBeenLastCalledWith(expect.objectContaining(project));
  expect(sessionShellStore.getSessionSnapshot()).toMatchObject({ sessionId: id, title: saved.title });
  expect(loadComposerSessionDraft(old.id)).toMatchObject({ text: "iau", goalMode: true });
  expect(getDraft()).toBe("");
  await act(async () => { await vi.advanceTimersByTimeAsync(500); });
  expect(api.sessionConnect).not.toHaveBeenCalled();
  await act(async () => { await result.current.openSession(old); });
  expect(getDraft()).toBe("iau");
});

test("preserves the separate new-chat buffer, which is restored only by New chat", async () => {
  const { result } = setup(true);
  await act(async () => { await result.current.openSession(id); });
  expect(loadComposerProjectDraft(projectDraftKey(null))?.text).toBe("iau");
  await act(async () => { await result.current.openSession(old); });
  expect(getDraft()).toBe("");
  await act(async () => { await result.current.newChat(null); });
  expect(getDraft()).toBe("iau");
});

test("a late lookup cannot take over a newer sidebar navigation", async () => {
  const lookup = deferred<Awaited<ReturnType<typeof api.sessionsList>>>();
  vi.mocked(api.sessionsList).mockReturnValueOnce(lookup.promise);
  const { result, viewing } = setup();
  let pending!: Promise<void>;
  act(() => { pending = result.current.openSession(id); });
  const newer = { ...old, id: "newer-sidebar" };
  await act(async () => { await result.current.openSession(newer); });
  await act(async () => { lookup.resolve([{ ...saved, modelId: saved.modelId }]); await pending; });
  expect(viewing.current).toBe(newer.id);
  expect(sessionShellStore.getSessionSnapshot().sessionId).toBe(newer.id);
});

test("late lookup errors are ignored after a user opens a new draft", async () => {
  const lookup = deferred<Awaited<ReturnType<typeof api.sessionsList>>>();
  vi.mocked(api.sessionsList).mockReturnValueOnce(lookup.promise);
  const { result, viewing } = setup();
  let pending!: Promise<void>;
  act(() => { pending = result.current.openSession(id); });
  await act(async () => { await result.current.newChat(null); });
  await act(async () => { lookup.reject(new Error("offline")); await pending; });
  expect(viewing.current).toBeNull();
  expect(getDraft()).toBe("");
});

test("a current lookup failure leaves the displayed session and composer untouched", async () => {
  vi.mocked(api.sessionsList).mockRejectedValueOnce(new Error("offline"));
  const { result, viewing } = setup();
  await act(async () => { await expect(result.current.openSession(id)).rejects.toThrow("offline"); });
  expect(viewing.current).toBe(old.id);
  expect(getDraft()).toBe("iau");
});

test("unmounting prevents a pending metadata reply from opening a session", async () => {
  const lookup = deferred<Awaited<ReturnType<typeof api.sessionsList>>>();
  vi.mocked(api.sessionsList).mockReturnValueOnce(lookup.promise);
  const { result, unmount, viewing } = setup();
  let pending!: Promise<void>;
  act(() => { pending = result.current.openSession(id); });
  unmount();
  await act(async () => { lookup.resolve([{ ...saved, modelId: saved.modelId }]); await pending; });
  expect(viewing.current).toBe(old.id);
  expect(getDraft()).toBe("iau");
});
