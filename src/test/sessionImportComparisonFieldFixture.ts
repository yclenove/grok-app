import type { SourceComparisonFieldPage } from "@/lib/api/sessionMigrationComparisonField";
import type { ComparisonFieldContext } from "@/lib/sessionImport/comparisonFieldState";
import { comparisonOwner, comparisonPage } from "./sessionImportComparisonFixture";

export function comparisonFieldContext(): ComparisonFieldContext {
  const owner = comparisonOwner();
  const event = comparisonPage(owner).rows[0].left!;
  return { owner, side: "left", position: 0, eventId: event.eventId, fieldsDigest: event.fieldsDigest };
}
export function comparisonFieldPage(context = comparisonFieldContext(), field = "kind", start = 0, span = 4): SourceComparisonFieldPage {
  const available = field !== "summary";
  const text = available ? JSON.stringify(field === "blocks" ? [{ kind: "text", text: "完整正文🚀" }] : "message") : "";
  const bytes = new TextEncoder().encode(text);
  let end = Math.min(start + span, bytes.length);
  while (end < bytes.length && (bytes[end] & 0xc0) === 0x80) --end;
  return { version: 1, left: { ...context.owner.left }, right: { ...context.owner.right },
    side: context.side, position: context.position, field, eventId: context.eventId, fieldsDigest: context.fieldsDigest,
    fieldDigest: available ? "a".repeat(64) : null, fieldNames: ["blocks", "complete", "kind", "role"],
    available, text: new TextDecoder().decode(bytes.slice(start, end)), startByte: start, endByte: end,
    totalBytes: bytes.length, nextCursor: end === bytes.length ? null : `field-next:${end}`, complete: end === bytes.length };
}
