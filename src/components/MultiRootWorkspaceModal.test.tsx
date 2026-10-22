/** @vitest-environment jsdom */
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { WorkspaceRecord } from "@/lib/multiRootWorkspace";
import { MultiRootWorkspaceModal } from "./MultiRootWorkspaceModal";

afterEach(cleanup);

const record = (id: string): WorkspaceRecord => ({ id, name: id, primaryProjectId: "p1", roots: [{ path: "/project", role: "primary", access: "write" }], capability: "contextOnly", updatedAt: "fixture" });

test("saved native imports are selectable through the real custom dropdown", async () => {
  const onSelect = vi.fn();
  const onSave = vi.fn();
  const first = record("First workspace");
  const imported = record("Imported workspace");
  render(<MultiRootWorkspaceModal open locale="en" busy={false} error={null} draft={first} projectName="Project"
    onClose={vi.fn()} onNameChange={vi.fn()} onAddRoot={vi.fn()} onRemoveRoot={vi.fn()} onSetExtraAccess={vi.fn()}
    onSave={onSave} writeCapableMode={false} workspaces={[first, imported]} onSelectWorkspace={onSelect} />);
  fireEvent.click(screen.getByRole("button", { name: "Saved workspaces" }));
  fireEvent.click(within(await screen.findByRole("option", { name: "Imported workspace" })).getByRole("button"));
  expect(onSelect).toHaveBeenCalledWith(imported.id);
  expect(onSave).not.toHaveBeenCalled();
  expect(document.querySelector("select")).toBeNull();
});
