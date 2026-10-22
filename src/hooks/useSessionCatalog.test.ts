/**
 * @vitest-environment jsdom
 *
 * Catalog list + multi-select live here. Open/new-chat live in useSessionNavigation.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, renderHook } from "@testing-library/react";
import * as api from "@/lib/api";
import {
  sessionSidebarSelectOrder,
  useSessionCatalog,
} from "./useSessionCatalog";
import type { SessionRow } from "@/lib/app/sidebarModels";

function row(
  partial: Partial<SessionRow> & { id: string },
): SessionRow {
  return {
    id: partial.id,
    title: partial.title ?? partial.id,
    projectId: partial.projectId ?? null,
    updatedAt: partial.updatedAt ?? "2026-01-02T00:00:00Z",
    archived: partial.archived,
    pinned: partial.pinned,
  };
}

function setup(projects: { id: string }[] = [{ id: "p1" }]) {
  return renderHook(() =>
    useSessionCatalog({
      projects,
      isDialogOpen: () => false,
    }),
  );
}

describe("sessionSidebarSelectOrder", () => {
  it("lists project sessions then orphans, pinned first", () => {
    const sessions = [
      row({ id: "old", projectId: "p1", updatedAt: "2026-01-01T00:00:00Z" }),
      row({
        id: "pin",
        projectId: "p1",
        pinned: true,
        updatedAt: "2026-01-01T00:00:00Z",
      }),
      row({ id: "orphan", projectId: null }),
    ];
    expect(sessionSidebarSelectOrder(sessions, [{ id: "p1" }])).toEqual([
      "pin",
      "old",
      "orphan",
    ]);
  });

  it("puts pinned chats from any folder at the global top", () => {
    const sessions = [
      row({ id: "p1-old", projectId: "p1", updatedAt: "2026-01-03T00:00:00Z" }),
      row({
        id: "p2-pin",
        projectId: "p2",
        pinned: true,
        updatedAt: "2026-01-01T00:00:00Z",
      }),
      row({ id: "orphan", projectId: null }),
    ];
    expect(
      sessionSidebarSelectOrder(sessions, [{ id: "p1" }, { id: "p2" }]),
    ).toEqual(["p2-pin", "p1-old", "orphan"]);
  });
});

describe("useSessionCatalog", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("enter seeds selection; exit clears it", () => {
    const { result } = setup();
    act(() => {
      result.current.enterSessionSelectMode("a");
    });
    expect(result.current.sessionSelectMode).toBe(true);
    expect([...result.current.selectedSessionIds]).toEqual(["a"]);
    act(() => {
      result.current.exitSessionSelectMode();
    });
    expect(result.current.sessionSelectMode).toBe(false);
    expect(result.current.selectedSessionIds.size).toBe(0);
  });

  it("Cmd-style toggle enters select mode and keeps prior ids", () => {
    const { result } = setup();
    act(() => {
      result.current.toggleSessionSelected("a");
    });
    expect(result.current.sessionSelectMode).toBe(true);
    expect([...result.current.selectedSessionIds]).toEqual(["a"]);
    act(() => {
      result.current.toggleSessionSelected("b");
    });
    expect([...result.current.selectedSessionIds].sort()).toEqual(["a", "b"]);
  });

  it("Shift-click selects a contiguous range in sidebar order", () => {
    const { result } = setup();
    act(() => {
      result.current.setSessions([
        row({ id: "a", projectId: "p1", updatedAt: "2026-01-03T00:00:00Z" }),
        row({ id: "b", projectId: "p1", updatedAt: "2026-01-02T00:00:00Z" }),
        row({ id: "c", projectId: "p1", updatedAt: "2026-01-01T00:00:00Z" }),
      ]);
    });
    act(() => {
      result.current.enterSessionSelectMode("a");
    });
    act(() => {
      result.current.toggleSessionSelected("c", { shiftKey: true });
    });
    expect([...result.current.selectedSessionIds].sort()).toEqual([
      "a",
      "b",
      "c",
    ]);
  });

  it("refreshSessions replaces the catalog from the host list", async () => {
    vi.spyOn(api, "sessionsList").mockResolvedValue([
      {
        id: "n1",
        title: "New",
        projectId: null,
        updatedAt: "2026-01-01T00:00:00Z",
        modelId: null,
      },
    ]);
    vi.spyOn(api, "trayRefresh").mockResolvedValue(undefined as never);
    const { result } = setup();
    await act(async () => {
      await result.current.refreshSessions();
    });
    expect(result.current.sessions.map((s) => s.id)).toEqual(["n1"]);
    expect(api.trayRefresh).toHaveBeenCalled();
  });

  it("reloads the catalog when sessions://changed fires", async () => {
    vi.useFakeTimers();
    vi.spyOn(api, "hasHost").mockReturnValue(true);
    let onChanged: ((payload: { reason?: string; sessionId?: string }) => void) | undefined;
    vi.spyOn(api, "listen").mockImplementation(async (event, handler) => {
      if (event === "sessions://changed") {
        onChanged = handler as typeof onChanged;
      }
      return () => {};
    });
    vi.spyOn(api, "sessionsList").mockResolvedValue([
      {
        id: "fresh",
        title: "Fresh",
        projectId: null,
        updatedAt: "2026-01-03T00:00:00Z",
        modelId: null,
      },
    ]);
    vi.spyOn(api, "trayRefresh").mockResolvedValue(undefined as never);
    const { result } = setup();
    await act(async () => {
      await Promise.resolve();
    });
    expect(onChanged).toBeTypeOf("function");
    await act(async () => {
      onChanged?.({ reason: "turn", sessionId: "fresh" });
      vi.advanceTimersByTime(150);
      await Promise.resolve();
    });
    expect(result.current.sessions.map((s) => s.id)).toEqual(["fresh"]);
  });

  it("drops selection for archived sessions", () => {
    const { result } = setup();
    act(() => {
      result.current.setSessions([
        row({ id: "keep" }),
        row({ id: "gone" }),
      ]);
    });
    act(() => {
      result.current.enterSessionSelectMode("gone");
    });
    act(() => {
      result.current.setSessions([
        row({ id: "keep" }),
        row({ id: "gone", archived: true }),
      ]);
    });
    expect([...result.current.selectedSessionIds]).toEqual([]);
    expect(result.current.selectableSessionCount).toBe(1);
  });

  it("coalesces continuation publication and retry events using authoritative native rows", async () => {
    vi.useFakeTimers();
    vi.spyOn(api, "hasHost").mockReturnValue(true);
    let changed: ((payload: { reason: string; sessionId: string }) => void) | undefined;
    const unlisten = vi.fn();
    vi.spyOn(api, "listen").mockImplementation(async (event, handler) => {
      if (event === "sessions://changed") changed = handler as typeof changed;
      return unlisten;
    });
    const existing = { id: "existing", title: "Existing", projectId: null,
      updatedAt: "2026-09-24T00:00:00Z", modelId: null };
    const continuation = { id: "native-continuation", title: "Reviewed source name", projectId: "p1",
      updatedAt: "2026-09-25T00:00:00Z", modelId: null };
    vi.spyOn(api, "sessionsList").mockResolvedValue([continuation, existing]);
    vi.spyOn(api, "trayRefresh").mockResolvedValue(undefined);
    const connect = vi.spyOn(api, "sessionConnect");
    const send = vi.spyOn(api, "sessionSend");
    const { result, unmount } = setup();
    await act(async () => { await Promise.resolve(); });
    act(() => {
      result.current.setSessions([row(existing)]);
      result.current.enterSessionSelectMode(existing.id);
      for (let i = 0; i < 3; i++) changed?.({ reason: "import-continuation", sessionId: continuation.id });
    });
    expect(api.sessionsList).not.toHaveBeenCalled();
    await act(async () => { await vi.advanceTimersByTimeAsync(150); });
    expect(api.sessionsList).toHaveBeenCalledOnce();
    expect(api.trayRefresh).toHaveBeenCalledOnce();
    expect(result.current.sessions.map((item) => item.id)).toEqual([continuation.id, existing.id]);
    expect(result.current.sessions[0]).toMatchObject({ title: continuation.title, projectId: "p1" });
    expect([...result.current.selectedSessionIds]).toEqual([existing.id]);
    expect(connect).not.toHaveBeenCalled();
    expect(send).not.toHaveBeenCalled();
    unmount();
    expect(unlisten).toHaveBeenCalledOnce();
    changed?.({ reason: "import-continuation", sessionId: continuation.id });
    await act(async () => { await vi.advanceTimersByTimeAsync(150); });
    expect(api.sessionsList).toHaveBeenCalledOnce();
  });

  it("preserves the visible catalog when a continuation refresh fails, then recovers", async () => {
    vi.useFakeTimers();
    vi.spyOn(api, "hasHost").mockReturnValue(true);
    let changed: (() => void) | undefined;
    vi.spyOn(api, "listen").mockImplementation(async (event, handler) => {
      if (event === "sessions://changed") changed = () => handler({} as never);
      return () => {};
    });
    vi.spyOn(api, "sessionsList").mockRejectedValueOnce(new Error("transient catalog read"))
      .mockResolvedValueOnce([{ id: "native-continuation", title: "Source", projectId: null,
        updatedAt: "2026-09-25T00:00:00Z", modelId: null }]);
    vi.spyOn(api, "trayRefresh").mockResolvedValue(undefined);
    const { result } = setup();
    await act(async () => { await Promise.resolve(); });
    act(() => { result.current.setSessions([row({ id: "existing" })]); changed?.(); });
    await act(async () => { await vi.advanceTimersByTimeAsync(150); });
    expect(result.current.sessions.map((item) => item.id)).toEqual(["existing"]);
    expect(api.trayRefresh).not.toHaveBeenCalled();
    act(() => changed?.());
    await act(async () => { await vi.advanceTimersByTimeAsync(150); });
    expect(result.current.sessions.map((item) => item.id)).toEqual(["native-continuation"]);
    expect(api.trayRefresh).toHaveBeenCalledOnce();
  });
});
