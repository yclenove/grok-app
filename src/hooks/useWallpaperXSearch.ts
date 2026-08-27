import { useCallback, useEffect, useRef, useState } from "react";
import * as api from "@/lib/api";
import {
  createWallpaperXSearchRequestId,
  isWallpaperXSearchProgress,
  type WallpaperXSearchProgress,
  type WallpaperXSearchStage,
} from "@/lib/wallpaperXSearch";
import type { WallpaperSearchResult } from "@/lib/wallpaperSource";

export type WallpaperXSearchClient = {
  search: (
    query: string,
    sort: "top" | "latest",
    requestId: string,
  ) => Promise<WallpaperSearchResult>;
  cancel: (requestId: string) => Promise<boolean>;
  listenProgress: (
    handler: (progress: WallpaperXSearchProgress) => void,
  ) => Promise<() => void>;
};

const DEFAULT_CLIENT: WallpaperXSearchClient = {
  search: api.wallpaperXSearch,
  cancel: api.wallpaperXSearchCancel,
  listenProgress: api.listenWallpaperXSearchProgress,
};

export type UseWallpaperXSearchResult = {
  busy: boolean;
  requestId: string | null;
  stage: WallpaperXSearchStage | null;
  search: (
    query: string,
    sort: "top" | "latest",
  ) => Promise<WallpaperSearchResult | null>;
  cancel: () => Promise<boolean>;
};

/**
 * Own one X-search generation at a time. A cancel or replacement invalidates
 * the previous generation immediately, so late IPC results cannot repopulate
 * a reopened modal even if an older Host takes time to stop.
 */
export function useWallpaperXSearch(
  client: WallpaperXSearchClient = DEFAULT_CLIENT,
  requestIdFactory: () => string = createWallpaperXSearchRequestId,
): UseWallpaperXSearchResult {
  const [busy, setBusy] = useState(false);
  const [requestId, setRequestId] = useState<string | null>(null);
  const [stage, setStage] = useState<WallpaperXSearchStage | null>(null);
  const activeRequestRef = useRef<string | null>(null);
  const generationRef = useRef(0);
  const mountedRef = useRef(true);

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    void client
      .listenProgress((progress) => {
        if (
          !isWallpaperXSearchProgress(progress) ||
          progress.requestId !== activeRequestRef.current ||
          !mountedRef.current
        ) {
          return;
        }
        setStage(progress.stage);
      })
      .then((cleanup) => {
        if (disposed) cleanup();
        else unlisten = cleanup;
      })
      .catch(() => {
        // Progress is advisory. The invoke result remains authoritative.
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [client]);

  const cancel = useCallback(async (): Promise<boolean> => {
    const activeRequest = activeRequestRef.current;
    if (!activeRequest) return false;
    generationRef.current += 1;
    activeRequestRef.current = null;
    if (mountedRef.current) {
      setBusy(false);
      setRequestId(null);
      setStage(null);
    }
    try {
      return await client.cancel(activeRequest);
    } catch {
      return false;
    }
  }, [client]);

  const search = useCallback(
    async (
      query: string,
      sort: "top" | "latest",
    ): Promise<WallpaperSearchResult | null> => {
      if (activeRequestRef.current) {
        await cancel();
      }

      const nextRequestId = requestIdFactory();
      const generation = generationRef.current + 1;
      generationRef.current = generation;
      activeRequestRef.current = nextRequestId;
      if (mountedRef.current) {
        setBusy(true);
        setRequestId(nextRequestId);
        setStage("preparing");
      }

      try {
        const result = await client.search(query, sort, nextRequestId);
        if (
          generationRef.current !== generation ||
          activeRequestRef.current !== nextRequestId
        ) {
          return null;
        }
        if (result.meta?.requestId && result.meta.requestId !== nextRequestId) {
          return null;
        }
        return result;
      } catch (error) {
        if (
          generationRef.current !== generation ||
          activeRequestRef.current !== nextRequestId
        ) {
          return null;
        }
        throw error;
      } finally {
        if (
          generationRef.current === generation &&
          activeRequestRef.current === nextRequestId
        ) {
          activeRequestRef.current = null;
          if (mountedRef.current) {
            setBusy(false);
            setRequestId(null);
            setStage(null);
          }
        }
      }
    },
    [cancel, client, requestIdFactory],
  );

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      generationRef.current += 1;
      const activeRequest = activeRequestRef.current;
      activeRequestRef.current = null;
      if (activeRequest) {
        void client.cancel(activeRequest).catch(() => false);
      }
    };
  }, [client]);

  return { busy, requestId, stage, search, cancel };
}
