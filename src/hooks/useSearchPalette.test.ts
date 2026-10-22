/**
 * @vitest-environment jsdom
 *
 * Palette open/query/filters live in this hook. AppWorkbench only supplies
 * action dispatch and session/project pick.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, renderHook } from "@testing-library/react";
import { createT } from "@/i18n";
import {
  sessionImportCatalogRebuild,
  sessionImportCatalogRebuildCancel,
  sessionsSearch,
} from "@/lib/api";
import { useSearchPalette } from "./useSearchPalette";
import type { PaletteActionDef } from "@/lib/paletteActions";

vi.mock("@/lib/api", () => ({
  sessionsSearch: vi.fn(async () => ({
    hits: [],
    importedHistoryError: null,
  })),
  sessionImportCatalogRebuild: vi.fn(async () => ({
    fromVersion: 1,
    toVersion: 2,
    revisionsScanned: 1,
    revisionsRebuilt: 1,
    chunksWritten: 1,
    searchableBytes: 32,
    projectionSha256: "a".repeat(64),
  })),
  sessionImportCatalogRebuildStatus: vi.fn(async () => ({
    running: true,
    phase: "rebuilding",
    completedRevisions: 0,
    totalRevisions: 1,
    report: null,
    errorCode: null,
  })),
  sessionImportCatalogRebuildCancel: vi.fn(async () => true),
}));

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.clearAllMocks();
  localStorage.clear();
});

function setup(onRunAction = vi.fn()) {
  return renderHook(() =>
    useSearchPalette({
      sessions: [
        { id: "s1", title: "Hello world", projectId: null },
      ],
      projects: [{ id: "p1", name: "grok-app", path: "/code/grok-app" }],
      tr: createT("en"),
      onRunAction,
      onPickProject: vi.fn(),
      onPickSession: vi.fn(),
    }),
  );
}

describe("useSearchPalette", () => {
  it.each(["close", "clear", "title"] as const)(
    "ignores in-flight content hits and errors after %s stops scanning",
    async (stop) => {
      vi.useFakeTimers();
      let finish!: (hits: Awaited<ReturnType<typeof sessionsSearch>>) => void;
      vi.mocked(sessionsSearch).mockReturnValueOnce(
        new Promise((resolve) => { finish = resolve; }),
      );
      const { result } = setup();
      act(() => {
        result.current.openBlank();
        result.current.setQuery("needle");
      });
      await act(async () => { await vi.advanceTimersByTimeAsync(280); });
      expect(sessionsSearch).toHaveBeenCalledWith("needle", 20);
      expect(result.current.contentLoading).toBe(true);
      act(() => {
        if (stop === "close") result.current.closePalette();
        if (stop === "clear") result.current.setQuery("");
        if (stop === "title") result.current.applyMode("title");
      });
      await act(async () => {
        finish({
          hits: [{
            id: "late-content",
            title: "needle",
            projectId: null,
            snippet: "needle in the journal",
            matchCount: 1,
            updatedAt: "2026-09-06T00:00:00Z",
            archived: false,
          }],
          importedHistoryError: "SCHEMA_UNSUPPORTED",
        });
      });
      expect(result.current.sessionHits.map((hit) => hit.id)).not.toContain("late-content");
      expect(result.current.contentLoading).toBe(false);
      expect(result.current.contentError).toBeNull();
    },
  );

  it("keeps valid hits while exposing an imported-history search error", async () => {
    vi.useFakeTimers();
    vi.mocked(sessionsSearch).mockResolvedValueOnce({
      hits: [{
        id: "partial-content",
        title: "Needle result",
        projectId: null,
        snippet: "needle in a readable journal",
        matchCount: 1,
        updatedAt: "2026-09-06T00:00:00Z",
        archived: false,
      }],
      importedHistoryError: "SCHEMA_UNSUPPORTED",
    });
    const { result } = setup();
    act(() => {
      result.current.openBlank();
      result.current.setQuery("needle");
    });

    await act(async () => { await vi.advanceTimersByTimeAsync(280); });

    expect(result.current.sessionHits.map((hit) => hit.id)).toContain("partial-content");
    expect(result.current.contentError).toBe("SCHEMA_UNSUPPORTED");
    expect(result.current.emptyState).toBeNull();
  });

  it("turns an IPC rejection into an observable content error", async () => {
    vi.useFakeTimers();
    vi.mocked(sessionsSearch).mockRejectedValueOnce(new Error("private path"));
    const { result } = setup();
    act(() => {
      result.current.openBlank();
      result.current.setQuery("needle");
    });

    await act(async () => { await vi.advanceTimersByTimeAsync(280); });

    expect(result.current.contentError).toBe("REQUEST_FAILED");
    expect(result.current.emptyState).toBeNull();
  });

  it("rebuilds an old imported index and reruns the active search", async () => {
    vi.useFakeTimers();
    vi.mocked(sessionsSearch)
      .mockResolvedValueOnce({
        hits: [],
        importedHistoryError: "SCHEMA_UNSUPPORTED",
      })
      .mockResolvedValueOnce({
        hits: [],
        importedHistoryError: null,
      });
    const { result } = setup();
    act(() => {
      result.current.openBlank();
      result.current.setQuery("needle");
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(280);
    });
    expect(result.current.contentError).toBe("SCHEMA_UNSUPPORTED");

    await act(async () => {
      await result.current.rebuildCatalogIndex();
    });
    expect(result.current.catalogRebuild?.phase).toBe("complete");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(280);
    });

    expect(sessionImportCatalogRebuild).toHaveBeenCalledTimes(1);
    expect(sessionsSearch).toHaveBeenCalledTimes(2);
    expect(result.current.contentError).toBeNull();
  });

  it("keeps only a stable error code when catalog rebuilding fails", async () => {
    vi.mocked(sessionImportCatalogRebuild).mockRejectedValueOnce(
      new Error("DISK_SPACE_LOW: C:\\private\\catalog.sqlite"),
    );
    const { result } = setup();

    await act(async () => {
      await result.current.rebuildCatalogIndex();
    });

    expect(result.current.catalogRebuild).toMatchObject({
      running: false,
      phase: "failed",
      errorCode: "DISK_SPACE_LOW",
    });
    expect(JSON.stringify(result.current.catalogRebuild)).not.toContain("private");
  });

  it("forwards rebuild cancellation only while maintenance is active", async () => {
    let fail!: (error: Error) => void;
    vi.mocked(sessionImportCatalogRebuild).mockReturnValueOnce(
      new Promise((_, reject) => {
        fail = reject;
      }),
    );
    const { result } = setup();
    act(() => {
      void result.current.rebuildCatalogIndex();
    });
    await act(async () => {
      await result.current.cancelCatalogRebuild();
    });
    expect(sessionImportCatalogRebuildCancel).toHaveBeenCalledTimes(1);
    await act(async () => {
      fail(new Error("PLAN_STALE: private cancellation detail"));
    });
    expect(result.current.catalogRebuild).toMatchObject({
      running: false,
      phase: "cancelled",
      errorCode: null,
    });
  });

  it("openBlank clears the query and opens", () => {
    const { result } = setup();
    act(() => {
      result.current.setQuery("old");
    });
    act(() => {
      result.current.openBlank();
    });
    expect(result.current.open).toBe(true);
    expect(result.current.query).toBe("");
  });

  it("closes and clears query before dispatching an action", () => {
    const onRunAction = vi.fn();
    const { result } = setup(onRunAction);
    act(() => {
      result.current.openBlank();
    });
    const action: PaletteActionDef = result.current.actions[0];
    expect(action).toBeTruthy();
    act(() => {
      result.current.runAction(action);
    });
    expect(result.current.open).toBe(false);
    expect(result.current.query).toBe("");
    expect(onRunAction).toHaveBeenCalledWith(action);
  });

  it("picking an importedHistory hit keeps kind for mouse and keyboard", () => {
    const onPickSession = vi.fn();
    const { result } = renderHook(() =>
      useSearchPalette({
        sessions: [],
        projects: [],
        tr: createT("en"),
        onRunAction: vi.fn(),
        onPickProject: vi.fn(),
        onPickSession,
      }),
    );
    const imported = {
      id: "imp:00000000-0000-4000-8000-000000000001",
      title: "Imported",
      projectId: null,
      titleMatch: true,
      contentMatch: false,
      kind: "importedHistory" as const,
    };
    act(() => {
      result.current.pickSession(imported);
    });
    expect(onPickSession).toHaveBeenCalledTimes(1);
    expect(onPickSession.mock.calls[0][0].kind).toBe("importedHistory");
  });

  it("filters sessions by query without asking the host", () => {
    const { result } = setup();
    act(() => {
      result.current.openBlank();
      result.current.setQuery("hello");
    });
    expect(result.current.sessionHits.map((h) => h.id)).toEqual(["s1"]);
    act(() => {
      result.current.setQuery("zzzz");
    });
    expect(result.current.sessionHits).toEqual([]);
  });
});
