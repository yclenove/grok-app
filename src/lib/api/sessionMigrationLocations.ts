/** Source registration and explicit associated-root metadata; no relocation fallback. */
import { invoke } from "./host";
import type { ImportProvider, ScanSource } from "./sessionMigration";

export type SourceLocation = {
  version: 1; locationId: string; sourceId: string; provider: ImportProvider;
  host: string; rootPath: string; kind: "primary" | "associated";
};
export type SourceLocationPage = {
  locations: SourceLocation[]; nextCursor: string | null; totalCount: number;
};
export type SourceRegistration = {
  sourceId: string; provider: ImportProvider; rootPath: string; profile: string; location?: SourceLocation;
};
export type SourceAssociationRequest = {
  sourceId: string; provider: ImportProvider; host: string; path: string; expectedPrimaryRoot: string;
};

export function sessionImportSources() {
  return invoke<SourceRegistration[]>("session_import_sources");
}

export function sessionImportSourceRegister(source: ScanSource) {
  return invoke<SourceRegistration>("session_import_source_register", { source });
}

export function sessionImportSourceLocations(
  sourceId: string,
  provider: ImportProvider,
  options?: { host?: string; cursor?: string; limit?: number },
) {
  return invoke<SourceLocationPage>("session_import_source_locations", {
    sourceId, provider, ...options,
  });
}

export function sessionImportSourceAssociate(request: SourceAssociationRequest) {
  return invoke<SourceLocation>("session_import_source_associate", { request });
}
