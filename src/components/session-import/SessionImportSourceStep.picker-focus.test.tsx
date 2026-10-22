/** @vitest-environment jsdom */
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, expect, test, vi } from "vitest";
import type { MessageKey, Vars } from "@/i18n";
import { GlassModal } from "@/components/GlassModal";
import { useSessionImportUi } from "@/hooks/useSessionImport";
import { installDialogFocus } from "@/lib/a11yFocus";
import { openSessionImport } from "@/lib/sessionImport/open";
import type { SourceRegistration, WorkspaceImportPreview } from "@/lib/api/sessionMigration";
import { choiceFromDefaultRoot } from "@/lib/sessionImport/sourceChoice";
import { SessionImportSourceStep } from "./SessionImportSourceStep";

const mocks = vi.hoisted(() => ({
  entries: vi.fn().mockResolvedValue([]),
  recovery: vi.fn().mockResolvedValue({
    plan: null, job: null, draft: null, expired: false, failed: null,
  }),
  startScan: vi.fn().mockResolvedValue(true),
  stopScan: vi.fn(),
  pickFiles: vi.fn().mockResolvedValue([]),
  pickFolder: vi.fn().mockResolvedValue(null),
  workspacePreview: vi.fn(),
  roots: vi.fn().mockResolvedValue([]),
  sources: vi.fn().mockResolvedValue([]),
  projects: vi.fn().mockResolvedValue([]),
  registerSource: vi.fn(),
}));

vi.mock("@/lib/api", () => ({
  sessionImportEntries: mocks.entries,
  sessionImportRecovery: mocks.recovery,
  sessionImportListRoots: mocks.roots,
  sessionImportSources: mocks.sources,
  sessionImportSourceRegister: mocks.registerSource,
  projectsList: mocks.projects,
  pickAttachFiles: mocks.pickFiles,
  pickDirectory: mocks.pickFolder,
}));

vi.mock("@/lib/api/sessionMigration", async (original) => ({
  ...await original<typeof import("@/lib/api/sessionMigration")>(),
  sessionImportWorkspacePreview: mocks.workspacePreview,
  sessionImportPrepareStart: vi.fn(),
  sessionImportPrepareGet: vi.fn(),
  sessionImportPrepareItems: vi.fn(),
  sessionImportPrepareCancel: vi.fn(),
  sessionImportPrepareRetry: vi.fn(),
}));

vi.mock("@/hooks/useSessionImportJob", () => ({
  importJobActive: () => false,
  useSessionImportJob: () => ({
    job: null,
    setJob: vi.fn(),
    jobActive: false,
    jobBusy: false,
    jobItemsStale: false,
    jobHasPrevious: false,
    refreshJobItems: vi.fn(),
    loadMoreJobItems: vi.fn(),
    previousJobItems: vi.fn(),
    mutateJob: vi.fn(),
  }),
}));

vi.mock("@/hooks/useSessionImportScan", () => ({
  useSessionImportScan: () => ({
    candidates: [],
    selected: new Set<string>(),
    scanGeneration: { current: 1 },
    scanState: null,
    selection: null,
    scanBusy: false,
    startScan: mocks.startScan,
    stopIncompleteScan: mocks.stopScan,
    changePage: vi.fn(),
    controlScan: vi.fn(),
    // This suite isolates native picker focus, not real scan/group ownership.
    // Keep the explicit scan facade compatible with the production bridge.
    getState: () => ({ scan: null, selection: null }),
    clearIntent: vi.fn(),
  }),
}));

const tr = (key: MessageKey, vars?: Vars) =>
  vars ? `${key}:${JSON.stringify(vars)}` : key;

const registeredSource: SourceRegistration = {
  sourceId: "src-1", provider: "cursor", rootPath: "D:/cursor-copy", profile: "default",
};

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (cause: unknown) => void;
  const promise = new Promise<T>((ok, fail) => {
    resolve = ok;
    reject = fail;
  });
  return { promise, resolve, reject };
}

function workspaceFixture(): WorkspaceImportPreview {
  return {
    source: { path: "C:/empty.code-workspace", kind: "file" },
    workspace: { sourceWorkspaceId: "ws:empty", displayName: "empty", roots: [], evidence: [] },
    fingerprint: "fp", savedRevision: null,
  };
}

