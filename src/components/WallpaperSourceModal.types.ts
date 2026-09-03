import type { MessageKey } from "@/i18n";
import type { WallpaperSourceKind } from "@/lib/wallpaperSource";
import type { WallpaperXSearchMode } from "@/lib/wallpaperXSearch";

export type WallpaperSourceTab = WallpaperSourceKind;

export type WallpaperSourceModalProps = {
  open: boolean;
  onClose: () => void;
  initialTab?: WallpaperSourceTab;
  t: (
    key: MessageKey,
    vars?: Record<string, string | number | undefined | null>,
  ) => string;
  /** Apply prepared File via parent (prepareWallpaperFromFile + onWallpaper). */
  onPickFile: (file: File) => void | Promise<void>;
  /** Jump to Account settings when login is required. */
  onRequestLogin?: () => void;
  /** Persist the X-only route selected inside the X source workspace. */
  wallpaperXSearchMode?: WallpaperXSearchMode;
  onWallpaperXSearchMode?: (
    value: WallpaperXSearchMode,
  ) => void | Promise<void>;
};
