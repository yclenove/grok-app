import type { MessageKey, Vars } from "@/i18n";
import { useImportedCatalog } from "@/hooks/useImportedCatalog";
import { ImportedHistoryGroupContent } from "./ImportedHistoryGroupContent";
import { ImportedWorkspaceFilter } from "./ImportedWorkspaceFilter";

export function ImportedHistoryGroup({ tr, variant = "sidebar" }: {
  tr: (key: MessageKey, vars?: Vars) => string;
  variant?: "sidebar" | "settings";
}) {
  const catalog = useImportedCatalog(variant);
  return <ImportedHistoryGroupContent tr={tr} variant={variant} catalog={catalog}
    workspaceFilter={<ImportedWorkspaceFilter tr={tr} value={catalog.workspace} onChange={catalog.setWorkspace} />} />;
}
