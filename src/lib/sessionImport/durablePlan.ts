/** Durable handles and bounded review summaries. Catalog on disk remains truth. */

export type PendingDurablePlan = {
  planId: string;
  planHash: string;
  requestId?: string;
  previewCount?: number;
  associationTargets?: import("@/lib/api/sessionMigrationIdentityAssociation").IdentityAssociationConfirmation[];
};

export type RecoverySnapshot = {
  plan: PendingDurablePlan | null;
  job: unknown | null;
  draft: unknown | null;
  expired: boolean;
  failed: string | null;
};
