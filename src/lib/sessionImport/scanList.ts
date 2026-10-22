import type { SourceCandidate } from "@/lib/api/sessionMigration";

/** Extra discoveries after a frozen select-all, excluding the current page. */
export function extrasNotOnPage(
  page: SourceCandidate[],
  extras: SourceCandidate[],
): SourceCandidate[] {
  const onPage = new Set(page.map((c) => c.candidateId));
  return extras.filter((c) => !onPage.has(c.candidateId));
}
