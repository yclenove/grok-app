import type { CatalogQuery } from "@/lib/api/sessionMigration";

export const IMPORTED_LIST_WINDOW = 200;
export type CatalogView = {
  query: string;
  provider: string | null;
  sort: "newest" | "oldest";
  workspace: NonNullable<CatalogQuery["workspace"]> | null;
};
const defaults: CatalogView = { query: "", provider: null, sort: "newest", workspace: null };
const key = (scope: string) => `grok:imported-catalog-view:v1:${scope}`;

export function readCatalogView(scope?: string): CatalogView {
  if (!scope) return { ...defaults };
  try {
    const value = JSON.parse(sessionStorage.getItem(key(scope)) ?? "null");
    if (!value || typeof value.query !== "string" || value.query.length > 2048
      || (value.provider != null && (typeof value.provider !== "string" || value.provider.length > 64))
      || !["newest", "oldest"].includes(value.sort)) return { ...defaults };
    const workspace = value.workspace;
    if (workspace != null && workspace.kind !== "unassigned"
      && !(workspace.kind === "workspace" && typeof workspace.sourceWorkspaceId === "string"
        && workspace.sourceWorkspaceId.length > 0 && workspace.sourceWorkspaceId.length <= 256)) return { ...defaults };
    return { query: value.query, provider: value.provider ?? null, sort: value.sort,
      workspace: workspace == null ? null : workspace.kind === "unassigned" ? { kind: "unassigned" }
        : { kind: "workspace", sourceWorkspaceId: workspace.sourceWorkspaceId } };
  } catch { return { ...defaults }; }
}

export function writeCatalogView(scope: string | undefined, view: CatalogView) {
  if (!scope) return;
  try { sessionStorage.setItem(key(scope), JSON.stringify(view)); } catch { /* Browsing still works with storage unavailable. */ }
}

export function appendImportedWindow<T>(old: T[], page: T[], id: (value: T) => string): { items: T[]; trimmed: boolean } {
  const merged = new Map(old.map((value) => [id(value), value]));
  for (const value of page) merged.set(id(value), value);
  const values = [...merged.values()];
  return { items: values.slice(-IMPORTED_LIST_WINDOW), trimmed: values.length > IMPORTED_LIST_WINDOW };
}
