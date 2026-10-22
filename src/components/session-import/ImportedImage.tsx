import { useEffect, useRef, useState } from "react";
import type { MessageKey, Vars } from "@/i18n";
import { useImageViewerOptional } from "@/components/ImageViewerContext";
import type { ImportedImages } from "@/hooks/useImportedImages";

type TFn = (key: MessageKey, vars?: Vars) => string;
const stateKeys = {
  missing: "sessionImport.mediaMissing", unresolved: "sessionImport.mediaUnresolved",
  excluded: "sessionImport.mediaExcluded", oversize: "sessionImport.mediaOversize",
  unsupported: "sessionImport.mediaUnsupported", invalid: "sessionImport.mediaInvalid",
  error: "sessionImport.mediaInvalid", loading: "sessionImport.peekLoading",
} satisfies Record<string, MessageKey>;

export function ImportedImage({ assetId, alt, images, tr }: {
  assetId: string; alt?: string; images?: ImportedImages; tr: TFn;
}) {
  const viewer = useImageViewerOptional();
  const owner = useRef(Symbol("import-image"));
  const state = images?.states[assetId] ?? { state: "loading" as const };
  const [failedUrl, setFailedUrl] = useState<string | null>(null);
  useEffect(() => {
    const claim = owner.current;
    return () => viewer.close(claim);
  }, [viewer, images?.identity, assetId]);
  const stored = state.state === "stored" ? state : null;
  const caption = alt && alt.length <= 160 && !/[\\/]|\b(?:data|opaque|file):/i.test(alt)
    ? alt : tr("sessionImport.attachments");
  const failed = !!stored?.thumbnail && failedUrl === stored.thumbnail;
  if (stored?.thumbnail && !failed) return <figure className="session-import__image" data-asset-state="stored">
    <button type="button" className="session-import__image-open" aria-label={tr("image.view")}
      onClick={(event) => {
        event.stopPropagation();
        viewer.open([{ src: stored.thumbnail!, alt: caption,
          loadOriginal: async (signal) => signal.aborted || !stored.original ? null : { src: stored.original },
          originalErrorMessage: () => tr("image.originalFailed"),
        }], 0, owner.current);
      }}>
      <img src={stored.thumbnail} alt={caption} loading="lazy" decoding="async"
        width={stored.image?.width} height={stored.image?.height} onError={() => setFailedUrl(stored.thumbnail!)} />
    </button>
  </figure>;
  const kind = failed ? "error" : state.state === "stored" ? "invalid" : state.state;
  return <div className="session-import__image session-import__image-status" data-asset-state={kind}
    role={kind === "loading" ? "status" : undefined}>
    <span>{tr(stateKeys[kind])}</span>
    {(kind === "invalid" || kind === "error") && images ? <button type="button" className="btn btn--sm btn--ghost"
      onClick={(event) => { event.stopPropagation(); setFailedUrl(null); images.retry(assetId); }}>
      {tr("sessionImport.comparison.retryRead")}
    </button> : null}
  </div>;
}
