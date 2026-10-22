import { expect, test } from "vitest";
import {
  DISPLAY_TEXT_CAP,
  boundedTextEnd,
  eventHasIssue,
  eventPlainText,
  importedEventDisplay,
  importedEventId,
} from "./eventView";

test("reads flatten id, eventId, and nested base.id", () => {
  expect(importedEventId({ id: "a" })).toBe("a");
  expect(importedEventId({ eventId: "b" })).toBe("b");
  expect(importedEventId({ base: { id: "c" } })).toBe("c");
  expect(importedEventId(null)).toBeNull();
});

test("keeps assets separate from incomplete source content and system text", () => {
  const mixed = importedEventDisplay({
    kind: "unsupported",
    sourceType: "weird-block",
    base: { id: "u1" },
    role: "system",
    complete: false,
    blocks: [
      { kind: "asset", assetId: "blob-9", alt: "shot.png" },
      { kind: "opaque", sourceType: "binary" },
      { kind: "text", text: "[[chat:other]] do not expand" },
    ],
  });
  expect(mixed.id).toBe("u1");
  expect(mixed.flags.unsupported).toBe(true);
  expect(mixed.flags.incomplete).toBe(true);
  expect(mixed.flags.truncated).toBe(false);
  expect(mixed.flags.system).toBe(true);
  expect(mixed.parts.some((p) => p.kind === "unsupported" && p.sourceType === "weird-block")).toBe(
    true,
  );
  expect(mixed.parts.some((p) => p.kind === "asset" && p.alt === "shot.png")).toBe(true);
  expect(mixed.parts.some((p) => p.kind === "unsupported" && p.sourceType === "binary")).toBe(true);
  const sys = mixed.parts.find((p) => p.kind === "system");
  expect(sys && sys.kind === "system" && sys.text).toContain("[[chat:other]]");
  expect(mixed.parts.some((p) => p.kind === "incomplete")).toBe(true);
});

test("caps oversized visible text without reporting an incomplete source", () => {
  const view = importedEventDisplay({
    id: "big",
    kind: "message",
    role: "assistant",
    complete: true,
    blocks: [{ kind: "text", text: "x".repeat(DISPLAY_TEXT_CAP + 80) }],
  });
  expect(view.flags.truncated).toBe(false);
  expect(view.flags.displayClipped).toBe(true);
  expect(eventHasIssue(view)).toBe(false);
  const text = view.parts.find((p) => p.kind === "text");
  expect(text && text.kind === "text" && text.text.length).toBe(DISPLAY_TEXT_CAP);
  expect(text && text.kind === "text" && text.fullText?.length).toBe(DISPLAY_TEXT_CAP + 80);
  expect(view.parts.some((part) => part.kind === "truncated")).toBe(false);
});

test("normalizes the snake_case fields emitted by Rust without losing historical camelCase", () => {
  const view = importedEventDisplay({
    id: "rust-wire", kind: "unsupported", source_type: "rust-event",
    blocks: [{ kind: "asset", asset_id: "rust-asset" }, { kind: "opaque", source_type: "rust-block" }],
  });
  expect(view.parts).toContainEqual({ kind: "unsupported", sourceType: "rust-event" });
  expect(view.parts).toContainEqual({ kind: "asset", assetId: "rust-asset", alt: undefined });
  expect(view.parts).toContainEqual({ kind: "unsupported", sourceType: "rust-block" });
  const legacy = importedEventDisplay({
    kind: "unsupported", sourceType: "legacy", blocks: [{ kind: "asset", assetId: "legacy-asset" }],
  });
  expect(legacy.parts).toContainEqual({ kind: "unsupported", sourceType: "legacy" });
  expect(legacy.parts).toContainEqual({ kind: "asset", assetId: "legacy-asset", alt: undefined });
});

