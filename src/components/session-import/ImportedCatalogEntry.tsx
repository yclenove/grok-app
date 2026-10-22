import { useState } from "react";
import { GlassModal } from "@/components/GlassModal";
import { useImportedCatalog } from "@/hooks/useImportedCatalog";
import type { SessionImportTFn } from "./sessionImportHostLabels";
import { ImportedHistoryGroup } from "./ImportedHistoryGroup";

/**
 * Settings keeps a count and an explicit catalog. The full list mounts only
 * while that surface is open, so Language is not pushed below every history.
 */
export function ImportedCatalogEntry({ tr }: { tr: SessionImportTFn }) {
  const catalog = useImportedCatalog("settings");
  const [open, setOpen] = useState(false);
  return (
    <div className="settings-row" id="settings-anchor-importedCatalog">
      <div className="settings-row__text">
        <div className="settings-row__label">{tr("sessionImport.importedGroup")}</div>
        <div className="settings-row__desc">
          {catalog.error
            ? tr("sessionImport.catalogError")
            : tr("sessionImport.catalogCompact", { n: catalog.total })}
        </div>
      </div>
      <button type="button" className="btn" onClick={() => setOpen(true)}>
        {tr("sessionImport.catalogOpen")}
      </button>
      <GlassModal
        open={open}
        onClose={() => setOpen(false)}
        title={tr("sessionImport.importedGroup")}
        size="lg"
        wrapBody
        className="session-import-modal session-import-catalog-modal"
        closeLabel={tr("sessionImport.close")}
      >
        {open ? <ImportedHistoryGroup tr={tr} variant="settings" /> : null}
      </GlassModal>
    </div>
  );
}
