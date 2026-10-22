import { useLayoutEffect, useRef } from "react";
import type { createT } from "@/i18n";
import { isDialogFocusAllowed } from "@/lib/a11yFocus";
import { importErrorKey } from "@/lib/sessionImport/errorCopy";

export function ImportedReaderError(props: {
  tr: ReturnType<typeof createT>;
  error: string | null;
  busy: boolean;
  copyBusy: boolean;
  onReload: () => void | Promise<void>;
  onCopy: () => void | Promise<void>;
}) {
  const restore = useRef<{ trigger: HTMLButtonElement; dialog: Element } | null>(null);
  // Run before a new activation can claim focus; a previous passive commit
  // must not consume that claim after its recovery button has been clicked.
  useLayoutEffect(() => {
    if (props.busy) return;
    const pending = restore.current;
    restore.current = null;
    if (props.error || !pending || !pending.dialog.isConnected) return;
    const active = document.activeElement;
    // Restore only focus lost with the successful recovery control, never a
    // deliberate user move or focus owned by a newer overlay/reader identity.
    if (active && active !== document.body && active !== pending.trigger && active.isConnected) return;
    const target = pending.dialog.querySelector<HTMLInputElement>(".session-import__reader-search input");
    if (target && !target.disabled && isDialogFocusAllowed(target)) target.focus({ preventScroll: true });
  }, [props.busy, props.error]);
  if (!props.error) return null;
  return (
    <p role="alert" className="settings-row__desc" data-import-reader-error="1" aria-busy={props.busy}>
      {props.tr(importErrorKey(props.error))}
      <button
        type="button"
        className="btn btn--sm btn--ghost"
        data-import-reader-reload="1"
        disabled={props.busy}
        onClick={(event) => {
          const trigger = event.currentTarget;
          if (event.defaultPrevented || props.busy || !isDialogFocusAllowed(trigger)) return;
          const dialog = trigger.closest('[role="dialog"]');
          restore.current = document.activeElement === trigger && dialog ? { trigger, dialog } : null;
          void props.onReload();
        }}
      >
        {props.tr("sessionImport.refresh")}
      </button>
      <button
        type="button"
        className="btn btn--sm btn--ghost"
        data-import-error-copy="1"
        disabled={props.copyBusy}
        onClick={() => void props.onCopy()}
      >
        {props.tr("sessionImport.copyText")}
      </button>
    </p>
  );
}
