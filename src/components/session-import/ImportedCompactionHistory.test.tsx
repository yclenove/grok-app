/** @vitest-environment jsdom */
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { createT } from "@/i18n";
import { OPEN_IMPORTED_EVENT, type OpenImportedDetail } from "@/lib/sessionImport/open";
import { compactionHistoryLeaf } from "@/lib/sessionImport/eventView";
import { ImportedCompactionHistory } from "./ImportedCompactionHistory";

const tr = createT("en");
const importId = "imp:00000000-0000-4000-8000-000000000001";
const compaction = { kind: "compaction", id: "c", summary: [], covered_event_ids: ["u1", "a1", "u2"], coverage_known: true };
afterEach(cleanup);

function opened() {
  const seen: OpenImportedDetail[] = [];
  const listener = vi.fn((ev: Event) => seen.push((ev as CustomEvent<OpenImportedDetail>).detail));
  window.addEventListener(OPEN_IMPORTED_EVENT, listener);
  return { seen, stop: () => window.removeEventListener(OPEN_IMPORTED_EVENT, listener) };
}

test("the last covered event is the pre-compaction leaf", () => {
  expect(compactionHistoryLeaf([{ kind: "message", id: "m" }, compaction])).toEqual({ leaf: "u2", count: 3 });
  expect(compactionHistoryLeaf([{ kind: "compaction", covered_event_ids: [] }])).toBeNull();
  expect(compactionHistoryLeaf([{ kind: "compaction", covered_event_ids: [7, ""] }])).toBeNull();
  expect(compactionHistoryLeaf([{ kind: "message", covered_event_ids: ["x"] }])).toBeNull();
});

test("opens the pre-compaction leaf of the same revision", () => {
  const events = opened();
  render(<ImportedCompactionHistory importId={importId} revision="rev:1" leaf={null} events={[compaction]} busy={false} tr={tr} />);
  expect(screen.queryByRole("button", { name: "Back to the latest conversation" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "View the conversation before compaction (3 events)" }));
  events.stop();
  expect(events.seen).toEqual([{ importId, revision: "rev:1", leaf: "u2" }]);
});

test("an explicit leaf can return to the latest conversation", () => {
  const events = opened();
  render(<ImportedCompactionHistory importId={importId} revision="rev:1" leaf="u2" events={[{ kind: "message", id: "u2" }]} busy={false} tr={tr} />);
  fireEvent.click(screen.getByRole("button", { name: "Back to the latest conversation" }));
  events.stop();
  expect(events.seen).toEqual([{ importId, revision: "rev:1" }]);
});

test("nothing renders without a compaction, an explicit leaf or a resolved revision", () => {
  const { container, rerender } = render(<ImportedCompactionHistory importId={importId} revision="rev:1" leaf={null} events={[{ kind: "message" }]} busy={false} tr={tr} />);
  expect(container.innerHTML).toBe("");
  rerender(<ImportedCompactionHistory importId={importId} revision={null} leaf="u2" events={[compaction]} busy={false} tr={tr} />);
  expect(container.innerHTML).toBe("");
});

test("busy readers disable both controls", () => {
  render(<ImportedCompactionHistory importId={importId} revision="rev:1" leaf="x" events={[compaction]} busy tr={tr} />);
  for (const button of screen.getAllByRole("button")) expect((button as HTMLButtonElement).disabled).toBe(true);
});