function Harness() {
  const ui = useSessionImportUi();
  const [ping, setPing] = useState(0);
  const sourceOpen = ui.open && ui.step === "source" && !ui.workspaceUi.mode;
  return (
    <>
      <button type="button" onClick={() => ui.setOpen(true)}>Show import</button>
      <button type="button" onClick={() => ui.setOpen(false)}>Hide import</button>
      <button type="button" onClick={() => ui.setChoice(choiceFromDefaultRoot("claude-code", "C:/switched"))}>
        Switch source
      </button>
      <button type="button" onClick={() => setPing((value) => value + 1)}>Ping</button>
      <span data-ping={ping} />
      <textarea aria-label="composer" />
      <div data-import-dialog="1">
        <button type="button">Close</button>
        {sourceOpen ? (
          <SessionImportSourceStep
            tr={tr}
            roots={ui.roots}
            registrations={ui.registrations}
            choice={ui.choice}
            busy={ui.busy}
            onChoose={ui.setChoice}
            onPickFolder={(provider) => ui.pickCustomRoot(provider)}
            onPickFile={() => ui.pickCustomFile()}
            onPickWorkspaceFolder={() => ui.pickWorkspaceFolder()}
            onRelocate={(reg) => ui.relocateRegistered(reg)}
            onScan={() => ui.scan()}
            fileFlight={ui.filePickerFlight}
          />
        ) : null}
      </div>
      {ui.workspaceUi.mode ? <div data-workspace-mode={ui.workspaceUi.mode} /> : null}
      {ui.error ? <p role="alert">{ui.error}</p> : null}
      <output data-choice-kind={ui.choice?.kind ?? ""} data-choice-path={ui.choice?.path ?? ""} />
    </>
  );
}

function choiceOf() {
  const node = document.querySelector("output");
  return {
    kind: node?.getAttribute("data-choice-kind") ?? "",
    path: node?.getAttribute("data-choice-path") ?? "",
  };
}

function buttonNamed(name: string) {
  return screen.getByRole("button", { name }) as HTMLButtonElement;
}

function loseFocus(control: HTMLElement) {
  // jsdom leaves document.activeElement on a disabled button. A native picker
  // moves focus to the document, which is the state this helper reproduces.
  const body = control.ownerDocument.body;
  body.tabIndex = -1;
  body.focus();
  body.removeAttribute("tabindex");
  if (document.activeElement === control) control.blur();
}

async function openSource() {
  fireEvent.click(buttonNamed("Show import"));
  await waitFor(() => expect(buttonNamed("sessionImport.chooseRoot").disabled).toBe(false));
}

async function beginPicker(name: string, arm: () => void) {
  arm();
  const trigger = buttonNamed(name);
  trigger.focus();
  fireEvent.click(trigger);
  await waitFor(() => expect(trigger.disabled).toBe(true));
  loseFocus(trigger);
  expect(document.activeElement).not.toBe(trigger);
  return trigger;
}

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  mocks.entries.mockReset().mockResolvedValue([]);
  mocks.recovery.mockReset().mockResolvedValue({
    plan: null, job: null, draft: null, expired: false, failed: null,
  });
  mocks.startScan.mockReset().mockResolvedValue(true);
  mocks.stopScan.mockReset();
  mocks.pickFiles.mockReset().mockResolvedValue([]);
  mocks.pickFolder.mockReset().mockResolvedValue(null);
  mocks.workspacePreview.mockReset();
  mocks.roots.mockReset().mockResolvedValue([]);
  mocks.sources.mockReset().mockResolvedValue([]);
  mocks.projects.mockReset().mockResolvedValue([]);
  mocks.registerSource.mockReset();
});

test("cancelled folder picker restores the re-enabled trigger and does not scan", async () => {
  const pending = deferred<string | null>();
  render(<Harness />);
  await openSource();
  const trigger = await beginPicker("sessionImport.chooseRoot", () => {
    mocks.pickFolder.mockReturnValueOnce(pending.promise);
  });
  await act(async () => { pending.resolve(null); });
  await waitFor(() => expect(trigger.disabled).toBe(false));
  expect(document.activeElement).toBe(trigger);
  expect(buttonNamed("Close")).not.toBe(document.activeElement);
  expect(choiceOf()).toEqual({ kind: "", path: "" });
  expect(mocks.startScan).not.toHaveBeenCalled();
  expect(mocks.workspacePreview).not.toHaveBeenCalled();
});

