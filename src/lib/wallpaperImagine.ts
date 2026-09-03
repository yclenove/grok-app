import type { WallpaperGalleryItem } from "@/lib/wallpaperSource";

export type WallpaperImagineMode = "image" | "video";
export type WallpaperVideoDuration = 6 | 10;
export type WallpaperVideoResolution = "480p" | "720p";
export type WallpaperVideoSourceStatus =
  | "idle"
  | "preparing"
  | "ready"
  | "error";

export type WallpaperImagineControlsModel = {
  mode: WallpaperImagineMode;
  prompt: string;
  aspect: string;
  aspectOptions: Array<{ value: string; label: string }>;
  videoDuration: WallpaperVideoDuration;
  videoResolution: WallpaperVideoResolution;
  videoSource: WallpaperGalleryItem | null;
  videoSourcePath: string | null;
  videoSourcePreview: string | null;
  videoSourceStatus: WallpaperVideoSourceStatus;
  generating: boolean;
  cancelling: boolean;
  onModeChange: (value: WallpaperImagineMode) => void;
  onPromptChange: (value: string) => void;
  onAspectChange: (value: string) => void;
  onVideoDurationChange: (value: WallpaperVideoDuration) => void;
  onVideoResolutionChange: (value: WallpaperVideoResolution) => void;
  onClearVideoSource: () => void;
  onGenerate: () => void;
  onCancelGeneration: () => void;
};

export const WALLPAPER_VIDEO_DURATIONS: readonly WallpaperVideoDuration[] = [
  6, 10,
] as const;

export const WALLPAPER_VIDEO_RESOLUTIONS: readonly WallpaperVideoResolution[] = [
  "480p",
  "720p",
] as const;

export function isWallpaperImageItem(
  item: Pick<WallpaperGalleryItem, "kind">,
): boolean {
  const kind = item.kind.trim().toLowerCase();
  return kind !== "video" && !kind.startsWith("video/");
}
