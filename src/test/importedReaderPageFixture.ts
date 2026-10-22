/** Enrich old compact success fixtures with the required IPC archive scope.
 * Explicit fields are never replaced, so mismatch tests still fail closed. */
export function importedReaderPageFixture<T extends object>(page: T, revision?: string | null, leaf?: string | null) {
  return { revision: revision ?? "rev-current", leaf: leaf ?? "leaf-current", ...page };
}
