import { useCallback, useEffect, useRef, useState } from "react";
import { resolveImportedImage, type ImportedImageScope, type ImportedImageView } from "@/lib/api/sessionMigrationImages";
import { importedEventDisplay } from "@/lib/sessionImport/eventView";

export type ImageViewState = ImportedImageView | { state: "loading" | "error" };
export type ImportedImages = { identity?: string; states: Record<string, ImageViewState>; retry: (id: string) => void };
const EMPTY: Record<string, ImageViewState> = {};

// A cancelled IPC invocation still owns its slot until the Host returns. Rapid
// navigation must not turn two visible requests into unbounded native reads.
let active = 0;
const waiting: Array<() => void> = [];
async function resolveBounded(scope: ImportedImageScope, id: string, signal: AbortSignal) {
  if (active >= 2) await new Promise<void>((done) => waiting.push(done));
  else active++;
  try {
    if (signal.aborted) throw new Error("aborted");
    return await resolveImportedImage(scope, id);
  } finally {
    const next = waiting.shift();
    if (next) next();
    else active--;
  }
}

/** Per-reader cache and lifetime; results never survive a revision/branch change. */
export function useImportedImages(scope: ImportedImageScope | null, events: unknown[], owner: string): ImportedImages {
  const scopeKey = JSON.stringify(scope);
  const identity = JSON.stringify([scopeKey, owner]);
  const ids = [...new Set(events.flatMap((event) => importedEventDisplay(event).parts
    .flatMap((part) => part.kind === "asset" ? [part.assetId] : [])))];
  const idsKey = JSON.stringify(ids);
  const key = JSON.stringify([identity, idsKey]);
  const latest = useRef(key);
  latest.current = key;
  const cache = useRef<{ identity: string; states: Record<string, ImageViewState> }>({ identity, states: {} });
  const [snapshot, setSnapshot] = useState<{ key: string; states: Record<string, ImageViewState> }>({ key, states: {} });
  const [retryVersion, setRetryVersion] = useState(0);
  useEffect(() => {
    const controller = new AbortController();
    if (cache.current.identity !== identity) cache.current = { identity, states: {} };
    const scope = JSON.parse(scopeKey) as ImportedImageScope | null;
    const ids = JSON.parse(idsKey) as string[];
    const present = Object.fromEntries(ids.map((id) => [id, cache.current.states[id] ?? { state: "loading" as const }]));
    setSnapshot({ key, states: present });
    if (scope) for (const id of ids) {
      if (cache.current.states[id]) continue;
      void resolveBounded(scope, id, controller.signal).then((value) => value, () => ({ state: "error" as const }))
        .then((value) => {
          if (controller.signal.aborted || latest.current !== key) return;
          if (Object.keys(cache.current.states).length >= 256) cache.current.states = {};
          cache.current.states[id] = value;
          setSnapshot((current) => current.key === key ? { key, states: { ...current.states, [id]: value } } : current);
        });
    }
    return () => controller.abort();
  }, [identity, idsKey, key, scopeKey, retryVersion]);
  const retry = useCallback((id: string) => {
    if (cache.current.identity !== identity || !JSON.parse(idsKey).includes(id)) return;
    delete cache.current.states[id];
    setRetryVersion((version) => version + 1);
  }, [identity, idsKey]);
  return { identity, states: snapshot.key === key ? snapshot.states : EMPTY, retry };
}