test("rejected folder picker restores the trigger and keeps the error", async () => {
  const pending = deferred<string | null>();
  render(<Harness />);
  await openSource();
  const trigger = await beginPicker("sessionImport.chooseRoot", () => {
    mocks.pickFolder.mockReturnValueOnce(pending.promise);
  });
  await act(async () => { pending.reject(new Error("picker denied")); });
  await waitFor(() => expect(trigger.disabled).toBe(false));
  expect(document.activeElement).toBe(trigger);
  expect(screen.getByRole("alert").textContent).toBe("Error: picker denied");
  expect(choiceOf().path).toBe("");
  expect(mocks.startScan).not.toHaveBeenCalled();
});

test("a chosen folder restores the same trigger without scanning", async () => {
  const pending = deferred<string | null>();
  render(<Harness />);
  await openSource();
  const trigger = await beginPicker("sessionImport.chooseRoot", () => {
    mocks.pickFolder.mockReturnValueOnce(pending.promise);
  });
  await act(async () => { pending.resolve("C:/synthetic-folder"); });
  await waitFor(() => expect(choiceOf().path).toBe("C:/synthetic-folder"));
  expect(choiceOf().kind).toBe("custom-folder");
  expect(document.activeElement).toBe(trigger);
  expect(mocks.startScan).not.toHaveBeenCalled();
  expect(mocks.workspacePreview).not.toHaveBeenCalled();
});

test("an ordinary file restores the file trigger and stays on the generic scan path", async () => {
  const pending = deferred<string[]>();
  render(<Harness />);
  await openSource();
  const trigger = await beginPicker("sessionImport.fileMode", () => {
    mocks.pickFiles.mockReturnValueOnce(pending.promise);
  });
  await act(async () => { pending.resolve(["C:/export.json"]); });
  await waitFor(() => expect(choiceOf().path).toBe("C:/export.json"));
  expect(choiceOf().kind).toBe("custom-file");
  expect(document.activeElement).toBe(trigger);
  expect(mocks.workspacePreview).not.toHaveBeenCalled();
  expect(mocks.startScan).not.toHaveBeenCalled();
});

test("cancelling a file picker restores the file trigger", async () => {
  const pending = deferred<string[]>();
  render(<Harness />);
  await openSource();
  const trigger = await beginPicker("sessionImport.fileMode", () => {
    mocks.pickFiles.mockReturnValueOnce(pending.promise);
  });
  await act(async () => { pending.resolve([]); });
  await waitFor(() => expect(trigger.disabled).toBe(false));
  expect(document.activeElement).toBe(trigger);
  expect(choiceOf().path).toBe("");
  expect(mocks.startScan).not.toHaveBeenCalled();
});

test("a workspace file leaves the source step and does not focus the composer", async () => {
  const pending = deferred<string[]>();
  mocks.workspacePreview.mockResolvedValue(workspaceFixture());
  render(<Harness />);
  await openSource();
  const composer = screen.getByRole("textbox", { name: "composer" });
  await beginPicker("sessionImport.fileMode", () => {
    mocks.pickFiles.mockReturnValueOnce(pending.promise);
  });
  await act(async () => { pending.resolve(["C:/empty.code-workspace"]); });
  await waitFor(() => expect(document.querySelector("[data-workspace-mode]")).toBeTruthy());
  expect(screen.queryByRole("button", { name: "sessionImport.chooseRoot" })).toBeNull();
  expect(document.activeElement).not.toBe(composer);
  expect(choiceOf().path).toBe("");
  expect(mocks.workspacePreview).toHaveBeenCalledWith({ path: "C:/empty.code-workspace", kind: "file" });
  expect(mocks.startScan).not.toHaveBeenCalled();
});

test("cancelling a workspace folder picker restores that trigger and does not preview", async () => {
  const pending = deferred<string | null>();
  render(<Harness />);
  await openSource();
  const trigger = await beginPicker("sessionImport.workspace.folder", () => {
    mocks.pickFolder.mockReturnValueOnce(pending.promise);
  });
  await act(async () => { pending.resolve(null); });
  await waitFor(() => expect(trigger.disabled).toBe(false));
  expect(document.activeElement).toBe(trigger);
  expect(document.querySelector("[data-workspace-mode]")).toBeNull();
  expect(mocks.workspacePreview).not.toHaveBeenCalled();
  expect(mocks.startScan).not.toHaveBeenCalled();
});

