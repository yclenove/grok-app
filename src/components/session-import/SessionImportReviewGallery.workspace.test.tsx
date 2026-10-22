/** @vitest-environment jsdom */
import "@testing-library/jest-dom/vitest";
import { StrictMode, useState } from "react";
import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi, type MockInstance } from "vitest";
import { createT } from "@/i18n";
import { GlassModal } from "@/components/GlassModal";
import { useWorkspaceImport, type WorkspaceImportClient } from "@/hooks/useWorkspaceImport";
import { SessionImportWorkspacePanel } from "./SessionImportWorkspacePanel";
import { SessionImportReviewGallery } from "./SessionImportReviewGallery";
import { LONG_PATH, registration, SYNTHETIC_PROJECT } from "./sessionImportReviewFixtures";
import { createReviewWorkspaceClient, REVIEW_WORKSPACE_FILE, REVIEW_RELOCATED_FOLDER } from "./sessionImportReviewWorkspaceClient";

const { live } = vi.hoisted(() => ({ live: vi.fn(() => { throw new Error("Live host call in synthetic workspace review"); }) }));
vi.mock("@/lib/api/host", () => ({ invoke: live, isTauri: () => false }));
vi.mock("@/lib/nativeWebviewCover", () => ({
  acquireNativeWebviewCover: () => () => {}, rectOverlapsNativeWebviewHost: () => false,
}));
vi.mock("@/components/MarkdownBody", () => ({
  MarkdownBody: ({ children }: { children: string }) => <div className="md-body">{children}</div>,
}));

const tr = createT("en");
let storageRead: MockInstance<typeof Storage.prototype.getItem>;
let storageWrite: MockInstance<typeof Storage.prototype.setItem>;
beforeEach(() => {
  storageRead = vi.spyOn(Storage.prototype, "getItem");
  storageWrite = vi.spyOn(Storage.prototype, "setItem");
  window.history.replaceState(null, "", "/gallery.html?surface=source");
});
afterEach(() => {
  expect(live).not.toHaveBeenCalled();
  expect(storageRead).not.toHaveBeenCalled(); expect(storageWrite).not.toHaveBeenCalled();
  cleanup(); vi.restoreAllMocks(); live.mockClear(); window.history.replaceState(null, "", "/");
});

async function openGallery(params = "") {
  window.history.replaceState(null, "", `/gallery.html?surface=source${params}`);
  render(<StrictMode><SessionImportReviewGallery /></StrictMode>);
  return within(await screen.findByRole("dialog", { name: tr("sessionImport.title") }));
}

test("synthetic file selection distinguishes workspace metadata from transcript discovery and can cancel", async () => {
  const user = userEvent.setup(); const panel = await openGallery();
  await user.click(panel.getByRole("button", { name: tr("sessionImport.fileMode") }));
  const picker = await screen.findByRole("dialog", { name: tr("sessionImport.fileMode") });
  await user.click(within(picker).getByRole("button", { name: tr("common.close") }));
  expect(panel.getByRole("button", { name: tr("sessionImport.scanAll") })).toBeEnabled();
  expect(document.querySelector("[data-import-workspace]")).toBeNull();
  await user.click(panel.getByRole("button", { name: tr("sessionImport.fileMode") }));
  await user.click(within(await screen.findByRole("dialog", { name: tr("sessionImport.fileMode") }))
    .getByRole("button", { name: LONG_PATH }));
  expect(document.querySelector("[data-source-kind='custom-file']")).toHaveTextContent(LONG_PATH);
  expect(document.querySelector("[data-import-workspace]")).toBeNull();
  await user.click(panel.getByRole("button", { name: tr("sessionImport.fileMode") }));
  await user.click(within(await screen.findByRole("dialog", { name: tr("sessionImport.fileMode") }))
    .getByRole("button", { name: REVIEW_WORKSPACE_FILE }));
  await waitFor(() => expect(document.querySelector("[data-import-workspace]")).toHaveAttribute("data-import-workspace", "preview"));
  expect(panel.getByRole("heading", { name: "Synthetic file workspace 🙂 锚点" })).toBeInTheDocument();
  expect(panel.queryByRole("button", { name: tr("sessionImport.scan") })).toBeNull();
  expect(panel.getByRole("button", { name: tr("common.save") }).closest(".modal-body")).toBeNull();
});

