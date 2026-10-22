import type { MessageKey } from "@/i18n";
import type { SourceGroupMember } from "@/lib/api/sessionMigrationGroups";

export const sourceGroupFormatKeys: Record<SourceGroupMember["format"], MessageKey> = {
  "cursor-transcript": "sessionImport.versions.formatTranscript",
  "cursor-composer": "sessionImport.versions.formatComposer",
  "cursor-legacy-chat": "sessionImport.versions.formatLegacy",
  "cursor-store": "sessionImport.versions.formatStore",
  artifact: "sessionImport.versions.formatArtifact",
};
