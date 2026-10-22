import type { ExternalQueueSync } from "@/hooks/useExternalSendQueue";
import type { createT } from "@/i18n";

export function SendQueueSyncNotice({ sync, tr }: {
  sync: ExternalQueueSync; tr: ReturnType<typeof createT>;
}) {
  if (!sync.enabled || (sync.ready && !sync.error)) return null;
  return <div className="composer__queue-hold composer__queue-sync" role={sync.error ? "alert" : "status"}>
    <span className="composer__queue-hold-text">
      {tr(sync.error ? "composer.queueSyncError" : "composer.queueSyncLoading")}
    </span>
    {sync.error && <button type="button" className="composer__queue-hold-retry"
      disabled={sync.loading} onClick={sync.refresh}>
      {tr(sync.loading ? "composer.queueSyncLoading" : "composer.queueSyncRefresh")}
    </button>}
  </div>;
}
