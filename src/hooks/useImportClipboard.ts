import { useCallback, useEffect, useMemo, useState } from "react";

export type ImportClipboardStatus = "idle" | "pending" | "copied" | "failed";

/** Clipboard completion belongs to the reader/error that initiated it. */
export function useImportClipboard(owner: string | null) {
  const scope = useMemo(() => ({ owner, active: false, flight: null as object | null }), [owner]);
  const [result, setResult] = useState({ scope, status: "idle" as ImportClipboardStatus });
  useEffect(() => {
    scope.active = true;
    return () => { scope.active = false; scope.flight = null; };
  }, [scope]);

  const copy = useCallback(async (text: string) => {
    if (!scope.active || scope.owner == null || scope.flight || !text) return false;
    const flight = {};
    scope.flight = flight;
    setResult({ scope, status: "pending" });
    try {
      await navigator.clipboard.writeText(text);
      if (scope.active && scope.flight === flight) setResult({ scope, status: "copied" });
      return true;
    } catch {
      if (scope.active && scope.flight === flight) setResult({ scope, status: "failed" });
      return false;
    } finally {
      if (scope.flight === flight) scope.flight = null;
    }
  }, [scope]);

  return { copy, status: result.scope === scope ? result.status : "idle" as ImportClipboardStatus };
}
