/** A reader window, its cursors and derived actions share one immutable archive. */
export type ReaderScope = { revision: string; leaf: string };

export function acceptReaderScope(
  requested: { revision: string | null; leaf: string | null },
  pinned: ReaderScope | null,
  response: { revision: string; leaf: string },
): ReaderScope {
  const { revision, leaf } = response;
  if (
    typeof revision !== "string" || !revision.trim() ||
    typeof leaf !== "string" || !leaf.trim() ||
    (requested.revision != null && requested.revision !== revision) ||
    (requested.leaf != null && requested.leaf !== leaf) ||
    (pinned != null && (pinned.revision !== revision || pinned.leaf !== leaf))
  ) {
    // Match the host's error-code protocol; never silently mix archive windows.
    throw "PLAN_STALE: reader snapshot does not match the requested archive";
  }
  return pinned ?? { revision, leaf };
}
