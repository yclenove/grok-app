import { useEffect, useMemo, useState } from "react";
import {
  forgetRemoteWallpaperThumbnail,
  peekRemoteWallpaperThumbnail,
  resolveRemoteWallpaperThumbnail,
  subscribeRemoteWallpaperThumbnail,
} from "@/lib/remoteWallpaperThumbnail";
import type { WallpaperGalleryItem } from "@/lib/wallpaperSource";

export type RemoteWallpaperThumbnailProps = {
  item: WallpaperGalleryItem;
  alt: string;
  onUnavailable: (id: string) => void;
};

export function RemoteWallpaperThumbnail({
  item,
  alt,
  onUnavailable,
}: RemoteWallpaperThumbnailProps) {
  const input = useMemo(
    () => ({
      source: item.source,
      thumbUrl: item.thumbUrl,
      fullUrl: item.fullUrl,
    }),
    [item.fullUrl, item.source, item.thumbUrl],
  );
  const directUrl = item.thumbUrl || item.fullUrl;
  const initialFallback = peekRemoteWallpaperThumbnail(input);
  const [directFailed, setDirectFailed] = useState(Boolean(initialFallback));
  const [fallbackSrc, setFallbackSrc] = useState<string | null>(initialFallback);
  const aspectRatio = useMemo(() => {
    const width = Number(item.width);
    const height = Number(item.height);
    return Number.isFinite(width) &&
      Number.isFinite(height) &&
      width > 0 &&
      height > 0
      ? `${width} / ${height}`
      : "16 / 9";
  }, [item.height, item.width]);

  useEffect(() => {
    const update = () => setFallbackSrc(peekRemoteWallpaperThumbnail(input));
    const cached = peekRemoteWallpaperThumbnail(input);
    setFallbackSrc(cached);
    setDirectFailed(Boolean(cached));
    return subscribeRemoteWallpaperThumbnail(input, update);
  }, [input]);

  useEffect(() => {
    if (!directFailed || fallbackSrc) return;
    let cancelled = false;
    void resolveRemoteWallpaperThumbnail(input).then((next) => {
      if (cancelled) return;
      if (next) setFallbackSrc(next);
      else onUnavailable(item.id);
    });
    return () => {
      cancelled = true;
    };
  }, [directFailed, fallbackSrc, input, item.id, onUnavailable]);

  if (!directFailed) {
    return (
      <img
        src={directUrl}
        alt={alt}
        className="wallpaper-masonry__img"
        loading="lazy"
        referrerPolicy="no-referrer"
        onError={() => setDirectFailed(true)}
      />
    );
  }

  if (!fallbackSrc) {
    return (
      <span
        className="wallpaper-masonry__thumb-placeholder"
        data-state="loading"
        style={{ aspectRatio }}
        aria-hidden="true"
      />
    );
  }

  return (
    <img
      src={fallbackSrc}
      alt={alt}
      className="wallpaper-masonry__img"
      loading="lazy"
      data-host-thumbnail="true"
      onError={() => {
        forgetRemoteWallpaperThumbnail(input);
        onUnavailable(item.id);
      }}
    />
  );
}
