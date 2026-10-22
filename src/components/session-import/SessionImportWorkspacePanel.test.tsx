/** @vitest-environment jsdom */
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { createT } from "@/i18n";
import { useWorkspaceImport } from "@/hooks/useWorkspaceImport";
import type { WorkspaceImportPreview } from "@/lib/api/sessionMigration";
import { SessionImportWorkspacePanel } from "./SessionImportWorkspacePanel";

const mocks = vi.hoisted(() => ({ preview: vi.fn(), save: vi.fn(), list: vi.fn(), get: vi.fn(), update: vi.fn(), pick: vi.fn(), apply: vi.fn(), create: vi.fn() }));
vi.mock("@/lib/api/sessionMigration", () => ({
  sessionImportWorkspacePreview: mocks.preview,
  sessionImportWorkspaceSave: mocks.save,
  sessionImportWorkspaces: mocks.list,
  sessionImportWorkspaceGet: mocks.get,
  sessionImportWorkspaceUpdate: mocks.update,
  sessionImportWorkspaceApply: mocks.apply,
  sessionImportProjectCreate: mocks.create,
}));
vi.mock("@/lib/api/fs", () => ({ pickDirectory: mocks.pick }));
afterEach(() => { cleanup(); vi.resetAllMocks(); });

function fixture(empty = false): WorkspaceImportPreview {
  return {
    source: { kind: "file", path: "/example.code-workspace" }, fingerprint: "fp", savedRevision: null,
    workspace: { sourceWorkspaceId: "ws:example", displayName: "example", evidence: [],
      roots: empty ? [] : ["one", "two"].map((id) => ({ rootId: id, originalPath: `/${id}`, pathSyntax: "posix", host: "local", availability: "missing", evidence: [] })),
    },
  };
}

function Harness() {
  const ui = useWorkspaceImport();
  const tr = createT("en");
  return ui.mode ? <SessionImportWorkspacePanel ui={ui} tr={tr} projects={[{ id: "existing", name: "My project" }]} /> : <>
    <button onClick={() => void ui.load(fixture().source)}>Open fixture</button>
    <button onClick={() => void ui.list()}>Browse fixtures</button>
  </>;
}

test("source installation selector submits a distinct preview and does not auto-save", async () => {
  const data = fixture(true);
  const longRoot = "/machine-b/" + "中文长目录🙂/".repeat(80) + "cursor";
  mocks.preview.mockResolvedValueOnce(data).mockResolvedValueOnce({ ...data,
    source: { ...data.source, sourceId: "source-b" },
    workspace: { ...data.workspace, sourceWorkspaceId: "ws:b", sourceIdentity: { sourceId: "source-b", provider: "cursor", locator: "file:/example.code-workspace" } },
  });
  function SourcesHarness() {
    const ui = useWorkspaceImport();
    return ui.mode ? <SessionImportWorkspacePanel ui={ui} tr={createT("en")} projects={[]}
      registrations={[
        { sourceId: "source-b", provider: "cursor", rootPath: longRoot, profile: "Laptop" },
        { sourceId: "source-c", provider: "cursor", rootPath: "/machine-c/cursor", profile: "Laptop" },
      ]} />
      : <button onClick={() => void ui.load(data.source)}>Open fixture</button>;
  }
  render(<SourcesHarness />);
  fireEvent.click(screen.getByText("Open fixture"));
  fireEvent.click(await screen.findByRole("button", { name: "Source installation" }));
  const option = await screen.findByRole("option", { name: "cursor · Laptop · source-b" });
  expect(option.textContent).not.toContain(longRoot);
  expect(screen.getByRole("option", { name: "cursor · Laptop · source-c" })).toBeTruthy();
  fireEvent.click(within(option).getByRole("button"));
  await waitFor(() => expect(screen.getByRole("button", { name: "Source installation" }).textContent).toContain("Laptop"));
  expect(mocks.preview.mock.calls[1][0]).toMatchObject({ sourceId: "source-b" });
  expect(screen.getByText(`cursor · Laptop · ${longRoot} · source-b`)).toBeTruthy();
  expect(mocks.save).not.toHaveBeenCalled();
  expect(document.querySelector("select")).toBeNull();
});

