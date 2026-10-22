import { useEffect, useMemo, useRef, useState } from "react";
import { createT, type Locale } from "@/i18n";
import type { Project, SessionRow } from "@/lib/app/sidebarModels";
import { importErrorKey } from "@/lib/sessionImport/errorCopy";
import { GlassModal } from "@/components/GlassModal";
import { SessionImportHost } from "./SessionImportHost";

/** Creation is already durable if opening fails. Retry the same native id;
 * never create a replacement continuation or expand its context into composer. */
export function SessionImportWorkbenchHost({ locale, onProjectsChanged, onOpenSession }: {
  locale: Locale;
  onProjectsChanged?: () => void;
  onOpenSession: (target: SessionRow | string, project?: Project | null) => void | Promise<void>;
}) {
  const tr = useMemo(() => createT(locale), [locale]);
  const [pending, setPending] = useState<{ id: string; busy: boolean; error: string | null } | null>(null);
  const generation = useRef(0);
  useEffect(() => () => { ++generation.current; }, []);

  async function open(id: string) {
    const token = ++generation.current;
    setPending((previous) => ({ id, busy: true, error: previous?.id === id ? previous.error : null }));
    try {
      await onOpenSession(id);
      if (token === generation.current) setPending(null);
    } catch (cause) {
      if (token === generation.current) {
        setPending({ id, busy: false, error: cause instanceof Error ? cause.message : String(cause) });
      }
    }
  }

  return <>
    <GlassModal open={!!pending?.error} title={tr("sessionImport.handoffTitle")} size="sm"
      closeLabel={tr("common.close")} showClose={!pending?.busy}
      onClose={() => { if (!pending?.busy) { ++generation.current; setPending(null); } }}
      footer={<button type="button" className="btn btn--primary" disabled={!pending || pending.busy}
        onClick={() => { if (pending) void open(pending.id); }}>{tr("sessionImport.openDraft")}</button>}>
      {pending?.error ? <p role={pending.busy ? "status" : "alert"}>{tr(pending.busy ? "sessionImport.workspace.busy"
        : pending.error === "SESSION_OPEN_MISSING" ? "session.openInNewWindowMissing"
          : importErrorKey(pending.error))}</p> : null}
    </GlassModal>
    <SessionImportHost locale={locale} onProjectsChanged={onProjectsChanged} onOpenNativeSession={open} />
  </>;
}
