/** Explicit read-only historical target inspection. No import, merge or grant. */
import { invoke } from "./host";
import type { PreviewSelection } from "@/lib/sessionImport/selection";
import type { WorkspaceMapping } from "./sessionMigration";
import type { PendingDurablePlan } from "@/lib/sessionImport/durablePlan";

export type IdentityAssociationConfirmation = {
  version: 1;
  importId: string;
  expectedRevision: string;
  previewId: string;
  reviewDigest: string;
};

export type PreparedAssociationRequest = {
  version: 1;
  batchId: string;
  expectedStateVersion: number;
  candidateId: string;
  previewId: string;
  importId: string;
  expectedRevision: string;
};

export type PreparedAssociationReview = {
  batchId: string;
  stateVersion: number;
  candidateId: string;
  leafId: string;
  review: IdentityAssociationReview;
};

export function sessionImportPreparedAssociationInspect(request: Readonly<PreparedAssociationRequest>) {
  return invoke<PreparedAssociationReview>("session_import_prepared_association_inspect", {
    request: { version: request.version, batchId: request.batchId,
      expectedStateVersion: request.expectedStateVersion, candidateId: request.candidateId,
      previewId: request.previewId, importId: request.importId, expectedRevision: request.expectedRevision },
  });
}

export function sessionImportPreparedAssociationPlan(request: {
  version: 1; batchId: string; expectedStateVersion: number;
  mappings: WorkspaceMapping[]; identityAssociations: IdentityAssociationConfirmation[];
}) {
  return invoke<PendingDurablePlan>("session_import_prepared_association_plan", {
    request: { version: request.version, batchId: request.batchId,
      expectedStateVersion: request.expectedStateVersion,
      mappings: request.mappings.map((mapping) => ({ ...mapping })),
      identityAssociations: request.identityAssociations.map((confirmation) => ({ ...confirmation })),
    },
  });
}

export type IdentityAssociationRequest = {
  version: 1;
  importId: string;
  expectedRevision: string;
  preview: PreviewSelection;
};

export type IdentityAssociationReview = {
  version: 1;
  importId: string;
  expectedRevision: string;
  previewId: string;
  sourceId: string;
  oldNativeSessionId: string;
  currentNativeSessionId: string;
  oldNormalizerVersion: string;
  currentNormalizerVersion: string;
  selectionHash: string;
  /** A binding summary, not a grant or a commit token. */
  reviewDigest: string;
};

export function sessionImportIdentityAssociationInspect(request: Readonly<IdentityAssociationRequest>) {
  const { preview } = request;
  const { selection, leafId, previewId } = preview;
  const { fields } = selection;
  // Freeze the complete declared wire scope before the async transport starts.
  // Preserve raw duplicates/order/opaque IDs for authoritative Host validation.
  return invoke<IdentityAssociationReview>("session_import_identity_association_inspect", {
    request: {
      version: request.version,
      importId: request.importId,
      expectedRevision: request.expectedRevision,
      preview: {
        previewId, leafId,
        selection: {
          version: selection.version,
          eventIds: [...selection.eventIds],
          pinnedEventIds: [...selection.pinnedEventIds],
          assetIds: [...selection.assetIds],
          fields: {
            messageText: fields.messageText,
            toolCallName: fields.toolCallName,
            toolCallInput: fields.toolCallInput,
            toolResultContent: fields.toolResultContent,
            compactionSummary: fields.compactionSummary,
            visibleReasoning: fields.visibleReasoning,
          },
          includeSubagents: selection.includeSubagents,
        },
      },
    },
  });
}
