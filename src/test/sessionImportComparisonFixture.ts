import type { SourceComparisonEvent, SourceComparisonPage, SourceComparisonRequest, SourceComparisonSide } from "@/lib/api/sessionMigrationComparison";
import type { SourceComparisonOwner } from "@/lib/sessionImport/sourceComparisonState";

export const comparisonScope = { scanId: "comparison-scan", epoch: 1, generation: 1 };
export function comparisonSide(member: string, leaf = "main"): SourceComparisonSide {
  return { previewId: `preview-${member}`, candidateId: `candidate:${comparisonScope.scanId}:${member}`,
    sourceRevision: `revision-${member}`, leafId: leaf };
}
export function comparisonOwner(): SourceComparisonOwner {
  return { scope: { ...comparisonScope }, groupId: "group:comparison-scan:shared-native",
    left: comparisonSide("A"), right: comparisonSide("B") };
}
export function comparisonPage(
  request: Pick<SourceComparisonRequest, "left" | "right"> = comparisonOwner(), start = 0,
): SourceComparisonPage {
  const event = (side: Readonly<SourceComparisonSide>, position: number): SourceComparisonEvent => ({
    eventId: `${side.previewId}:${side.leafId}:${position}`, kind: "message", timestamp: null,
    timeOrigin: "unknown", spans: [{ artifactId: side.candidateId, record: `line:${position + 1}` }],
    excerpt: `正文 ${position}`, truncated: false, fieldsDigest: position.toString(16).padStart(64, "0"),
  });
  const rows = Array.from({ length: Math.min(2, 3 - start) }, (_, index) => {
    const position = start + index;
    return { position, left: event(request.left, position), right: position < 2 ? event(request.right, position) : null,
      differences: position < 2 ? [] : ["missing-side"],
      metadataDifferences: position < 2 ? ["event-id" as const] : [], uncertain: position >= 2 };
  });
  const complete = start + rows.length === 3;
  return {
    version: 1, alignment: "position-only", equivalence: "unverified",
    left: { ...request.left, snapshotDigest: "snapshot-A", groupId: comparisonOwner().groupId,
      eventCount: 3, title: "A", cwd: null, outcome: "complete" },
    right: { ...request.right, snapshotDigest: "snapshot-B", groupId: comparisonOwner().groupId,
      eventCount: 2, title: "B", cwd: null, outcome: "complete" },
    metadataDifferences: ["title"], totalPositions: 3, rows,
    nextCursor: complete ? null : "signed-next:2", complete, uncertain: rows.some((row) => row.uncertain),
  };
}
export function comparisonDeferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (cause: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
