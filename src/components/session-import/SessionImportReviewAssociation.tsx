import { useState } from "react";
import type { ImportedWorkspaceAssociation, ImportedWorkspaceChoice } from "@/lib/api/sessionWorkspaceAssociation";
import { ImportedWorkspaceAssociationContent } from "./ImportedWorkspaceAssociation";
import type { SessionImportTFn } from "./sessionImportHostLabels";
import { catalogEntry, root } from "./sessionImportReviewFixtures";

const choice: ImportedWorkspaceChoice = {
  sourceWorkspaceId: "synthetic-workspace", revision: "synthetic-workspace-revision",
  displayName: "Synthetic workspace", sourceLocator: root.path,
};

export function SessionImportReviewAssociation({ tr, importId = catalogEntry.importId }: {
  tr: SessionImportTFn; importId?: string;
}) {
  const [association, setAssociation] = useState<ImportedWorkspaceAssociation>({
    importId, version: 1, sourceId: "src-synthetic", provider: "codex",
    sourceReady: true, choice: null, status: "unassigned",
  });
  const [draft, setDraft] = useState<ImportedWorkspaceChoice | null>(null);
  const [saved, setSaved] = useState(false);
  return <ImportedWorkspaceAssociationContent tr={tr} ui={{
    association, draft, saved, items: [choice], cursor: null, busy: false, error: null, windowed: false,
    choose: (id) => { if (!id || id === choice.sourceWorkspaceId) { setDraft(id ? choice : null); setSaved(false); } },
    refresh: async () => { setDraft(association.choice); setSaved(false); },
    loadMore: async () => { throw new Error("Synthetic association has no additional page"); },
    save: async (clear = false) => {
      const next = clear ? null : draft;
      setAssociation((current) => ({ ...current, version: current.version + 1, choice: next, status: next ? "assigned" : "unassigned" }));
      setDraft(next); setSaved(true);
    },
  }} />;
}
