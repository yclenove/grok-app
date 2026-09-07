import { describe, expect, it, vi } from "vitest";
import {
  appendGalleryItems,
  dedupeGalleryItems,
  errorCodeFromSearchResult,
  fetchEntireMediaBlob,
  isStaticImageLibraryEntry,
  libraryEntriesToGalleryItems,
  libraryEntryToGalleryItem,
  MEDIA_IPC_CHUNK,
  MEDIA_PROTO_CHUNK,
  mergeAuthoritativeGalleryItems,
  parseContentRangeTotal,
  parseWallpaperSourceError,
  readLocalMediaBlobViaIpc,
  resolveApplySource,
  sortLibraryEntriesStaticFirst,
  type WallpaperGalleryItem,
  type WallpaperLibraryEntry,
} from "./wallpaperSource";

function item(
  partial: Partial<WallpaperGalleryItem> & Pick<WallpaperGalleryItem, "id" | "fullUrl">,
): WallpaperGalleryItem {
  return {
    thumbUrl: partial.thumbUrl ?? partial.fullUrl,
    kind: partial.kind ?? "image",
    source: partial.source ?? "x",
    ...partial,
  };
}

describe("wallpaperSource", () => {
  it.each([
    "imagine_access_denied", "imagine_rate_limited", "imagine_request_rejected",
    "imagine_upstream_failed", "imagine_network_failed", "imagine_result_invalid",
  ] as const)("preserves generation error %s without exposing diagnostics", (code) => {
    expect(parseWallpaperSourceError(new Error(code))).toBe(code);
    expect(errorCodeFromSearchResult({ items: [], errorCode: code, message: "private diagnostic" })).toBe(code);
    expect(errorCodeFromSearchResult({ items: [item({ id: "saved", fullUrl: "https://a/saved.png" })], errorCode: code })).toBeNull();
  });

  it("parses host error codes", () => {
    expect(parseWallpaperSourceError("auth_required")).toBe("auth_required");
    expect(parseWallpaperSourceError("pexels_key_missing")).toBe(
      "pexels_key_required",
    );
    expect(parseWallpaperSourceError("pexels_key_invalid")).toBe(
      "pexels_key_invalid",
    );
    expect(parseWallpaperSourceError(new Error("download_failed: HTTP 403"))).toBe(
      "download_failed",
    );
    expect(parseWallpaperSourceError("url_blocked")).toBe("url_blocked");
    expect(parseWallpaperSourceError("path_not_allowed")).toBe("url_blocked");
    expect(parseWallpaperSourceError("timeout")).toBe("timeout");
    expect(parseWallpaperSourceError("responses_rate_limited")).toBe(
      "rate_limited",
    );
    expect(parseWallpaperSourceError("responses_network")).toBe("search_failed");
    expect(parseWallpaperSourceError("responses_tool_not_called")).toBe(
      "service_unavailable",
    );
    expect(parseWallpaperSourceError("responses_unauthorized")).toBe(
      "auth_required",
    );
    expect(parseWallpaperSourceError("imagine_failed")).toBe("imagine_failed");
    expect(parseWallpaperSourceError("wallpaper_imagine: boom")).toBe(
      "imagine_failed",
    );
    expect(parseWallpaperSourceError("delete_failed: EPERM")).toBe("generic");
    expect(parseWallpaperSourceError("something else")).toBe("generic");
  });

  it("maps empty search results", () => {
    expect(
      errorCodeFromSearchResult({ items: [], errorCode: "auth_required" }),
    ).toBe("auth_required");
    expect(
      errorCodeFromSearchResult({ items: [], errorCode: "imagine_failed" }),
    ).toBe("imagine_failed");
    expect(
      errorCodeFromSearchResult({ items: [], errorCode: "pexels_key_missing" }),
    ).toBe("pexels_key_required");
    expect(
      errorCodeFromSearchResult({ items: [], errorCode: "pexels_key_invalid" }),
    ).toBe("pexels_key_invalid");
    expect(
      errorCodeFromSearchResult({ items: [], errorCode: "timeout" }),
    ).toBe("timeout");
    expect(
      errorCodeFromSearchResult({
        items: [],
        errorCode: "responses_rate_limited",
      }),
    ).toBe("rate_limited");
    expect(
      errorCodeFromSearchResult({
        items: [],
        errorCode: "responses_network",
      }),
    ).toBe("search_failed");
    expect(
      errorCodeFromSearchResult({
        items: [],
        errorCode: "responses_tool_not_called",
      }),
    ).toBe("service_unavailable");
    expect(errorCodeFromSearchResult({ items: [], errorCode: null })).toBe("empty");
    expect(
      errorCodeFromSearchResult({
        items: [item({ id: "1", fullUrl: "https://pbs.twimg.com/a.jpg" })],
        errorCode: "empty",
      }),
    ).toBeNull();
  });

  it("dedupes by url / path", () => {
    const items = dedupeGalleryItems([
      item({ id: "1", fullUrl: "https://a/x.jpg" }),
      item({ id: "2", fullUrl: "https://a/x.jpg" }),
      item({ id: "3", fullUrl: "https://a/y.jpg", localPath: "/tmp/y.jpg" }),
      item({ id: "4", fullUrl: "file:///tmp/y.jpg", localPath: "/tmp/y.jpg" }),
    ]);
    expect(items.map((i) => i.id)).toEqual(["1", "3"]);
  });

  it("keeps remote identity stable after an item gains a local path", () => {
    const remote = item({
      id: "remote",
      fullUrl: "https://images.example.test/wallpaper.jpg",
    });
    const materialized = {
      ...remote,
      localPath: "C:\\wallpapers\\wallpaper.jpg",
    };

    expect(dedupeGalleryItems([materialized, remote])).toEqual([materialized]);
    expect(
      mergeAuthoritativeGalleryItems([materialized], [
        { ...remote, username: "final-author", likes: 12 },
      ]),
    ).toEqual([
      {
        ...remote,
        username: "final-author",
        likes: 12,
        localPath: "C:\\wallpapers\\wallpaper.jpg",
      },
    ]);
  });

  it("uses authoritative membership and ranking while preserving local paths", () => {
    const first = item({
      id: "first",
      fullUrl: "https://images.example.test/first.jpg",
      localPath: "C:\\wallpapers\\first.jpg",
    });
    const second = item({
      id: "second",
      fullUrl: "https://images.example.test/second.jpg",
    });
    const progressiveOnly = item({
      id: "progressive-only",
      fullUrl: "https://images.example.test/progressive-only.jpg",
    });

    const merged = mergeAuthoritativeGalleryItems(
      [first, second, progressiveOnly],
      [
        { ...second, username: "ranked-first" },
        { ...first, username: "ranked-second", localPath: null },
      ],
    );

    expect(merged.map((entry) => entry.id)).toEqual(["second", "first"]);
    expect(merged[1]?.localPath).toBe("C:\\wallpapers\\first.jpg");
    expect(merged.some((entry) => entry.id === "progressive-only")).toBe(false);
  });

  it("appends a page without replacing the existing gallery", () => {
    const first = item({
      id: "first",
      fullUrl: "https://images.example.test/first.jpg",
      localPath: "C:\\wallpapers\\first.jpg",
    });
    const extra = item({
      id: "extra",
      fullUrl: "https://images.example.test/extra.jpg",
    });

    const merged = appendGalleryItems(
      [first],
      [{ ...first, username: "final-author", localPath: null }, extra],
    );

    expect(merged.map((entry) => entry.id)).toEqual(["first", "extra"]);
    expect(merged[0]).toMatchObject({
      username: "final-author",
      localPath: "C:\\wallpapers\\first.jpg",
    });
  });

  it("resolves apply source", () => {
    expect(
      resolveApplySource(
        item({ id: "1", fullUrl: "https://a/x.jpg", localPath: "/w/a.jpg" }),
      ),
    ).toEqual({ kind: "path", path: "/w/a.jpg" });
    expect(
      resolveApplySource(item({ id: "2", fullUrl: "file:///Users/me/a.jpg" })),
    ).toEqual({ kind: "path", path: "/Users/me/a.jpg" });
    expect(
      resolveApplySource(item({ id: "3", fullUrl: "https://pbs.twimg.com/a.jpg" })),
    ).toEqual({ kind: "url", url: "https://pbs.twimg.com/a.jpg" });
  });

  it("maps read_failed to download_failed", () => {
    expect(parseWallpaperSourceError(new Error("read_failed: HTTP 403"))).toBe(
      "download_failed",
    );
    expect(
      parseWallpaperSourceError(new Error("read_failed: short read (10/99 bytes)")),
    ).toBe("download_failed");
  });

  it("parses Content-Range totals", () => {
    expect(parseContentRangeTotal("bytes 0-0/4096")).toBe(4096);
    expect(parseContentRangeTotal("bytes 0-2097151/5000000")).toBe(5_000_000);
    expect(parseContentRangeTotal(null)).toBeNull();
    expect(parseContentRangeTotal("bytes */100")).toBeNull();
  });

  it("reassembles multi-chunk media:// style responses", async () => {
    // Use a small synthetic chunk size so the suite stays fast under load
    // (production path still defaults to MEDIA_PROTO_CHUNK = 2 MiB).
    const chunk = 4096;
    const total = chunk + 1234;
    const bytes = new Uint8Array(total);
    for (let i = 0; i < total; i++) bytes[i] = i % 251;

    const fetchImpl: typeof fetch = async (_input, init) => {
      const headers = init?.headers;
      let range: string | undefined;
      if (headers instanceof Headers) {
        range = headers.get("Range") ?? undefined;
      } else if (Array.isArray(headers)) {
        range = headers.find(([k]) => k.toLowerCase() === "range")?.[1];
      } else if (headers && typeof headers === "object") {
        range = (headers as Record<string, string>).Range;
      }
      if (!range) {
        // Bare GET would only return first chunk (the bug we avoid)
        return new Response(bytes.slice(0, chunk), {
          status: 206,
          headers: {
            "Content-Range": `bytes 0-${chunk - 1}/${total}`,
            "Content-Type": "image/png",
          },
        });
      }
      const m = /^bytes=(\d+)-(\d+)$/.exec(range);
      if (!m) return new Response(null, { status: 400 });
      const start = Number(m[1]);
      const end = Number(m[2]);
      const slice = bytes.slice(start, end + 1);
      return new Response(slice, {
        status: 206,
        headers: {
          "Content-Range": `bytes ${start}-${end}/${total}`,
          "Content-Type": "image/png",
          "Content-Length": String(slice.length),
        },
      });
    };

    const spy = vi.fn(fetchImpl);
    const blob = await fetchEntireMediaBlob("media://localhost/x.png", spy, {
      chunkSize: chunk,
    });
    expect(blob.size).toBe(total);
    const out = new Uint8Array(await blob.arrayBuffer());
    expect(out).toEqual(bytes);
    // Every request must carry a Range header (no truncated bare GET body).
    expect(spy).toHaveBeenCalled();
    for (const call of spy.mock.calls) {
      const headers = call[1]?.headers as Record<string, string> | undefined;
      expect(headers?.Range).toMatch(/^bytes=\d+-\d+$/);
    }
    // multi-chunk path used more than probe+one full read
    expect(spy.mock.calls.length).toBeGreaterThanOrEqual(3);
    // production default remains 2 MiB
    expect(MEDIA_PROTO_CHUNK).toBe(2 * 1024 * 1024);
  });

  it("reassembles bounded raw IPC chunks without loopback fetch", async () => {
    const bytes = Uint8Array.from([0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    const invoke = vi.fn(
      async (command: string, args?: Record<string, unknown>): Promise<unknown> => {
        if (command === "media_file_info") {
          return { bytes: bytes.length, mime: "image/png", name: "wall.png" };
        }
        if (command === "media_read_file_chunk") {
          const offset = Number(args?.offset);
          const length = Number(args?.length);
          return bytes.slice(offset, offset + length).buffer;
        }
        throw new Error(`unexpected command: ${command}`);
      },
    );

    const { blob, info } = await readLocalMediaBlobViaIpc(
      "C:/wallpapers/wall.png",
      invoke,
      { chunkSize: 4 },
    );
    expect(info).toEqual({
      bytes: bytes.length,
      mime: "image/png",
      name: "wall.png",
    });
    expect(new Uint8Array(await blob.arrayBuffer())).toEqual(bytes);
    expect(blob.type).toBe("image/png");
    expect(invoke.mock.calls.map(([command]) => command)).toEqual([
      "media_file_info",
      "media_read_file_chunk",
      "media_read_file_chunk",
      "media_read_file_chunk",
    ]);
    expect(MEDIA_IPC_CHUNK).toBe(8 * 1024 * 1024);
  });

  it("rejects a short raw IPC chunk", async () => {
    const invoke = vi.fn(
      async (command: string): Promise<unknown> =>
        command === "media_file_info"
          ? { bytes: 4, mime: "image/png", name: "wall.png" }
          : Uint8Array.from([1, 2, 3]).buffer,
    );

    await expect(
      readLocalMediaBlobViaIpc("C:/wallpapers/wall.png", invoke, {
        chunkSize: 4,
      }),
    ).rejects.toThrow("short IPC read (3/4 bytes at 0)");
  });

  it("stops IPC reads after cancellation and respects a smaller byte cap", async () => {
    const controller = new AbortController();
    const invoke = vi.fn(async (command: string): Promise<unknown> => {
      if (command === "media_file_info") return { bytes: 8, mime: "image/png" };
      controller.abort();
      return new Uint8Array(4).buffer;
    });
    await expect(readLocalMediaBlobViaIpc("/wall.png", invoke, { maxBytes: 4 }))
      .rejects.toThrow("invalid media size");
    expect(invoke).toHaveBeenCalledTimes(1);
    invoke.mockClear();
    await expect(readLocalMediaBlobViaIpc("/wall.png", invoke, { chunkSize: 4, signal: controller.signal }))
      .rejects.toThrow();
    expect(invoke).toHaveBeenCalledTimes(2);
  });

  it("maps library entries to gallery items (static first)", () => {
    const entries: WallpaperLibraryEntry[] = [
      {
        path: "/w/imagine/2026-08-01/b.mp4",
        name: "b.mp4",
        source: "imagine",
        kind: "video",
        bytes: 10,
        modifiedMs: 200,
      },
      {
        path: "/w/x/2026-08-01/a.jpg",
        name: "a.jpg",
        source: "x",
        kind: "image",
        bytes: 5,
        modifiedMs: 100,
      },
      {
        path: "/w/imagine/2026-08-01/c.png",
        name: "c.png",
        source: "imagine",
        kind: "image",
        bytes: 8,
        modifiedMs: 300,
      },
    ];
    expect(isStaticImageLibraryEntry(entries[0]!)).toBe(false);
    expect(isStaticImageLibraryEntry(entries[1]!)).toBe(true);

    const sorted = sortLibraryEntriesStaticFirst(entries);
    expect(sorted.map((e) => e.name)).toEqual(["c.png", "a.jpg", "b.mp4"]);

    const imagesOnly = sortLibraryEntriesStaticFirst(entries, {
      imagesOnly: true,
    });
    expect(imagesOnly.map((e) => e.name)).toEqual(["c.png", "a.jpg"]);

    const items = libraryEntriesToGalleryItems(entries);
    expect(items.map((i) => i.localPath)).toEqual([
      "/w/imagine/2026-08-01/c.png",
      "/w/x/2026-08-01/a.jpg",
      "/w/imagine/2026-08-01/b.mp4",
    ]);
    expect(items[0]!.source).toBe("imagine");
    expect(items[0]!.kind).toBe("image");
    expect(items[2]!.kind).toBe("video");

    const one = libraryEntryToGalleryItem(entries[1]!);
    expect(one.localPath).toBe("/w/x/2026-08-01/a.jpg");
    expect(one.fullUrl).toBe("file:///w/x/2026-08-01/a.jpg");
  });
});