test("file workspace chooses installation, maps roots, explicitly creates a project, saves and creates a workspace", async () => {
  const user = userEvent.setup(); const panel = await openGallery();
  await user.click(panel.getByRole("button", { name: tr("sessionImport.fileMode") }));
  await user.click(within(await screen.findByRole("dialog", { name: tr("sessionImport.fileMode") }))
    .getByRole("button", { name: REVIEW_WORKSPACE_FILE }));
  await panel.findByRole("heading", { name: "Synthetic file workspace 🙂 锚点" });
  await user.click(panel.getByRole("button", { name: tr("sessionImport.workspace.sourceIdentity") }));
  await user.click(within(await screen.findByRole("option", { name: /synthetic/ })).getByRole("button"));
  await waitFor(() => expect(panel.getByRole("button", { name: tr("sessionImport.workspace.sourceIdentity") }))
    .toHaveTextContent(registration.profile!));
  const app = `${SYNTHETIC_PROJECT}\\app`; const docs = `${SYNTHETIC_PROJECT}\\docs`;
  const remote = "vscode-remote://ssh-remote+synthetic/archived/repo";
  for (const source of [app, remote]) {
    await user.click(panel.getByRole("button", { name: tr("sessionImport.mappingTarget", {
      source, target: tr("sessionImport.workspace.unassigned"),
    }) }));
    await user.click(within(await screen.findByRole("option", { name: "Synthetic project" })).getByRole("button"));
  }
  await user.click(panel.getByRole("button", { name: `${tr("sessionImport.workspace.chooseTarget")}: ${docs}` }));
  await user.click(await panel.findByRole("button", { name: `${tr("sessionImport.workspace.createProject")}: ${docs}` }));
  await panel.findByText(tr("sessionImport.workspace.projectReady", { name: "Synthetic created project" }));
  await user.click(panel.getByRole("button", { name: tr("common.save") }));
  await panel.findByText(tr("sessionImport.workspace.saved"));
  const create = panel.getByRole("button", { name: tr("sessionImport.workspace.createNative") });
  expect(create).toBeEnabled(); expect(create.closest(".modal-actions")).not.toBeNull();
  expect(create).toHaveFocus();
  await user.click(create);
  await panel.findByText(tr("sessionImport.workspace.nativeCreated", { name: "Synthetic file workspace 🙂 锚点" }));
  expect(create).toBeDisabled();
  await user.click(panel.getByRole("button", { name: tr("sessionImport.back") }));
  expect(panel.getByRole("button", { name: tr("sessionImport.workspace.folder") })).toBeInTheDocument();
});

test("folder preview keeps one declared root and its exact chosen directory", async () => {
  const user = userEvent.setup(); const panel = await openGallery();
  await user.click(panel.getByRole("button", { name: tr("sessionImport.workspace.folder") }));
  await panel.findByRole("heading", { name: "Synthetic folder workspace 🙂 锚点" });
  expect(document.activeElement?.closest("[role='dialog']")).toHaveAccessibleName(tr("sessionImport.title"));
  expect(document.querySelectorAll(".session-import__workspace-roots > li")).toHaveLength(1);
  expect(panel.getByRole("button", { name: tr("sessionImport.mappingTarget", {
    source: SYNTHETIC_PROJECT, target: "Synthetic project",
  }) })).toBeInTheDocument();
  expect(panel.queryByText("Documentation")).toBeNull();
  expect(panel.queryByText("Remote archive")).toBeNull();
});

test("saved workspace pagination, edit cancellation, revision conflict, refresh and save stay usable", async () => {
  const user = userEvent.setup(); const panel = await openGallery("&workspace=conflict");
  await user.click(panel.getByRole("button", { name: tr("sessionImport.workspace.savedList") }));
  await user.click(await panel.findByRole("button", { name: tr("sessionImport.loadMore") }));
  await panel.findByRole("button", { name: /Synthetic folder workspace/ });
  expect(panel.getByRole("button", { name: tr("sessionImport.back") })).toHaveFocus();
  await user.click(panel.getByRole("button", { name: /Synthetic file workspace/ }));
  await user.click(panel.getByRole("button", { name: tr("sessionImport.workspace.editMappings") }));
  const primary = await panel.findAllByRole("button", { name: tr("sessionImport.primaryRoot") });
  await waitFor(() => expect(primary[1]).toBeEnabled());
  await user.click(primary[1]); await user.click(panel.getByRole("button", { name: tr("common.cancel") }));
  expect(panel.getAllByRole("button", { name: tr("sessionImport.primaryRoot") })[0]).toHaveAttribute("aria-pressed", "true");
  await user.click(panel.getByRole("button", { name: tr("sessionImport.workspace.editMappings") }));
  await waitFor(() => expect(panel.getByRole("button", { name: tr("common.save") })).toBeEnabled());
  await user.click(panel.getAllByRole("button", { name: tr("sessionImport.primaryRoot") })[1]);
  await user.click(panel.getByRole("button", { name: tr("common.save") }));
  expect(await panel.findByRole("alert")).toHaveTextContent(tr("sessionImport.workspace.mappingChanged"));
  expect(panel.getAllByRole("button", { name: tr("sessionImport.primaryRoot") })[1]).toHaveAttribute("aria-pressed", "true");
  await user.click(panel.getByRole("button", { name: tr("sessionImport.refresh") }));
  await waitFor(() => expect(panel.queryByRole("alert")).toBeNull());
  expect(panel.getAllByRole("button", { name: tr("sessionImport.primaryRoot") })[0]).toHaveAttribute("aria-pressed", "true");
  await user.click(panel.getByRole("button", { name: tr("common.save") }));
  await panel.findByText(tr("sessionImport.workspace.saved"));
});

