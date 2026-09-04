/**
 * Compact “changed files” strip under a finished assistant turn (#998).
 * Click opens existing Changes / Review UI — no new diff engine.
 */

import { memo, useMemo } from "react";
import type { Locale } from "@/i18n";
import { createT } from "@/i18n";
import { IconFileDiff } from "@/components/icons";
import {
  splitTurnChangedFiles,
  turnChangedFileItems,
} from "@/lib/turnChangedFiles";

export const TurnChangedFiles = memo(function TurnChangedFiles({
  paths,
  locale,
  streaming,
  onOpenPath,
  onViewAll,
}: {
  paths: string[];
  locale: Locale;
  streaming?: boolean;
  onOpenPath?: (path: string) => void;
  onViewAll?: () => void;
}) {
  const tr = useMemo(() => createT(locale), [locale]);
  const items = useMemo(() => turnChangedFileItems(paths), [paths]);
  const { visible, hiddenCount } = useMemo(
    () => splitTurnChangedFiles(items),
    [items],
  );

  if (streaming) return null;
  if (!items.length) return null;

  const canOpen = typeof onOpenPath === "function";
  const canViewAll = typeof onViewAll === "function";

  return (
    <div
      className="lobe-turn-changed"
      data-testid="turn-changed-files"
      role="group"
      aria-label={tr("chat.changedFiles.aria")}
    >
      <div className="lobe-turn-changed__head">
        <IconFileDiff size={14} className="lobe-turn-changed__icon" />
        <span className="lobe-turn-changed__title">
          {items.length === 1
            ? tr("chat.editedFilesOne")
            : tr("chat.editedFiles", { n: String(items.length) })}
        </span>
        {canViewAll ? (
          <button
            type="button"
            className="lobe-turn-changed__view-all"
            data-testid="turn-changed-view-all"
            onClick={() => onViewAll()}
          >
            {tr("chat.changedFiles.viewAll")}
          </button>
        ) : null}
      </div>
      <div className="lobe-turn-changed__chips">
        {visible.map((item) => {
          const label = tr("chat.changedFiles.openFile", {
            name: item.name,
          });
          if (!canOpen) {
            return (
              <span
                key={item.path}
                className="lobe-turn-changed__chip lobe-turn-changed__chip--static"
                title={item.path}
              >
                {item.name}
              </span>
            );
          }
          return (
            <button
              key={item.path}
              type="button"
              className="lobe-turn-changed__chip"
              data-testid="turn-changed-file"
              data-path={item.path}
              title={item.path}
              aria-label={label}
              onClick={() => onOpenPath(item.path)}
            >
              {item.name}
            </button>
          );
        })}
        {hiddenCount > 0 && canViewAll ? (
          <button
            type="button"
            className="lobe-turn-changed__chip lobe-turn-changed__chip--more"
            data-testid="turn-changed-more"
            onClick={() => onViewAll()}
          >
            {tr("chat.changedFiles.more", { n: String(hiddenCount) })}
          </button>
        ) : hiddenCount > 0 ? (
          <span className="lobe-turn-changed__chip lobe-turn-changed__chip--static">
            {tr("chat.changedFiles.more", { n: String(hiddenCount) })}
          </span>
        ) : null}
      </div>
    </div>
  );
});
