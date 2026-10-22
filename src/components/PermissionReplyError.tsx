import type { createT } from "@/i18n";

/** Keep the Host failure visible after a cached/pulled card is restored. */
export function PermissionReplyError({ error, responseError, tr }: {
  error: string | null;
  responseError?: string | null;
  tr: ReturnType<typeof createT>;
}) {
  const automatic = responseError?.trim();
  const message = error || (automatic ? tr("perm.replyFailed", { error: automatic }) : null);
  return message ? <p className="perm-bar__error" role="alert">{message}</p> : null;
}