test("a workspace folder selection leaves the source step without focusing the composer", async () => {
  const pending = deferred<string | null>();
  mocks.workspacePreview.mockResolvedValue(workspaceFixture());
  render(<Harness />);
  await openSource();
  const composer = screen.getByRole("textbox", { name: "composer" });
  await beginPicker("sessionImport.workspace.folder", () => {
    mocks.pickFolder.mockReturnValueOnce(pending.promise);
  });
  await act(async () => { pending.resolve("C:/project"); });
  await waitFor(() => expect(document.querySelector("[data-workspace-mode]")?.getAttribute("data-workspace-mode")).toBe("preview"));
  expect(screen.queryByRole("button", { name: "sessionImport.workspace.folder" })).toBeNull();
  expect(document.activeElement).not.toBe(composer);
  expect(mocks.workspacePreview).toHaveBeenCalledWith({ path: "C:/project", kind: "folder" });
  expect(mocks.startScan).not.toHaveBeenCalled();
});

test("relocation success restores the relocate control on the same source surface", async () => {
  const pending = deferred<string | null>();
  const relocated = { ...registeredSource, rootPath: "D:/relocated" };
  mocks.sources.mockResolvedValue([registeredSource]);
  render(<Harness />);
  await openSource();
  await waitFor(() => expect(buttonNamed("sessionImport.relocateSource").disabled).toBe(false));
  const trigger = await beginPicker("sessionImport.relocateSource", () => {
    mocks.pickFolder.mockReturnValueOnce(pending.promise);
    mocks.registerSource.mockResolvedValueOnce(relocated);
    mocks.sources.mockResolvedValue([relocated]);
  });
  await act(async () => { pending.resolve("D:/relocated"); });
  await waitFor(() => expect(choiceOf()).toEqual({ kind: "relocate", path: "D:/relocated" }));
  expect(document.activeElement).toBe(trigger);
  expect(mocks.startScan).not.toHaveBeenCalled();
  expect(mocks.workspacePreview).not.toHaveBeenCalled();
});

test("a rejected relocation restores the relocate control", async () => {
  const pending = deferred<string | null>();
  mocks.sources.mockResolvedValue([registeredSource]);
  render(<Harness />);
  await openSource();
  await waitFor(() => expect(buttonNamed("sessionImport.relocateSource").disabled).toBe(false));
  const trigger = await beginPicker("sessionImport.relocateSource", () => {
    mocks.pickFolder.mockReturnValueOnce(pending.promise);
  });
  await act(async () => { pending.reject(new Error("relocate denied")); });
  await waitFor(() => expect(trigger.disabled).toBe(false));
  expect(document.activeElement).toBe(trigger);
  expect(screen.getByRole("alert").textContent).toBe("Error: relocate denied");
  expect(choiceOf().path).toBe("");
  expect(mocks.registerSource).not.toHaveBeenCalled();
  expect(mocks.startScan).not.toHaveBeenCalled();
});

test("a stale picker does not regain focus when source intent changes without a new claim", async () => {
  // Hook contract, not a reachable click. Source controls are disabled while
  // pickerBusy, so the product modal cannot call setChoice this way. The
  // harness button only invalidates sourceGeneration and leaves focus lost.
  const pending = deferred<string | null>();
  render(<Harness />);
  await openSource();
  const trigger = await beginPicker("sessionImport.chooseRoot", () => {
    mocks.pickFolder.mockReturnValueOnce(pending.promise);
  });
  fireEvent.click(buttonNamed("Switch source"));
  expect(choiceOf()).toEqual({ kind: "default", path: "C:/switched" });
  expect(trigger.isConnected).toBe(true);
  loseFocus(trigger);
  expect(document.activeElement).not.toBe(trigger);
  await act(async () => { pending.resolve("C:/from-old"); });
  await waitFor(() => expect(trigger.disabled).toBe(false));
  expect(choiceOf()).toEqual({ kind: "default", path: "C:/switched" });
  expect(document.activeElement).not.toBe(trigger);
  expect(mocks.startScan).not.toHaveBeenCalled();
  expect(mocks.workspacePreview).not.toHaveBeenCalled();
});

