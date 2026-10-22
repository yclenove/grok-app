import type {
  ImportProvider,
  ScanSource,
  SourceRegistration,
  SourceLocation,
} from "@/lib/api/sessionMigration";
import { LOCATION_SELECTION_LIMIT, validateSourceLocation } from "./sourceLocationState";

export type SourceKind =
  | "default"
  | "custom-folder"
  | "custom-file"
  | "registered"
  | "relocate";

export type SourceChoice = {
  kind: SourceKind;
  provider: string;
  path: string;
  sourceId?: string;
  locations?: SourceLocation[];
};

/** `undefined` means scan every default root. A specific choice is one source. */
export function scanSourcesForChoice(choice: SourceChoice | null): ScanSource[] | undefined {
  if (!choice) return undefined;
  if (choice.locations) {
    if (!choice.sourceId || !choice.locations.length || choice.locations.length > LOCATION_SELECTION_LIMIT) throw new Error("RESOURCE_LIMIT: invalid history root selection");
    const source = { sourceId: choice.sourceId, provider: choice.provider as ImportProvider,
      rootPath: choice.path, profile: "" };
    return choice.locations.map((location) => {
      validateSourceLocation(source, location);
      return { provider: location.provider, path: location.rootPath, sourceId: location.sourceId,
        locationId: location.locationId, host: location.host };
    });
  }
  const source: ScanSource = { provider: choice.provider, path: choice.path };
  if (choice.kind === "registered" || choice.kind === "relocate") {
    if (choice.sourceId) source.sourceId = choice.sourceId;
  }
  return [source];
}

export function choiceFromDefaultRoot(provider: string, path: string): SourceChoice {
  return { kind: "default", provider, path };
}

export function choiceFromCustomFolder(
  provider: ImportProvider,
  path: string,
): SourceChoice {
  return { kind: "custom-folder", provider, path };
}

export function choiceFromCustomFile(path: string): SourceChoice {
  return { kind: "custom-file", provider: "generic-file", path };
}

export function choiceFromRegistered(reg: SourceRegistration): SourceChoice {
  return {
    kind: "registered",
    provider: reg.provider,
    path: reg.rootPath,
    sourceId: reg.sourceId,
  };
}

export function choiceFromLocations(reg: SourceRegistration, locations: SourceLocation[]): SourceChoice {
  if (!locations.length || locations.length > LOCATION_SELECTION_LIMIT) throw new Error("RESOURCE_LIMIT: invalid history root selection");
  if (new Set(locations.map((location) => location.locationId)).size !== locations.length) throw new Error("SOURCE_CHANGED: duplicate selected root");
  for (const location of locations) validateSourceLocation(reg, location);
  return { ...choiceFromRegistered(reg), locations: locations.map((location) => ({ ...location })) };
}

export function choiceFromRelocate(
  reg: Pick<SourceRegistration, "provider" | "sourceId">,
  newPath: string,
): SourceChoice {
  return {
    kind: "relocate",
    provider: reg.provider,
    path: newPath,
    sourceId: reg.sourceId,
  };
}

export function sameSourceChoice(a: SourceChoice | null, b: SourceChoice): boolean {
  return (
    !!a &&
    a.kind === b.kind &&
    a.provider === b.provider &&
    a.path === b.path &&
    (a.sourceId ?? "") === (b.sourceId ?? "") &&
    JSON.stringify(a.locations ?? null) === JSON.stringify(b.locations ?? null)
  );
}

export function kindLabelKey(kind: SourceKind):
  | "sessionImport.kind.default"
  | "sessionImport.kind.folder"
  | "sessionImport.kind.file"
  | "sessionImport.kind.registered"
  | "sessionImport.kind.relocate" {
  switch (kind) {
    case "custom-folder":
      return "sessionImport.kind.folder";
    case "custom-file":
      return "sessionImport.kind.file";
    case "registered":
      return "sessionImport.kind.registered";
    case "relocate":
      return "sessionImport.kind.relocate";
    default:
      return "sessionImport.kind.default";
  }
}