test("create project appears only after selecting a directory, then Save persists its mapping", async () => {
  const data = fixture();
  mocks.preview.mockResolvedValue(data);
  mocks.pick.mockResolvedValue("/new-target");
  mocks.create.mockResolvedValue({ project: { id: "created-project", name: "New project", path: "/new-target", trusted: false }, created: true });
  mocks.save.mockImplementation(async (request) => ({ ...data.workspace, mappingChoices: request.choices }));
  render(<Harness />);
  fireEvent.click(screen.getByText("Open fixture"));
  const chooseTarget = await screen.findByRole("button", { name: "Choose target folder…: /one" });
  expect(screen.queryByRole("button", { name: "Create project: /one" })).toBeNull();
  fireEvent.click(chooseTarget);
  fireEvent.click(await screen.findByRole("button", { name: "Create project: /one" }));
  await screen.findByText(/“New project” is ready/);
  expect(mocks.save).not.toHaveBeenCalled();
  expect(screen.getByRole("button", { name: "Project mapping for /one: New project" }).textContent).toContain("New project");
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await screen.findByText("Workspace metadata saved.");
  expect(mocks.save.mock.calls[0][0].choices[0].targetProjectId).toBe("created-project");
});

test("creation is explicit and usable after mapped metadata has been saved", async () => {
  const workspace = { ...fixture().workspace, mappingChoices: [
    { rootId: "one", primary: true, targetProjectId: "existing" },
    { rootId: "two", primary: false, targetProjectId: null, explicitTarget: "/mapped" },
  ] };
  mocks.list.mockResolvedValue({ items: [workspace], nextCursor: null });
  mocks.get.mockResolvedValue({ workspace, revision: "reviewed" });
  mocks.apply.mockRejectedValueOnce("SNAPSHOT_UNAVAILABLE: retry").mockResolvedValueOnce({ name: "example" });
  render(<Harness />);
  fireEvent.click(screen.getByText("Browse fixtures"));
  fireEvent.click(await screen.findByRole("button", { name: /example/ }));
  expect(mocks.apply).not.toHaveBeenCalled();
  const create = screen.getByRole("button", { name: "Create project workspace" });
  fireEvent.click(create);
  await screen.findByRole("alert");
  await waitFor(() => expect((create as HTMLButtonElement).disabled).toBe(false));
  fireEvent.click(create);
  await screen.findByText(/Created “example”/);
  expect((create as HTMLButtonElement).disabled).toBe(true);
});

test("unmapped and empty workspaces cannot create a native copy", async () => {
  mocks.list.mockResolvedValue({ items: [fixture().workspace], nextCursor: null });
  render(<Harness />);
  fireEvent.click(screen.getByText("Browse fixtures"));
  fireEvent.click(await screen.findByRole("button", { name: /example/ }));
  const create = screen.getByRole("button", { name: "Create project workspace" });
  expect((create as HTMLButtonElement).disabled).toBe(true);
  fireEvent.click(create);
  expect(mocks.apply).not.toHaveBeenCalled();
});