test("a real focus move is kept when a stale picker later completes", async () => {
  const first = deferred<string | null>();
  const second = deferred<string | null>();
  render(<Harness />);
  await openSource();
  await beginPicker("sessionImport.chooseRoot", () => {
    mocks.pickFolder.mockReturnValueOnce(first.promise);
  });
  const switcher = buttonNamed("Switch source");
  switcher.focus();
  fireEvent.click(switcher);
  expect(choiceOf().path).toBe("C:/switched");
  const trigger = await beginPicker("sessionImport.chooseRoot", () => {
    mocks.pickFolder.mockReturnValueOnce(second.promise);
  });
  await act(async () => { first.resolve("C:/from-old"); });
  expect(trigger.disabled).toBe(true);
  expect(document.activeElement).not.toBe(trigger);
  expect(choiceOf()).toEqual({ kind: "default", path: "C:/switched" });
  await act(async () => { second.resolve(null); });
  await waitFor(() => expect(trigger.disabled).toBe(false));
  expect(document.activeElement).toBe(trigger);
  expect(choiceOf()).toEqual({ kind: "default", path: "C:/switched" });
  expect(mocks.startScan).not.toHaveBeenCalled();
});

test("closing and reopening ignores a late picker and the next cancel restores the new trigger", async () => {
  const first = deferred<string | null>();
  render(<Harness />);
  await openSource();
  await beginPicker("sessionImport.chooseRoot", () => {
    mocks.pickFolder.mockReturnValueOnce(first.promise);
  });
  const hide = buttonNamed("Hide import");
  hide.focus();
  fireEvent.click(hide);
  expect(screen.queryByRole("button", { name: "sessionImport.chooseRoot" })).toBeNull();
  const composer = screen.getByRole("textbox", { name: "composer" });
  await openSource();
  composer.focus();
  await act(async () => { first.resolve("C:/after-close"); });
  expect(document.activeElement).toBe(composer);
  expect(choiceOf().path).toBe("");
  expect(mocks.startScan).not.toHaveBeenCalled();
  const second = deferred<string | null>();
  const trigger = await beginPicker("sessionImport.chooseRoot", () => {
    mocks.pickFolder.mockReturnValueOnce(second.promise);
  });
  await act(async () => { second.resolve(null); });
  await waitFor(() => expect(trigger.disabled).toBe(false));
  expect(document.activeElement).toBe(trigger);
});

test("focus already on Close or the composer is not stolen", async () => {
  const pending = deferred<string | null>();
  render(<Harness />);
  await openSource();
  const trigger = await beginPicker("sessionImport.chooseRoot", () => {
    mocks.pickFolder.mockReturnValueOnce(pending.promise);
  });
  const close = buttonNamed("Close");
  close.focus();
  await act(async () => { pending.resolve(null); });
  await waitFor(() => expect(trigger.disabled).toBe(false));
  expect(document.activeElement).toBe(close);
  const again = deferred<string | null>();
  await beginPicker("sessionImport.chooseRoot", () => {
    mocks.pickFolder.mockReturnValueOnce(again.promise);
  });
  const composer = screen.getByRole("textbox", { name: "composer" });
  composer.focus();
  await act(async () => { again.resolve("C:/synthetic-folder"); });
  await waitFor(() => expect(choiceOf().path).toBe("C:/synthetic-folder"));
  expect(document.activeElement).toBe(composer);
});

test("the open import dialog can restore its own source trigger", async () => {
  const pending = deferred<string | null>();
  render(<Harness />);
  await openSource();
  const dialog = document.querySelector("[data-import-dialog]") as HTMLElement;
  const stop = installDialogFocus(() => dialog, { initialFocus: "none" });
  try {
    const trigger = await beginPicker("sessionImport.chooseRoot", () => {
      mocks.pickFolder.mockReturnValueOnce(pending.promise);
    });
    await act(async () => { pending.resolve(null); });
    await waitFor(() => expect(trigger.disabled).toBe(false));
    expect(document.activeElement).toBe(trigger);
  } finally {
    stop();
  }
});

