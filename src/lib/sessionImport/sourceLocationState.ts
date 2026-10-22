import type { SourceLocation, SourceLocationPage, SourceRegistration } from "@/lib/api/sessionMigrationLocations";

export const LOCATION_PAGE_LIMIT = 50;
export const LOCATION_SELECTION_LIMIT = 200;
const text = (value: unknown): value is string => typeof value === "string" && value.length > 0 && value.length <= 8192;
const failure = (detail: string): never => { throw new Error(`SOURCE_CHANGED: ${detail}`); };

/** Validate wire identity, not folder-name similarity or inferred installation ownership. */
export function validateSourceLocation(source: SourceRegistration, location: SourceLocation) {
  if (!location || location.version !== 1 || !text(location.locationId) || !text(location.rootPath) ||
    location.sourceId !== source.sourceId || location.provider !== source.provider || location.host !== "local" ||
    !["primary", "associated"].includes(location.kind)) failure("foreign source location");
  if (location.kind === "primary" && (location.locationId !== `primary:${source.sourceId}` ||
    location.rootPath !== source.rootPath)) failure("primary source location changed");
  if (location.kind === "associated" && (source.provider !== "cursor" ||
    !/^[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}$/i.test(location.locationId))) failure("invalid associated location identity");
}

export function validateSourceLocationPage(source: SourceRegistration, page: SourceLocationPage) {
  if (!page || !Array.isArray(page.locations) || page.locations.length > LOCATION_PAGE_LIMIT ||
    !Number.isSafeInteger(page.totalCount) || page.totalCount < 1 || page.locations.length > page.totalCount ||
    (page.nextCursor !== null && !text(page.nextCursor)) || (!page.locations.length && page.nextCursor)) {
    failure("invalid source location page");
  }
  const ids = new Set<string>();
  for (const location of page.locations) {
    validateSourceLocation(source, location);
    if (ids.has(location.locationId)) failure("duplicate source location");
    ids.add(location.locationId);
  }
}