test("relocation compares roots, preserves a lost-response attempt over close and retries without another commit", async () => {
  const user = userEvent.setup(); const panel = await openGallery("&relocation=retry");
  await user.click(panel.getByRole("button", { name: tr("sessionImport.workspace.savedList") }));
  await user.click(await panel.findByRole("button", { name: /Synthetic file workspace/ }));
  await user.click(panel.getByRole("button", { name: tr("sessionImport.relocate.title") }));
  let dialog = await screen.findByRole("dialog", { name: tr("sessionImport.relocate.title") });
  await user.click(within(dialog).getByRole("button", { name: tr("sessionImport.relocate.choose") }));
  for (const key of ["sessionImport.relocate.retained", "sessionImport.relocate.added", "sessionImport.relocate.removed"] as const) {
    expect(await within(dialog).findByRole("heading", { name: tr(key) })).toBeInTheDocument();
  }
  const save = within(dialog).getByRole("button", { name: tr("common.save") });
  expect(save.closest(".modal-body")).toBeNull();
  await user.click(save); await within(dialog).findByRole("alert");
  expect(within(dialog).getByRole("button", { name: tr("sessionImport.relocate.choose") })).toBeDisabled();
  await user.click(within(dialog).getAllByRole("button", { name: tr("common.close") }).at(-1)!);
  await user.click(panel.getByRole("button", { name: tr("sessionImport.relocate.title") }));
  dialog = await screen.findByRole("dialog", { name: tr("sessionImport.relocate.title") });
  expect(within(dialog).getByText(`${REVIEW_RELOCATED_FOLDER}\\review.code-workspace`)).toBeInTheDocument();
  expect(within(dialog).getByRole("button", { name: tr("sessionImport.relocate.choose") })).toBeDisabled();
  await user.click(within(dialog).getByRole("button", { name: tr("sessionImport.relocate.retry") }));
  await waitFor(() => expect(screen.queryByRole("dialog", { name: tr("sessionImport.relocate.title") })).toBeNull());
  expect(panel.getByText(`${REVIEW_RELOCATED_FOLDER}\\tests`)).toBeInTheDocument();
  expect(panel.queryByText("Remote archive")).toBeNull();
  expect(panel.getByRole("button", { name: tr("sessionImport.workspace.createNative") })).toBeDisabled();
});

function CoveredWorkspace({ client, covered }: { client: WorkspaceImportClient; covered: boolean }) {
  const ui = useWorkspaceImport(undefined, client);
  const [footer, setFooter] = useState<HTMLDivElement | null>(null);
  return <>
    <GlassModal open wrapBody onClose={() => {}} title="Synthetic workspace owner" closeLabel="Close owner"
      footer={<div ref={setFooter} className="session-import__action-host" />}>
      {ui.mode ? <SessionImportWorkspacePanel ui={ui} tr={tr} projects={[{ id: "proj-synthetic", name: "Synthetic project" }]}
        footerTarget={footer} /> : <button onClick={() => void ui.load({ kind: "folder", path: SYNTHETIC_PROJECT })}>Open folder</button>}
    </GlassModal>
    {covered ? <GlassModal open wrapBody onClose={() => {}} title="Synthetic newer dialog" closeLabel="Close newer">
      <textarea aria-label="Newer editor" />
    </GlassModal> : null}
  </>;
}

test("a late workspace save never takes keyboard focus from a newer dialog", async () => {
  const user = userEvent.setup(); const local = createReviewWorkspaceClient();
  let release!: () => void;
  const pending = new Promise<void>((resolve) => { release = resolve; });
  const client: WorkspaceImportClient = { ...local.client,
    async sessionImportWorkspaceSave(request) { await pending; return local.client.sessionImportWorkspaceSave(request); },
  };
  const { rerender } = render(<CoveredWorkspace client={client} covered={false} />);
  await user.click(screen.getByRole("button", { name: "Open folder" }));
  await user.click(await screen.findByRole("button", { name: tr("common.save") }));
  rerender(<CoveredWorkspace client={client} covered />);
  const editor = within(await screen.findByRole("dialog", { name: "Synthetic newer dialog" })).getByRole("textbox");
  await user.click(editor); await user.type(editor, "Newer synthetic text");
  await act(async () => { release(); await pending; });
  await screen.findByText(tr("sessionImport.workspace.saved"));
  expect(editor).toHaveFocus();
  await user.keyboard("{Enter}still owned");
  expect(editor).toHaveValue("Newer synthetic text\nstill owned");
});