test("a newer topmost dialog drops the claim instead of focusing the source trigger later", async () => {
  const pending = deferred<string | null>();
  render(<Harness />);
  await openSource();
  const trigger = await beginPicker("sessionImport.chooseRoot", () => {
    mocks.pickFolder.mockReturnValueOnce(pending.promise);
  });
  const overlay = document.createElement("div");
  const nested = document.createElement("button");
  nested.type = "button";
  nested.textContent = "Nested action";
  overlay.append(nested);
  document.body.append(overlay);
  const stop = installDialogFocus(() => overlay, { initialFocus: "none" });
  try {
    expect(document.activeElement).not.toBe(trigger);
    await act(async () => { pending.resolve(null); });
    await waitFor(() => expect(trigger.disabled).toBe(false));
    expect(document.activeElement).not.toBe(trigger);
    expect(mocks.startScan).not.toHaveBeenCalled();
  } finally {
    stop();
    overlay.remove();
  }
  fireEvent.click(buttonNamed("Ping"));
  expect(document.activeElement).not.toBe(trigger);
});

function FileEntryHarness() {
  const ui = useSessionImportUi();
  const [ping, setPing] = useState(0);
  const sourceOpen = ui.open && ui.step === "source" && !ui.workspaceUi.mode;
  return (
    <>
      <button type="button" data-file-entry="settings" onClick={() => openSessionImport({ mode: "file" })}>
        Import a file
      </button>
      <button type="button" onClick={() => ui.setOpen(true)}>Show import</button>
      <button type="button" onClick={() => ui.setOpen(false)}>Hide import</button>
      <button type="button" onClick={() => ui.setChoice(choiceFromDefaultRoot("claude-code", "C:/switched"))}>
        Switch source
      </button>
      <button type="button" onClick={() => setPing((value) => value + 1)}>Ping</button>
      <span data-ping={ping} />
      <textarea aria-label="composer" />
      <GlassModal open={ui.open} onClose={() => ui.setOpen(false)} title="Import" closeLabel="Close">
        {sourceOpen ? (
          <SessionImportSourceStep
            tr={tr}
            roots={ui.roots}
            registrations={ui.registrations}
            choice={ui.choice}
            busy={ui.busy}
            onChoose={ui.setChoice}
            onPickFolder={(provider) => ui.pickCustomRoot(provider)}
            onPickFile={() => ui.pickCustomFile()}
            onPickWorkspaceFolder={() => ui.pickWorkspaceFolder()}
            onRelocate={(reg) => ui.relocateRegistered(reg)}
            onScan={() => ui.scan()}
            fileFlight={ui.filePickerFlight}
          />
        ) : null}
        {ui.workspaceUi.mode ? <div data-workspace-mode={ui.workspaceUi.mode} /> : null}
      </GlassModal>
      {ui.error ? <p role="alert">{ui.error}</p> : null}
      <output data-choice-kind={ui.choice?.kind ?? ""} data-choice-path={ui.choice?.path ?? ""} />
    </>
  );
}

function settingsFileEntry() {
  return document.querySelector("[data-file-entry='settings']") as HTMLButtonElement;
}

async function beginExplicitFile(arm: () => void) {
  arm();
  const entry = settingsFileEntry();
  entry.focus();
  fireEvent.click(entry);
  const trigger = await screen.findByRole("button", { name: "sessionImport.fileMode" }) as HTMLButtonElement;
  await waitFor(() => expect(trigger.disabled).toBe(true));
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
  expect(document.activeElement).not.toBe(entry);
  loseFocus(trigger);
  expect(document.activeElement).not.toBe(trigger);
  expect(document.activeElement).not.toBe(entry);
  return { trigger, entry };
}

test("cancelling an explicit file entry restores the in-modal file trigger and does not scan", async () => {
  const pending = deferred<string[]>();
  render(<FileEntryHarness />);
  const { trigger, entry } = await beginExplicitFile(() => {
    mocks.pickFiles.mockReturnValueOnce(pending.promise);
  });
  await act(async () => { pending.resolve([]); });
  await waitFor(() => expect(trigger.disabled).toBe(false));
  expect(document.activeElement).toBe(trigger);
  expect(document.activeElement).not.toBe(entry);
  expect(choiceOf()).toEqual({ kind: "", path: "" });
  expect(mocks.startScan).not.toHaveBeenCalled();
  expect(mocks.workspacePreview).not.toHaveBeenCalled();
});

test("an explicit file error restores the in-modal file trigger and keeps the error", async () => {
  const pending = deferred<string[]>();
  render(<FileEntryHarness />);
  const { trigger, entry } = await beginExplicitFile(() => {
    mocks.pickFiles.mockReturnValueOnce(pending.promise);
  });
  await act(async () => { pending.reject(new Error("file denied")); });
  await waitFor(() => expect(trigger.disabled).toBe(false));
  expect(document.activeElement).toBe(trigger);
  expect(document.activeElement).not.toBe(entry);
  expect(screen.getByRole("alert").textContent).toBe("Error: file denied");
  expect(choiceOf().path).toBe("");
  expect(mocks.startScan).not.toHaveBeenCalled();
});

