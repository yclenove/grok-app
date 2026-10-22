import { useCallback, useEffect, useState } from "react";
import { sessionImportEntry, type CatalogEntry } from "@/lib/api/sessionMigration";

type MetadataState = {
  importId: string;
  entry: CatalogEntry | null;
  busy: boolean;
  error: boolean;
};

/** Metadata never owns the reader's pinned revision, leaf, or search state. */
export function useImportedEntry(
  importId: string | null,
  knownEntry?: CatalogEntry,
  refreshVersion = 0,
) {
  const [state, setState] = useState<MetadataState | null>(null);
  const [retry, setRetry] = useState(0);
  const reload = useCallback(() => setRetry((version) => version + 1), []);
  const known = knownEntry?.importId === importId ? knownEntry : undefined;

  useEffect(() => {
    if (!importId || known) return;
    let current = true;
    setState((previous) => ({
      importId,
      entry: previous?.importId === importId ? previous.entry : null,
      busy: true,
      error: false,
    }));
    void sessionImportEntry(importId).then((entry) => {
      if (!current) return;
      // A mismatched response must never label a different archive.
      if (entry && entry.importId !== importId) throw new Error("metadata owner mismatch");
      setState({ importId, entry, busy: false, error: false });
    }).catch(() => {
      if (current) setState({ importId, entry: null, busy: false, error: true });
    });
    return () => { current = false; };
  }, [importId, known, refreshVersion, retry]);

  const active = importId && state?.importId === importId ? state : null;
  return {
    entry: importId ? known ?? active?.entry ?? null : null,
    busy: !!importId && !known && (active?.busy ?? true),
    error: !!importId && !known && !!active?.error,
    reload,
  };
}