test("real panel shows zero roots and saves only on explicit confirmation", async () => {
  const data = fixture(true);
  mocks.preview.mockResolvedValue(data);
  mocks.save.mockResolvedValue(data.workspace);
  render(<Harness />);
  fireEvent.click(screen.getByText("Open fixture"));
  await screen.findByText("This workspace declares no folders.");
  expect(mocks.save).not.toHaveBeenCalled();
  expect(screen.queryByText("Scan")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await screen.findByText("Workspace metadata saved.");
  expect(mocks.save.mock.calls[0][0].choices).toEqual([]);
});

test("real custom Select and primary controls submit workspace root mappings", async () => {
  const data = fixture();
  mocks.preview.mockResolvedValue(data);
  mocks.save.mockImplementation(async (request) => ({ ...data.workspace, mappingChoices: request.choices }));
  render(<Harness />);
  fireEvent.click(screen.getByText("Open fixture"));
  await screen.findByText("/two");
  fireEvent.click(screen.getByRole("button", { name: "Project mapping for /two: No project mapping" }));
  fireEvent.click(within(await screen.findByRole("option", { name: "My project" })).getByRole("button"));
  fireEvent.click(screen.getAllByRole("button", { name: "Primary root" })[1]);
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await screen.findByText("Workspace metadata saved.");
  expect(mocks.save.mock.calls[0][0].choices).toEqual([
    { rootId: "one", targetProjectId: null, primary: false },
    { rootId: "two", targetProjectId: "existing", primary: true },
  ]);
  expect(document.querySelector("select")).toBeNull();
});

test("failed save exposes localized error and keeps a usable retry action", async () => {
  mocks.preview.mockResolvedValue(fixture());
  mocks.save.mockRejectedValueOnce("SOURCE_CHANGED: file changed").mockResolvedValueOnce(fixture().workspace);
  render(<Harness />);
  fireEvent.click(screen.getByText("Open fixture"));
  fireEvent.click(await screen.findByRole("button", { name: "Save" }));
  await screen.findByRole("alert");
  expect(screen.getByRole("alert").textContent).toBe("Source changed since preview");
  await waitFor(() => expect((screen.getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(false));
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await screen.findByText("Workspace metadata saved.");
});

test("saved record reopens offline from the catalog with no source read", async () => {
  mocks.list.mockResolvedValue({ items: [fixture().workspace], nextCursor: null });
  render(<Harness />);
  fireEvent.click(screen.getByText("Browse fixtures"));
  fireEvent.click(await screen.findByRole("button", { name: /example/ }));
  await screen.findByText("/two");
  expect(mocks.preview).not.toHaveBeenCalled();
  expect(screen.queryByRole("button", { name: "Save" })).toBeNull();
  expect((screen.getByRole("button", { name: "Project mapping for /one: No project mapping" }) as HTMLButtonElement).disabled).toBe(true);
});

async function browseAndEdit() {
  const data = fixture().workspace;
  mocks.list.mockResolvedValue({ items: [data], nextCursor: null });
  mocks.get.mockResolvedValue({ workspace: data, revision: "r1" });
  render(<Harness />);
  fireEvent.click(screen.getByText("Browse fixtures"));
  fireEvent.click(await screen.findByRole("button", { name: /example/ }));
  fireEvent.click(screen.getByRole("button", { name: "Edit mappings" }));
  await waitFor(() => expect((screen.getByRole("button", { name: "Save" }) as HTMLButtonElement).disabled).toBe(false));
  return data;
}

test("saved workspace can relocate a root and save without reopening its source", async () => {
  const data = await browseAndEdit();
  mocks.pick.mockResolvedValue("/new place");
  mocks.update.mockImplementation(async (request) => ({ workspace: { ...data, mappingChoices: request.choices }, revision: "r2" }));
  fireEvent.click(screen.getByRole("button", { name: "Choose target folder…: /one" }));
  await screen.findByText("/new place");
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await screen.findByText("Workspace metadata saved.");
  expect(mocks.update.mock.calls[0][0]).toMatchObject({ expectedRevision: "r1", choices: [
    { rootId: "one", explicitTarget: "/new place", primary: true }, { rootId: "two", primary: false },
  ] });
  expect(screen.getByRole("button", { name: "Project mapping for /one: /new place" })).toBeTruthy();
  expect(mocks.preview).not.toHaveBeenCalled();
  expect(mocks.save).not.toHaveBeenCalled();
});

test("cancel mapping edit restores saved primary without writing", async () => {
  await browseAndEdit();
  fireEvent.click(screen.getAllByRole("button", { name: "Primary root" })[1]);
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  expect(screen.queryByRole("button", { name: "Save" })).toBeNull();
  expect(screen.getAllByRole("button", { name: "Primary root" })[0].getAttribute("aria-pressed")).toBe("true");
  expect(mocks.update).not.toHaveBeenCalled();
});

test("mapping conflict keeps the draft visible and refresh enables a new save", async () => {
  const data = await browseAndEdit();
  mocks.update.mockRejectedValueOnce("PLAN_STALE: changed").mockResolvedValueOnce({ workspace: data, revision: "r2" });
  fireEvent.click(screen.getAllByRole("button", { name: "Primary root" })[1]);
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  expect((await screen.findByRole("alert")).textContent).toBe("Mappings changed elsewhere. Refresh before saving.");
  expect(screen.getAllByRole("button", { name: "Primary root" })[1].getAttribute("aria-pressed")).toBe("true");
  mocks.get.mockResolvedValue({ workspace: data, revision: "r2" });
  fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
  await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await screen.findByText("Workspace metadata saved.");
  expect(mocks.update.mock.calls[1][0].expectedRevision).toBe("r2");
});

test("remote roots with identical paths retain distinct URI labels and mapping controls", async () => {
  const data = fixture();
  data.workspace.roots = ["alpha", "beta"].map((host) => ({
    rootId: `ssh-${host}`, originalPath: "/repo", sourceUri: `vscode-remote://ssh-remote+${host}/repo`,
    hostIdentity: `ssh-remote+${host}`, host: "ssh", pathSyntax: "posix", availability: "offline", evidence: [],
  }));
  mocks.preview.mockResolvedValue(data);
  mocks.save.mockImplementation(async (request) => ({ ...data.workspace, mappingChoices: request.choices }));
  render(<Harness />);
  fireEvent.click(screen.getByText("Open fixture"));
  const uri = "vscode-remote://ssh-remote+beta/repo";
  fireEvent.click(await screen.findByRole("button", { name: `Project mapping for ${uri}: No project mapping` }));
  fireEvent.click(within(await screen.findByRole("option", { name: "My project" })).getByRole("button"));
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await screen.findByText("Workspace metadata saved.");
  expect(mocks.save.mock.calls[0][0].choices).toEqual([
    { rootId: "ssh-alpha", targetProjectId: null, primary: true },
    { rootId: "ssh-beta", targetProjectId: "existing", primary: false },
  ]);
  expect(screen.getByText(uri).closest("a")).toBeNull();
});

test("unsupported URI stays visible and can receive a local target without hiding other roots", async () => {
  const data = fixture();
  const sourceUri = "custom://server/repo";
  data.workspace.roots[1] = { ...data.workspace.roots[1], originalPath: sourceUri,
    sourceUri, unsupportedUri: true, host: "unknown", availability: "unresolved" };
  mocks.preview.mockResolvedValue(data);
  mocks.pick.mockResolvedValue("/relocated");
  mocks.save.mockImplementation(async (request) => ({ ...data.workspace, mappingChoices: request.choices }));
  render(<Harness />);
  fireEvent.click(screen.getByText("Open fixture"));
  await screen.findByText("Unsupported URI type — choose a target folder");
  fireEvent.click(screen.getByRole("button", { name: `Choose target folder…: ${sourceUri}` }));
  await screen.findByText("/relocated");
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await screen.findByText("Workspace metadata saved.");
  expect(screen.getByRole("button", { name: "Project mapping for /one: No project mapping" })).toBeTruthy();
  expect(screen.getByText(sourceUri)).toBeTruthy();
  expect(mocks.save.mock.calls[0][0].choices[1]).toMatchObject({ rootId: "two", explicitTarget: "/relocated" });
});
