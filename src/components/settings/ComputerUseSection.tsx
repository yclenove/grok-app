import { useMemo, useRef, useState } from "react";
import { createT, intlLocale, type Locale } from "@/i18n";
import { Select } from "@/components/Select";
import { GlassModal } from "@/components/GlassModal";
import { UiSwitch } from "./shared";
import { useComputerUseSettings } from "@/hooks/useComputerUseSettings";
import * as api from "@/lib/api/computerUse";
import {
  iconSrc,
  permissionStatus,
  type Approval,
  type Confirmation,
  type Foreground,
  type PermissionKind,
} from "@/lib/computer-use/settings";
import { sanitizePlain } from "@/lib/computer-use/approvalDesk";
import {
  channelFor,
  shippedReleaseEvidence,
  type ReleaseEvidence,
} from "@/lib/computer-use/releaseGate";

/** Settings → Extensions → Computer Use. State stays inside the domain module. */
export function ComputerUseSection({
  locale,
  evidence = shippedReleaseEvidence(),
}: {
  locale: Locale;
  /** Section 8 report. The shipped default is no report, which stays experimental. */
  evidence?: ReleaseEvidence | null;
}) {
  const t = useMemo(() => createT(locale), [locale]);
  const channel = channelFor(evidence);
  const s = useComputerUseSettings();
  const [revoking, setRevoking] = useState<Approval | null>(null);
  const [restartNeeded, setRestartNeeded] = useState(false);
  const cancelRef = useRef<HTMLButtonElement>(null);
  const policy = s.settings.data;
  const report = s.doctor.data;
  const disabled = !s.desktop || s.busy || s.settings.loading || s.settings.error || !policy;
  const run = s.mutate;
  const statusKeys = {
    granted: "onboarding.status.granted",
    missing: "onboarding.status.missing",
    unknown: "cu.settings.unknown",
  } as const;

  function error(retry: () => void) {
    return (
      <div className="settings-row settings-row--stack">
        <div className="settings-row__hint is-danger" role="alert">
          {t("cu.settings.error")}
        </div>
        <button type="button" className="btn btn--ghost btn--sm" disabled={s.busy} onClick={retry}>
          {t("cu.settings.retry")}
        </button>
      </div>
    );
  }

  function permission(kind: PermissionKind, title: string, body: string) {
    const status = report ? permissionStatus(report, kind) : "unknown";
    return (
      <section className="settings-row" aria-label={title}>
        <div className="settings-row__text">
          <div className="settings-row__label">{title}</div>
          <div className="settings-row__desc">{body}</div>
          <div className="settings-row__hint" role="status">{t(statusKeys[status])}</div>
        </div>
        {status !== "granted" && (
          <button
            type="button"
            className="btn btn--ghost btn--sm"
            disabled={disabled || s.doctor.loading}
            onClick={() => void run(async () => {
              await api.cuOpenSystemSettings(kind);
              if (kind === "screen_recording") setRestartNeeded(true);
            })}
          >
            {t("cu.settings.authorize")}
          </button>
        )}
      </section>
    );
  }

  async function restart() {
    await run(async () => {
      const { prepareForAppUpdate } = await import("@/lib/api/system");
      await prepareForAppUpdate();
      const { relaunch } = await import("@tauri-apps/plugin-process");
      await relaunch();
    });
  }

  function closeRevoke() {
    if (s.busy) return;
    setRevoking(null);
    s.clearError();
  }

  function grantedAt(row: Approval) {
    if (row.grantedMs == null || !Number.isFinite(row.grantedMs)) return t("cu.settings.dateUnknown");
    const date = new Date(row.grantedMs);
    return Number.isNaN(date.getTime())
      ? t("cu.settings.dateUnknown")
      : t("cu.settings.granted", { date: date.toLocaleString(intlLocale(locale)) });
  }

  return (
    <div className="settings-computer-use">
      <p className="settings-page__lead">
        {t(channel === "formal" ? "cu.settings.descFormal" : "cu.settings.desc")}
      </p>
      {!s.desktop && <p className="settings-row__hint" role="status">{t("cu.settings.desktopOnly")}</p>}
      {s.saveError && !revoking && (
        <div className="settings-row__hint is-danger" role="alert">{t("cu.settings.saveError")}</div>
      )}
      <section className="settings-card" id="settings-anchor-ext-computer" aria-label={t("cu.settings.title")}>
        <div className="settings-row">
          <div className="settings-row__text">
            <div className="settings-row__label">{t("cu.settings.enable")}</div>
            <div className="settings-row__desc">{t("cu.settings.enableDesc")}</div>
          </div>
          <UiSwitch
            checked={policy?.enabled ?? false}
            disabled={disabled}
            label={t("cu.settings.enable")}
            onChange={enabled => void run(() => api.cuSettingsEnabled(enabled))}
          />
        </div>
        {s.settings.loading && <div className="settings-row" role="status">{t("cu.settings.loading")}</div>}
        {s.settings.error && error(() => void s.settings.refresh())}
      </section>

      <section className="settings-card" id="settings-anchor-ext-computer-release" aria-label={t("cu.settings.release")}>
        <div className="settings-row">
          <div className="settings-row__text">
            <div className="settings-row__label">{t("cu.settings.release")}</div>
            <div className="settings-row__desc">{t("cu.settings.releaseBody")}</div>
          </div>
          <span role="status">
            {t(channel === "formal" ? "cu.settings.release.formal" : "cu.settings.release.experimental")}
          </span>
        </div>
      </section>

      <section className="settings-card" id="settings-anchor-ext-computer-permissions" aria-label={t("cu.settings.permissions")}>
        <div className="settings-row">
          <div className="settings-row__label">{t("cu.settings.permissions")}</div>
          <button
            type="button"
            className="btn btn--ghost btn--sm"
            disabled={disabled || !policy?.enabled || s.doctor.loading}
            onClick={() => void s.doctor.refresh()}
          >
            {t("cu.settings.refresh")}
          </button>
        </div>
        {!policy?.enabled ? (
          <div className="settings-row" role="status">{t("cu.settings.disabled")}</div>
        ) : s.doctor.loading && !report ? (
          <div className="settings-row" role="status">{t("cu.settings.loading")}</div>
        ) : s.doctor.error ? error(() => void s.doctor.refresh()) : report && (
          <>
            <div className="settings-row" role="status">
              {t(report.ready ? "cu.settings.ready" : "cu.settings.limited")}
            </div>
            {report.capabilities.platform === "macos" ? (
              <>
                {permission("accessibility", t("onboarding.accessibility.title"), t("cu.settings.accessBody"))}
                {permission("screen_recording", t("onboarding.screen_recording.title"), t("cu.settings.screenBody"))}
                {permission("input_monitoring", t("cu.settings.input"), t("cu.settings.inputBody"))}
                {(restartNeeded || permissionStatus(report, "screen_recording") === "missing") && (
                  <div className="settings-row">
                    <div className="settings-row__desc">{t("cu.settings.restartBody")}</div>
                    <button type="button" className="btn btn--ghost btn--sm" disabled={disabled} onClick={() => void restart()}>
                      {t("cu.settings.restart")}
                    </button>
                  </div>
                )}
              </>
            ) : (
              <div className="settings-row settings-row--stack">
                <div className="settings-row__desc">
                  {t(report.capabilities.platform === "windows" ? "cu.settings.windows"
                    : report.issues.includes("session_type: wayland") ? "cu.settings.wayland" : "cu.settings.linux")}
                </div>
              </div>
            )}
          </>
        )}
      </section>

      <section className="settings-card" id="settings-anchor-ext-computer-approvals" aria-label={t("cu.settings.approvals")}>
        <div className="settings-row">
          <div className="settings-row__label">{t("cu.settings.approvals")}</div>
          <button
            type="button"
            className="btn btn--ghost btn--sm"
            disabled={!s.desktop || s.busy || s.approvals.loading}
            onClick={() => void s.approvals.refresh()}
          >
            {t("cu.settings.refresh")}
          </button>
        </div>
        {s.approvals.loading ? (
          <div className="settings-row" role="status">{t("cu.settings.loading")}</div>
        ) : s.approvals.error ? error(() => void s.approvals.refresh())
          : !s.approvals.data?.length ? (
            <div className="settings-row" role="status">{t("approvals.empty")}</div>
          ) : s.approvals.data.map(row => (
            <div className="settings-row" key={row.id}>
              <AppIcon row={row} label={t("approval.icon_label")} />
              <div className="settings-row__text">
                <div className="settings-row__label">{sanitizePlain(row.name)}</div>
                <div className="settings-row__desc">{sanitizePlain(row.imagePath)}</div>
                <div className="settings-row__hint">{grantedAt(row)}</div>
              </div>
              <button
                type="button"
                className="btn btn--ghost btn--sm"
                disabled={s.busy}
                onClick={() => { s.clearError(); setRevoking(row); }}
              >
                {t("cu.settings.revoke")}
              </button>
            </div>
          ))}
      </section>

      <section className="settings-card" id="settings-anchor-ext-computer-confirmation" aria-label={t("cu.settings.confirmation")}>
        <div className="settings-row">
          <div className="settings-row__label">{t("cu.settings.confirmation")}</div>
          <Select
            value={policy?.confirmation ?? "standard"}
            disabled={disabled}
            aria-label={t("cu.settings.confirmation")}
            options={(["off", "standard", "strict"] as const).map(value => ({ value, label: t(`cu.settings.confirmation.${value}`) }))}
            onChange={value => {
              if (policy) void run(() => api.cuSettingsPolicy(value as Confirmation, policy.foreground));
            }}
          />
        </div>
      </section>
      <section className="settings-card" id="settings-anchor-ext-computer-foreground" aria-label={t("cu.settings.foreground")}>
        <div className="settings-row">
          <div className="settings-row__label">{t("cu.settings.foreground")}</div>
          <Select
            value={policy?.foreground ?? "ask"}
            disabled={disabled}
            aria-label={t("cu.settings.foreground")}
            options={(["never", "ask", "always"] as const).map(value => ({ value, label: t(`cu.settings.foreground.${value}`) }))}
            onChange={value => {
              if (policy) void run(() => api.cuSettingsPolicy(policy.confirmation, value as Foreground));
            }}
          />
        </div>
      </section>
      <section className="settings-card" id="settings-anchor-ext-computer-hotkey" aria-label={t("cu.settings.hotkey")}>
        <div className="settings-row">
          <div className="settings-row__text">
            <div className="settings-row__label">{t("cu.settings.hotkey")}</div>
            <div className="settings-row__desc">{t("cu.settings.hotkeyBody")}</div>
          </div>
          <kbd>
            {policy?.hotkey && policy.hotkey !== "default" ? sanitizePlain(policy.hotkey)
              : report ? t(report.capabilities.platform === "macos" ? "cu.settings.hotkey.mac" : "cu.settings.hotkey.other")
                : t("cu.settings.unknown")}
          </kbd>
        </div>
      </section>

      <GlassModal
        open={!!revoking}
        size="sm"
        title={t("cu.settings.revokeTitle", { name: sanitizePlain(revoking?.name ?? "") })}
        onClose={closeRevoke}
        closeLabel={t("common.close")}
        showClose={!s.busy}
        closeOnOverlay={!s.busy}
        initialFocus={() => cancelRef.current}
        footer={
          <>
            <button ref={cancelRef} type="button" className="btn btn--ghost" disabled={s.busy} onClick={closeRevoke}>
              {t("common.cancel")}
            </button>
            <button
              type="button"
              className="btn btn--solid"
              disabled={s.busy}
              onClick={() => {
                if (revoking) void run(() => api.cuApprovalsRevoke(revoking.id)).then(ok => {
                  if (ok) setRevoking(null);
                });
              }}
            >
              {t("cu.settings.revoke")}
            </button>
          </>
        }
      >
        <p className="settings-row__desc">{t("cu.settings.revokeBody")}</p>
        {s.saveError && <p className="settings-row__hint is-danger" role="alert">{t("cu.settings.saveError")}</p>}
      </GlassModal>
    </div>
  );
}

function AppIcon({ row, label }: { row: Approval; label: string }) {
  const [failed, setFailed] = useState(false);
  const source = iconSrc(row.icon);
  return source && !failed ? (
    <img width={32} height={32} src={source} alt={label} onError={() => setFailed(true)} />
  ) : (
    <span className="settings-row__label" aria-hidden>{sanitizePlain(row.name).slice(0, 1)}</span>
  );
}
