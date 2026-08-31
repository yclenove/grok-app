import { GlassModal } from "@/components/GlassModal";
import type { MessageKey } from "@/i18n";
import type { WallpaperGalleryItem } from "@/lib/wallpaperSource";

type Translate = (
  key: MessageKey,
  vars?: Record<string, string | number | undefined | null>,
) => string;

export type WallpaperLibraryDeleteDialogProps = {
  t: Translate;
  item: WallpaperGalleryItem | null;
  onClose: () => void;
  onConfirm: (item: WallpaperGalleryItem) => void;
};

export function WallpaperLibraryDeleteDialog({
  t,
  item,
  onClose,
  onConfirm,
}: WallpaperLibraryDeleteDialogProps) {
  return (
    <GlassModal
      open={!!item}
      onClose={onClose}
      title={t("wallpaper.library.deleteConfirmTitle")}
      size="sm"
      closeLabel={t("common.close")}
      footer={
        <>
          <button type="button" className="btn btn--ghost" onClick={onClose}>
            {t("common.cancel")}
          </button>
          <button
            type="button"
            className="btn btn--solid btn--danger"
            data-testid="wallpaper-library-delete-confirm"
            onClick={() => {
              if (item) onConfirm(item);
            }}
          >
            {t("wallpaper.library.deleteConfirmAction")}
          </button>
        </>
      }
    >
      <p className="rp-modal-copy">{t("wallpaper.library.deleteConfirm")}</p>
    </GlassModal>
  );
}
