/** @vitest-environment jsdom */
import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { SourceChoice } from "@/lib/sessionImport/sourceChoice";

const mocks = vi.hoisted(() => ({
  busy: true,
  scanStarting: false,
  choice: null as SourceChoice | null,
  scan: vi.fn(),
  pick: vi.fn(),
  closeWizard: vi.fn(),
}));
vi.mock("@/hooks/useSessionImport", () => ({
  useSessionImportUi: () => ({
    open: true, step: "source", busy: mocks.busy, scanStarting: mocks.scanStarting,
    roots: [], registrations: [], choice: mocks.choice,
    candidates: [], selected: new Set(), selectedCount: 0, mappings: [], projects: [],
    readerId: null, readerRevision: null, readerLeaf: null, readerAnchor: null,
    scan: mocks.scan, pickCustomFile: mocks.pick, pickCustomRoot: mocks.pick,
    setChoice: vi.fn(), setOpen: vi.fn(), setStep: vi.fn(),
    workspaceUi: { mode: null, list: vi.fn() },
  }),
}));
vi.mock("@/hooks/useImportedCatalog", () => ({ useImportedCatalog: () => ({ entries: [] }) }));
vi.mock("@/hooks/useImportedEntry", () => ({ useImportedEntry: () => ({ entry: null }) }));
vi.mock("@/hooks/useImportedReader", () => ({ useImportedReader: () => ({ events: [], hits: [] }) }));
vi.mock("@/hooks/useSessionImportContentStates", () => ({
  useSessionImportContentStates: () => ({
    byCandidate: {}, closeWizard: mocks.closeWizard, ensureSelected: vi.fn(),
  }),
}));
vi.mock("@/components/GlassModal", () => ({
  GlassModal: ({ open, children, footer }: { open: boolean; children: React.ReactNode; footer?: React.ReactNode }) =>
    open ? <div role="dialog"><div className="modal-body">{children}</div><div className="modal-actions">{footer}</div></div> : null,
}));

import { SessionImportHost } from "./SessionImportHost";

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  mocks.busy = true;
  mocks.scanStarting = false;
  mocks.choice = null;
});

test("V117 Host keeps scan-all disabled without claiming picker work is scanning", () => {
  render(<SessionImportHost locale="en" />);
  const button = screen.getByRole("button", { name: "Scan all defaults" });
  expect(button).toBeDisabled();
  fireEvent.click(button);
  expect(mocks.scan).not.toHaveBeenCalled();
  expect(screen.queryByRole("button", { name: /Scanning/ })).toBeNull();
});

test("V117 Host passes the real pending-scan state and restores the chosen action", () => {
  mocks.choice = { kind: "custom-file", provider: "generic-file", path: "D:/one.json" };
  mocks.scanStarting = true;
  const view = render(<SessionImportHost locale="en" />);
  expect(screen.getByRole("button", { name: /Scanning/ })).toBeDisabled();
  mocks.busy = false;
  mocks.scanStarting = false;
  view.rerender(<SessionImportHost locale="en" />);
  const button = screen.getByRole("button", { name: "Scan" });
  expect(button).toBeEnabled();
  fireEvent.click(button);
  expect(mocks.scan).toHaveBeenCalledOnce();
});

test("source scan actions are siblings of the scroll body and keep disabled state", () => {
  render(<SessionImportHost locale="en" />);
  const action = screen.getByRole("button", { name: "Scan all defaults" });
  expect(action.closest(".modal-body")).toBeNull();
  expect(action.closest(".modal-actions")?.previousElementSibling).toHaveClass("modal-body");
  expect(document.querySelectorAll(".session-import__source-actions")).toHaveLength(1);
  expect(action).toBeDisabled();
});
