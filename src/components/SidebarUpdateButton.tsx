/**
 * Sidebar brand-row update affordance: circle-arrow-up + accent dot when a
 * newer app version is known. Startup discovery lives in `useUpdater` (download
 * only). Signed install+relaunch needs an in-app confirm; unsigned GitHub
 * download opens the release page with no confirm.
 */

import { useCallback, useState } from "react";
import { IconCircleArrowUp } from "@/components/icons";
import { Tip } from "@/components/ui/tooltip";
import { UpdateInstallConfirmModal } from "@/components/UpdateInstallConfirmModal";
import { useUpdaterContext } from "@/hooks/UpdaterProvider";
import type { UpdateStatus } from "@/hooks/useUpdater";
import {
  isUpdateAffordanceVisible,
  needsInstallAndRestartConfirm,
  isUpdateRestartPending,
  isUpdateInstallPending,
} from "@/lib/appUpdateHonesty";
import { isUpdateSimActive } from "@/lib/updateSim";
import * as api from "@/lib/api";
import type { MessageKey } from "@/i18n";

function statusVersion(status: UpdateStatus): string | undefined {
  if (
    status.state === "available" ||
    status.state === "downloading" ||
    status.state === "ready" ||
    status.state === "installing" ||
    status.state === "preparing-restart" ||
    status.state === "restarting" ||
    status.state === "manual-required" ||
    status.state === "up-to-date"
    || status.state === "error"
  ) {
    return status.version;
  }
  return undefined;
}

function tipKey(status: UpdateStatus): MessageKey {
  if (status.state === "error" && status.installBlocked) return "settings.autoUpdateInstallBlocked";
  if (isUpdateInstallPending(status)) return "settings.autoUpdateRetryInstall";
  if (isUpdateRestartPending(status)) return "settings.autoUpdateRetryRestart";
  switch (status.state) {
    case "downloading":
      return "sidebar.update.downloading";
    case "installing":
      return "sidebar.update.installing";
    case "preparing-restart":
      return "settings.autoUpdatePreparingRestart";
    case "restarting":
      return "sidebar.update.restarting";
    case "ready":
      return "sidebar.update.ready";
    case "manual-required":
      return "sidebar.update.manual";
    default:
      return "sidebar.update.available";
  }
}

export function SidebarUpdateButton({
  t,
}: {
  t: (key: MessageKey, vars?: Record<string, string | number>) => string;
}) {
  const { status, applyAvailableUpdate } = useUpdaterContext();
  const installBlocked = status.state === "error" && status.installBlocked === true;
  const [busyClick, setBusyClick] = useState(false);
  const [confirmInstall, setConfirmInstall] = useState(false);

  const visible = isUpdateAffordanceVisible(status);
  const version = statusVersion(status);
  // Message templates end with optional `{version}` (e.g. " (1.2.3)" or "").
  const tipLabel = t(tipKey(status), {
    version: installBlocked ? (version ?? "") : version ? ` (${version})` : "",
  });

  const runApply = useCallback(async () => {
    if (busyClick || installBlocked) return;
    if (status.state === "installing" || status.state === "preparing-restart" || status.state === "restarting") return;
    setBusyClick(true);
    try {
      const result = await applyAvailableUpdate();
      if (result.kind === "manual") {
        const url = result.downloadUrl || result.releaseUrl;
        if (url) {
          try {
            await api.openExternalUrl(url);
          } catch {
            // Soft-fail: tip still explains manual path; About has full errors.
          }
        }
      }
    } finally {
      setBusyClick(false);
    }
  }, [applyAvailableUpdate, busyClick, installBlocked, status.state]);

  const onClick = useCallback(() => {
    if (busyClick || installBlocked) return;
    if (status.state === "installing" || status.state === "preparing-restart" || status.state === "restarting") return;
    if (needsInstallAndRestartConfirm(status)) {
      setConfirmInstall(true);
      return;
    }
    void runApply();
  }, [busyClick, installBlocked, runApply, status]);

  // Desktop host, or update sim (browser can still exercise the UI).
  if (!visible || (!api.isDesktopHost() && !isUpdateSimActive())) {
    return null;
  }

  const busy =
    busyClick ||
    status.state === "downloading" ||
    status.state === "installing" ||
    status.state === "preparing-restart" ||
    status.state === "restarting";

  return (
    <>
      <Tip label={tipLabel} placement="bottom">
        <button
          type="button"
          className={
            "sidebar-update-btn" +
            (status.state === "downloading" ||
            status.state === "installing" ||
            status.state === "preparing-restart" ||
            status.state === "restarting"
              ? " sidebar-update-btn--busy"
              : "")
          }
          aria-label={tipLabel}
          title={tipLabel}
          disabled={
            installBlocked || status.state === "installing" || status.state === "preparing-restart" || status.state === "restarting"
          }
          onClick={() => onClick()}
        >
          <IconCircleArrowUp size={16} />
          <span
            className={
              "sidebar-update-btn__dot" +
              (busy ? " sidebar-update-btn__dot--pulse" : "")
            }
            aria-hidden
          />
        </button>
      </Tip>
      <UpdateInstallConfirmModal
        open={confirmInstall}
        version={version}
        t={t}
        onClose={() => setConfirmInstall(false)}
        onConfirm={() => {
          setConfirmInstall(false);
          void runApply();
        }}
      />
    </>
  );
}
