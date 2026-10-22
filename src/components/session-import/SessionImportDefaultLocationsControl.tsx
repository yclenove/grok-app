import { useLayoutEffect, useRef, useState } from "react";
import type { DiscoveredRoot, SourceRegistration } from "@/lib/api/sessionMigration";
import { sessionImportSourceRegister } from "@/lib/api/sessionMigrationLocations";
import { importErrorKey } from "@/lib/sessionImport/errorCopy";
import { validateSourceLocation } from "@/lib/sessionImport/sourceLocationState";
import type { SourceChoice } from "@/lib/sessionImport/sourceChoice";
import type { SessionImportTFn } from "./sessionImportHostLabels";
import { SessionImportSourceLocationsControl } from "./SessionImportSourceLocationsControl";

/** Registration happens only after this explicit action; never register/merge default roots while listing. */
export function SessionImportDefaultLocationsControl({ root, disabled, tr, onChoose, scopeKey }: {
  root: DiscoveredRoot; disabled: boolean; tr: SessionImportTFn; onChoose: (choice: SourceChoice) => void;
  scopeKey: string;
}) {
  const trigger = useRef<HTMLButtonElement>(null);
  const lifetime = useRef(0);
  const input = useRef({ scopeKey, disabled });
  const flight = useRef(false);
  const [busy, setBusy] = useState(false);
  const [source, setSource] = useState<SourceRegistration | null>(null);
  const [open, setOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useLayoutEffect(() => () => { ++lifetime.current; flight.current = false; }, []);
  useLayoutEffect(() => {
    if (input.current.scopeKey === scopeKey && input.current.disabled === disabled) return;
    input.current = { scopeKey, disabled }; ++lifetime.current; flight.current = false;
    setBusy(false); setOpen(false); setSource(null); setError(null);
  }, [scopeKey, disabled]);
  const start = async () => {
    if (disabled || open || flight.current || root.provider !== "cursor" || !root.exists || !root.readable) return;
    if (source) { setOpen(true); return; }
    const owner = lifetime.current;
    const intent = scopeKey;
    const owns = () => owner === lifetime.current && intent === input.current.scopeKey && !input.current.disabled;
    const request = { provider: root.provider, path: root.path };
    flight.current = true; setBusy(true); setError(null);
    try {
      const registration = await sessionImportSourceRegister(request);
      if (!owns()) return;
      if (!registration.sourceId || registration.provider !== "cursor" || !registration.rootPath ||
        (registration.location && registration.location.host !== "local")) throw new Error("SOURCE_CHANGED: invalid default installation registration");
      if (registration.location) validateSourceLocation(registration, registration.location);
      // Keep this trigger mounted across registration/dialog phases so closing
      // returns to the same default row instead of a detached old control.
      setSource({ ...registration }); setOpen(true);
    } catch (cause) { if (owns()) setError(String(cause)); }
    finally { if (owns()) { flight.current = false; setBusy(false); } }
  };
  useLayoutEffect(() => { if (open && trigger.current?.isConnected) trigger.current.focus(); }, [open]);
  return <>
    <button ref={trigger} type="button" className="btn btn--sm" data-kind="locations" disabled={disabled || busy}
      onClick={() => void start()}>{tr("sessionImport.locations.title")}</button>
    {busy ? <span role="status">{tr("sessionImport.locations.busy")}</span> : null}
    {error ? <span role="alert">{tr(importErrorKey(error))}</span> : null}
    {source ? <SessionImportSourceLocationsControl source={source} disabled={disabled} tr={tr}
      onChoose={onChoose} autoOpen={open} hideTrigger scopeKey={scopeKey} onClosed={() => setOpen(false)} /> : null}
  </>;
}
