/** @vitest-environment jsdom */
import type { ReactNode } from "react";
import { cleanup, render } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { useImportedReader } from "@/hooks/useImportedReader";
import { MAX_READER_EVENTS } from "@/lib/sessionImport/readerBudget";
import { MAX_RENDERED_EVENTS } from "@/lib/sessionImport/readerWindow";
import { SessionImportHost } from "./SessionImportHost";

const mocks = vi.hoisted(() => ({ reader: null as ReturnType<typeof useImportedReader> | null }));
vi.mock("@/hooks/useImportedReader", () => ({ useImportedReader: () => mocks.reader }));
vi.mock("@/components/GlassModal", () => ({ GlassModal: ({ open, children, footer }: {
  open: boolean; children: ReactNode; footer?: ReactNode;
}) => open ? <div role="dialog">{children}{footer}</div> : null }));
vi.mock("@/hooks/useSessionImport", () => ({ useSessionImportUi: () => ({
  open: false, step: "source", busy: false, readerId: "imp:scale-reader",
  readerRevision: null, readerLeaf: null,
  candidates: [], mappings: [], selected: new Set(), selectedCount: 0,
  prepareItems: [], prepareBatch: null,
}) }));
vi.mock("@/hooks/useImportedCatalog", () => ({ useImportedCatalog: () => ({ entries: [] }) }));
vi.mock("@/hooks/useImportedEntry", () => ({ useImportedEntry: () => ({ entry: null, busy: false, error: false }) }));
vi.mock("@/hooks/useSessionImportContentStates", () => ({ useSessionImportContentStates: () => ({ ensureSelected: () => {} }) }));
vi.mock("./ImportedWorkspaceAssociation", () => ({ ImportedWorkspaceAssociation: () => null }));
vi.mock("./SessionImportContinuationModal", () => ({ SessionImportContinuationModal: () => null }));

const retainedEvents = Array.from({ length: MAX_READER_EVENTS }, (_, i) => ({
  id: `event-${10_000 - MAX_READER_EVENTS + i}`, kind: "message", role: "user", text: `Retained body ${i}`,
}));
const scope = { revision: "scale-revision", leaf: "scale-leaf" };

beforeEach(() => {
  mocks.reader = {
    scope, getScope: () => scope, events: retainedEvents, health: null,
    previousCursor: "9200", nextCursor: null, query: "", setQuery: vi.fn(),
    hits: [], hitsPrevious: null, hitsNext: null, hitsWindowed: false, eventsWindowed: true,
    notice: null, error: null, searchError: null, exportError: null, busy: false,
    reload: vi.fn(async () => {}), search: vi.fn(async () => {}),
    loadMore: vi.fn(async () => null), loadPrevious: vi.fn(async () => null),
    loadMoreHits: vi.fn(async () => {}), loadPreviousHits: vi.fn(async () => {}),
    ensureEvent: vi.fn(async () => true), clearSearch: vi.fn(), exportMarkdown: vi.fn(async () => {}),
  };
});
afterEach(() => { cleanup(); vi.resetAllMocks(); });

test("the real Host renders only 160 of the reader's 800 retained events", () => {
  // The paging budget is tested in useImportedReader.scale.test.tsx. Pass its
  // full retained buffer here so only the production Host can limit the DOM.
  render(<SessionImportHost locale="en" />);
  expect(retainedEvents).toHaveLength(800);
  const rows = document.querySelectorAll(".session-import__events > li");
  expect(rows).toHaveLength(MAX_RENDERED_EVENTS);
  expect(rows[0].id).toBe("import-ev-event-9840");
  expect(rows[rows.length - 1].id).toBe("import-ev-event-9999");
  expect(document.getElementById("import-ev-event-9200")).toBeNull();
});
