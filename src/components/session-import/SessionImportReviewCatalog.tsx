import { useState } from "react";
import type { ImportedWorkspaceFilter } from "@/lib/api/sessionWorkspaceAssociation";
import type { openImportedHistory } from "@/lib/sessionImport/open";
import { ImportedHistoryGroupContent } from "./ImportedHistoryGroupContent";
import { ImportedWorkspaceFilterContent } from "./ImportedWorkspaceFilter";
import type { SessionImportTFn } from "./sessionImportHostLabels";
import { catalogEntry, reviewCatalogEntries, root } from "./sessionImportReviewFixtures";
const group = {
  sourceWorkspaceId: "synthetic-workspace", displayName: "Synthetic workspace",
  provider: "codex", sourceId: "src-synthetic", sourceLocator: root.path, missing: false,
};

export function SessionImportReviewCatalog({ tr, onOpen, onContinue }: {
  tr: SessionImportTFn;
  onOpen: typeof openImportedHistory;
  onContinue: (id: string) => void;
}) {
  const [query, setQuery] = useState("");
  const [provider, setProvider] = useState<string | null>(null);
  const [sort, setSort] = useState<"newest" | "oldest">("newest");
  const [workspace, setWorkspace] = useState<ImportedWorkspaceFilter | null>(null);
  const [refreshVersion, setRefreshVersion] = useState(0);
  const refresh = async () => setRefreshVersion((value) => value + 1);
  const unavailable = async () => { throw new Error("Synthetic catalog has no additional page or live export"); };
  const visible = reviewCatalogEntries.filter((entry) =>
    (!provider || entry.provider === provider) &&
    (!workspace || (workspace.kind === "unassigned" ? entry !== catalogEntry : entry === catalogEntry)) &&
    `${entry.title} ${entry.provider}`.toLowerCase().includes(query.trim().toLowerCase()),
  ).sort((a, b) => sort === "newest" ? b.createdAt.localeCompare(a.createdAt) : a.createdAt.localeCompare(b.createdAt));
  return <div data-review-catalog-revision={refreshVersion}>
    <ImportedHistoryGroupContent tr={tr} variant="settings" catalog={{
      visible, query, setQuery, provider, setProvider, providers: ["codex", "claude-code"],
      sort, setSort, workspace, load: refresh, loadMore: unavailable, nextCursor: null,
      total: visible.length, busy: false, queryPending: false, error: null, windowed: false,
    }} exportMarkdown={unavailable} onOpen={onOpen} onContinue={onContinue}
      workspaceFilter={<ImportedWorkspaceFilterContent tr={tr} value={workspace} onChange={setWorkspace}
        groups={{ items: [group], selected: workspace?.kind === "workspace" ? group : null,
          cursor: null, busy: false, error: null, windowed: false, refresh, loadMore: unavailable }} />} />
  </div>;
}