test("an explicit ordinary transcript restores the in-modal file trigger without scanning", async () => {
  const pending = deferred<string[]>();
  render(<FileEntryHarness />);
  const { trigger, entry } = await beginExplicitFile(() => {
    mocks.pickFiles.mockReturnValueOnce(pending.promise);
  });
  await act(async () => { pending.resolve(["C:/export.json"]); });
  await waitFor(() => expect(choiceOf()).toEqual({ kind: "custom-file", path: "C:/export.json" }));
  expect(document.activeElement).toBe(trigger);
  expect(document.activeElement).not.toBe(entry);
  expect(mocks.workspacePreview).not.toHaveBeenCalled();
  expect(mocks.startScan).not.toHaveBeenCalled();
});

test("an explicit workspace file unmounts the source step without focusing the covered entry or composer", async () => {
  const pending = deferred<string[]>();
  mocks.workspacePreview.mockResolvedValue(workspaceFixture());
  render(<FileEntryHarness />);
  const composer = screen.getByRole("textbox", { name: "composer" });
  const { entry } = await beginExplicitFile(() => {
    mocks.pickFiles.mockReturnValueOnce(pending.promise);
  });
  await act(async () => { pending.resolve(["C:/empty.code-workspace"]); });
  await waitFor(() => expect(document.querySelector("[data-workspace-mode]")).toBeTruthy());
  expect(screen.queryByRole("button", { name: "sessionImport.fileMode" })).toBeNull();
  expect(document.activeElement).not.toBe(composer);
  expect(document.activeElement).not.toBe(entry);
  expect(choiceOf().path).toBe("");
  expect(mocks.workspacePreview).toHaveBeenCalledWith({ path: "C:/empty.code-workspace", kind: "file" });
  expect(mocks.startScan).not.toHaveBeenCalled();
});

test("closing an explicit file entry ignores the late file and the reopened cancel restores the new trigger", async () => {
  const first = deferred<string[]>();
  render(<FileEntryHarness />);
  const { entry } = await beginExplicitFile(() => {
    mocks.pickFiles.mockReturnValueOnce(first.promise);
  });
  const hide = buttonNamed("Hide import");
  hide.focus();
  fireEvent.click(hide);
  expect(screen.queryByRole("button", { name: "sessionImport.fileMode" })).toBeNull();
  const composer = screen.getByRole("textbox", { name: "composer" });
  composer.focus();
  await act(async () => { first.resolve(["C:/after-close.json"]); });
  expect(document.activeElement).toBe(composer);
  expect(document.activeElement).not.toBe(entry);
  expect(choiceOf().path).toBe("");
  expect(mocks.startScan).not.toHaveBeenCalled();
  fireEvent.click(buttonNamed("Show import"));
  await waitFor(() => expect(buttonNamed("sessionImport.fileMode").disabled).toBe(false));
  const second = deferred<string[]>();
  const trigger = await beginPicker("sessionImport.fileMode", () => {
    mocks.pickFiles.mockReturnValueOnce(second.promise);
  });
  await act(async () => { second.resolve([]); });
  await waitFor(() => expect(trigger.disabled).toBe(false));
  expect(document.activeElement).toBe(trigger);
  expect(document.activeElement).not.toBe(entry);
  expect(choiceOf().path).toBe("");
  expect(mocks.startScan).not.toHaveBeenCalled();
});

test("a newer explicit file event keeps its trigger when the older picker settles first", async () => {
  // Event contract. The in-modal file control is disabled while the first
  // picker is busy, and the settings entry stays covered by the open modal.
  const first = deferred<string[]>();
  const second = deferred<string[]>();
  render(<FileEntryHarness />);
  const { trigger, entry } = await beginExplicitFile(() => {
    mocks.pickFiles
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);
  });
  fireEvent.click(entry);
  await waitFor(() => expect(trigger.disabled).toBe(true));
  loseFocus(trigger);
  expect(document.activeElement).not.toBe(entry);
  await act(async () => { first.resolve(["C:/old.json"]); });
  expect(trigger.disabled).toBe(true);
  expect(choiceOf()).toEqual({ kind: "", path: "" });
  expect(document.activeElement).not.toBe(trigger);
  expect(document.activeElement).not.toBe(entry);
  expect(mocks.startScan).not.toHaveBeenCalled();
  await act(async () => { second.resolve([]); });
  await waitFor(() => expect(trigger.disabled).toBe(false));
  expect(choiceOf()).toEqual({ kind: "", path: "" });
  expect(document.activeElement).toBe(trigger);
  expect(document.activeElement).not.toBe(entry);
  expect(mocks.workspacePreview).not.toHaveBeenCalled();
});