test("retains tool names and JSON arguments as inert bounded text", () => {
  const value = { command: "<script>never run</script>", cwd: "C:/workspace", timeout: 30 };
  const view = importedEventDisplay({
    id: "call", kind: "tool-call", call_id: "tool-1", name: "exec_command",
    input: { encoding: "json", value },
  });
  expect(eventPlainText(view)).toBe(`exec_command\n${JSON.stringify(value, null, 2)}`);
  expect(view.flags.truncated).toBe(false);
  expect(eventHasIssue(view)).toBe(false);
  const oversized = importedEventDisplay({
    kind: "tool-call", name: "shell", input: { encoding: "json", value: { command: "x".repeat(6000) } },
  });
  expect(oversized.flags.displayClipped).toBe(true);
  expect(eventPlainText(oversized).length).toBeLessThanOrEqual(DISPLAY_TEXT_CAP + "shell\n".length);
});

test("identifies payload-only tool inputs without reading or inventing their contents", () => {
  const view = importedEventDisplay({
    kind: "tool-call", name: "shell",
    input: { encoding: "payload", payload: { blobId: "blob-7", mediaType: "application/json", byteLength: 20 } },
  });
  expect(view.parts).toContainEqual({ kind: "tool-input-payload", blobId: "blob-7" });
  expect(eventPlainText(view)).toBe("shell");
});

test("long complete text pages preserve surrogate pairs while real source truncation remains an issue", () => {
  const content = "x".repeat(DISPLAY_TEXT_CAP - 1) + "🙂" + "tail";
  const first = boundedTextEnd(content);
  expect(first).toBe(DISPLAY_TEXT_CAP - 1);
  expect(content.slice(first, boundedTextEnd(content, first))).toBe("🙂tail");
  expect(content.slice(0, first) + content.slice(first, boundedTextEnd(content, first))).toBe(content);
  const incomplete = importedEventDisplay({ kind: "message", complete: false, text: content });
  expect(incomplete.flags.displayClipped).toBe(true);
  expect(incomplete.flags.truncated).toBe(true);
  expect(eventHasIssue(incomplete)).toBe(true);
  expect(incomplete.parts).toContainEqual({ kind: "truncated" });
});

test("plain text copy skips placeholders", () => {
  const view = importedEventDisplay({
    id: "m1",
    kind: "message",
    role: "user",
    blocks: [
      { kind: "text", text: "hello" },
      { kind: "asset", assetId: "x" },
    ],
  });
  expect(eventPlainText(view)).toBe("hello");
});

test("eventHasIssue is true for truncated or missing assets", () => {
  expect(
    eventHasIssue(
      importedEventDisplay({
        id: "m1",
        kind: "message",
        role: "user",
        complete: true,
        blocks: [{ kind: "text", text: "ok" }],
      }),
    ),
  ).toBe(false);
  expect(
    eventHasIssue(
      importedEventDisplay({
        id: "m2",
        kind: "message",
        role: "assistant",
        complete: false,
        blocks: [{ kind: "asset", assetId: "x" }],
      }),
    ),
  ).toBe(true);
});

test("only resolved attachment failures enter the issue filter", () => {
  const view = importedEventDisplay({ kind: "message", complete: true,
    blocks: [{ kind: "asset", asset_id: "photo" }] });
  for (const state of [undefined, "loading", "stored"]) {
    expect(eventHasIssue(view, state ? { photo: { state } } : {})).toBe(false);
  }
  for (const state of ["missing", "unresolved", "excluded", "oversize", "unsupported", "invalid", "error"]) {
    expect(eventHasIssue(view, { photo: { state } })).toBe(true);
  }
});

test("preview uses bounded ordered parts and pure images never invent flattened prose", () => {
  const view = importedEventDisplay({ eventId: "mixed", kind: "message", role: "user", text: "flattened wrong order",
    displayParts: [{ kind: "text", text: "before" }, { kind: "asset", asset_id: "photo" }, { kind: "text", text: "after" }],
    displayLimited: true, truncated: true });
  expect(view.parts.map((part) => part.kind)).toEqual(["text", "asset", "text"]);
  expect(eventPlainText(view)).toBe("before\nafter");
  expect(view.flags.truncated).toBe(false);
  expect(eventHasIssue(view, { photo: { state: "stored" } })).toBe(false);
  const pure = importedEventDisplay({ text: "DO_NOT_DUPLICATE_ALT", displayParts: [{ kind: "asset", asset_id: "photo" }] });
  expect(eventPlainText(pure)).toBe("");
});