test("an explicit file flight does not regain focus when source intent changes without a new claim", async () => {
  // Hook contract, not a reachable click. Source controls are disabled while
  // pickerBusy. Switch source only invalidates this flight and leaves focus lost.
  const pending = deferred<string[]>();
  render(<FileEntryHarness />);
  const { trigger, entry } = await beginExplicitFile(() => {
    mocks.pickFiles.mockReturnValueOnce(pending.promise);
  });
  fireEvent.click(buttonNamed("Switch source"));
  expect(choiceOf()).toEqual({ kind: "default", path: "C:/switched" });
  expect(trigger.isConnected).toBe(true);
  loseFocus(trigger);
  expect(document.activeElement).not.toBe(trigger);
  await act(async () => { pending.resolve(["C:/from-old.json"]); });
  await waitFor(() => expect(trigger.disabled).toBe(false));
  expect(choiceOf()).toEqual({ kind: "default", path: "C:/switched" });
  expect(document.activeElement).not.toBe(trigger);
  expect(document.activeElement).not.toBe(entry);
  expect(mocks.startScan).not.toHaveBeenCalled();
  expect(mocks.workspacePreview).not.toHaveBeenCalled();
});

test("an explicit file entry does not steal focus from Close or the composer", async () => {
  const pending = deferred<string[]>();
  render(<FileEntryHarness />);
  const entry = settingsFileEntry();
  entry.focus();
  mocks.pickFiles.mockReturnValueOnce(pending.promise);
  fireEvent.click(entry);
  const trigger = await screen.findByRole("button", { name: "sessionImport.fileMode" }) as HTMLButtonElement;
  await waitFor(() => expect(trigger.disabled).toBe(true));
  await act(async () => { await new Promise((resolve) => setTimeout(resolve, 0)); });
  const close = buttonNamed("Close");
  expect(document.activeElement).toBe(close);
  expect(document.activeElement).not.toBe(entry);
  await act(async () => { pending.resolve([]); });
  await waitFor(() => expect(trigger.disabled).toBe(false));
  expect(document.activeElement).toBe(close);
  expect(document.activeElement).not.toBe(entry);
  const again = deferred<string[]>();
  await beginPicker("sessionImport.fileMode", () => {
    mocks.pickFiles.mockReturnValueOnce(again.promise);
  });
  const composer = screen.getByRole("textbox", { name: "composer" });
  composer.focus();
  await act(async () => { again.resolve(["C:/export.json"]); });
  await waitFor(() => expect(choiceOf().path).toBe("C:/export.json"));
  expect(document.activeElement).toBe(composer);
  expect(document.activeElement).not.toBe(entry);
  expect(mocks.startScan).not.toHaveBeenCalled();
});

test("a newer dialog during an explicit file entry does not focus the file trigger later", async () => {
  const pending = deferred<string[]>();
  render(<FileEntryHarness />);
  const { trigger, entry } = await beginExplicitFile(() => {
    mocks.pickFiles.mockReturnValueOnce(pending.promise);
  });
  const overlay = document.createElement("div");
  const nested = document.createElement("button");
  nested.type = "button";
  nested.textContent = "Nested action";
  overlay.append(nested);
  document.body.append(overlay);
  const stop = installDialogFocus(() => overlay, { initialFocus: "none" });
  try {
    await act(async () => { pending.resolve([]); });
    await waitFor(() => expect(trigger.disabled).toBe(false));
    expect(document.activeElement).not.toBe(trigger);
    expect(document.activeElement).not.toBe(entry);
    expect(mocks.startScan).not.toHaveBeenCalled();
  } finally {
    stop();
    overlay.remove();
  }
  fireEvent.click(buttonNamed("Ping"));
  expect(document.activeElement).not.toBe(trigger);
  expect(document.activeElement).not.toBe(entry);
});
