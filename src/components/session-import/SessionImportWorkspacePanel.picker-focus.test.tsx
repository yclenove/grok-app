/** @vitest-environment jsdom */
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { GlassModal } from "@/components/GlassModal";
import { useWorkspaceImport, type WorkspaceImportClient } from "@/hooks/useWorkspaceImport";
import { createT } from "@/i18n";
import type { WorkspaceImportPreview } from "@/lib/api/sessionMigration";
import { installDialogFocus } from "@/lib/a11yFocus";
import { SessionImportWorkspacePanel } from "./SessionImportWorkspacePanel";

afterEach(cleanup);

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (cause: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

function fixture(): WorkspaceImportPreview {
  return {
    source: { kind: "file", path: "/synthetic.code-workspace" },
    fingerprint: "synthetic-fingerprint", savedRevision: null,
    workspace: {
      sourceWorkspaceId: "ws:synthetic", displayName: "Synthetic workspace", evidence: [],
      roots: ["one", "two"].map((rootId) => ({
        rootId, originalPath: `/${rootId}`, pathSyntax: "posix", host: "local",
        availability: "missing", evidence: [],
      })),
    },
  };
}

function makeClient(): WorkspaceImportClient {
  return {
    sessionImportWorkspacePreview: vi.fn().mockResolvedValue(fixture()),
    sessionImportWorkspaceSave: vi.fn(), sessionImportWorkspaces: vi.fn(),
    sessionImportWorkspaceGet: vi.fn(), sessionImportWorkspaceUpdate: vi.fn(),
    sessionImportWorkspaceApply: vi.fn(), sessionImportProjectCreate: vi.fn(),
    pickDirectory: vi.fn(),
  };
}

function Harness({ client }: { client: WorkspaceImportClient }) {
  const ui = useWorkspaceImport(undefined, client);
  const [footer, setFooter] = useState<HTMLDivElement | null>(null);
  return <>
    <input aria-label="composer" />
    <button onClick={() => void ui.load(fixture().source)}>Open workspace</button>
    <GlassModal open={!!ui.mode} title="Import workspace" onClose={ui.reset}
      initialFocus="none" closeLabel="Close import" footer={<div ref={setFooter} />}>
      {ui.mode ? <SessionImportWorkspacePanel ui={ui} tr={createT("en")}
        projects={[]} footerTarget={footer} /> : null}
    </GlassModal>
  </>;
}

async function beginPicker(client: WorkspaceImportClient) {
  fireEvent.click(screen.getByRole("button", { name: "Open workspace" }));
  const trigger = await screen.findByRole<HTMLButtonElement>("button", { name: "Choose target folder…: /one" });
  await waitFor(() => expect(trigger.disabled).toBe(false));
  trigger.focus();
  fireEvent.click(trigger);
  expect(client.pickDirectory).toHaveBeenCalledTimes(1);
  expect(trigger.disabled).toBe(true);
  // jsdom retains a disabled button as active. Native WebViews drop its focus.
  act(() => {
    document.body.tabIndex = -1;
    document.body.focus();
    document.body.removeAttribute("tabindex");
  });
  expect(document.activeElement).toBe(document.body);
  return trigger;
}

test.each(["success", "cancel", "failure"] as const)(
  "a %s mapping picker returns focus to its re-enabled trigger before the next Tab",
  async (outcome) => {
    const client = makeClient();
    const pending = deferred<string | null>();
    vi.mocked(client.pickDirectory).mockReturnValueOnce(pending.promise);
    render(<Harness client={client} />);
    const trigger = await beginPicker(client);
    await act(async () => {
      if (outcome === "failure") pending.reject(new Error("synthetic picker failure"));
      else pending.resolve(outcome === "success" ? "/synthetic target" : null);
    });
    await waitFor(() => expect(trigger.disabled).toBe(false));
    expect(document.activeElement).toBe(trigger);
    await userEvent.tab();
    expect(document.activeElement).not.toBe(screen.getByRole("button", { name: "Close import" }));
    if (outcome === "success") {
      expect(document.activeElement).toBe(screen.getByRole("button", { name: "Create project: /one" }));
    } else {
      expect(screen.queryByRole("button", { name: "Create project: /one" })).toBeNull();
    }
    if (outcome === "failure") expect(screen.getByRole("alert").textContent).toBeTruthy();
    expect(client.sessionImportWorkspaceSave).not.toHaveBeenCalled();
    expect(client.sessionImportProjectCreate).not.toHaveBeenCalled();
  },
);

test("a deliberate focus move keeps ownership when the mapping picker finishes", async () => {
  const client = makeClient();
  const pending = deferred<string | null>();
  vi.mocked(client.pickDirectory).mockReturnValueOnce(pending.promise);
  render(<Harness client={client} />);
  const trigger = await beginPicker(client);
  const close = screen.getByRole("button", { name: "Close import" });
  close.focus();
  await act(async () => pending.resolve("/synthetic target"));
  await waitFor(() => expect(trigger.disabled).toBe(false));
  expect(document.activeElement).toBe(close);
});

test("a newer dialog keeps focus ownership even when its focus is lost", async () => {
  const client = makeClient();
  const pending = deferred<string | null>();
  vi.mocked(client.pickDirectory).mockReturnValueOnce(pending.promise);
  render(<Harness client={client} />);
  const trigger = await beginPicker(client);
  const overlay = document.createElement("div");
  overlay.setAttribute("role", "dialog");
  document.body.append(overlay);
  const release = installDialogFocus(() => overlay, { initialFocus: "none", restoreFocus: false });
  try {
    await act(async () => pending.resolve("/synthetic target"));
    await waitFor(() => expect(trigger.disabled).toBe(false));
    expect(document.activeElement).toBe(document.body);
    expect(document.activeElement).not.toBe(screen.getByRole("button", { name: "Save" }));
  } finally {
    release();
    overlay.remove();
  }
});

test.each(["resolve", "reject"] as const)("a removed mapping picker cannot reclaim focus on late %s", async (outcome) => {
  const client = makeClient();
  const pending = deferred<string | null>();
  vi.mocked(client.pickDirectory).mockReturnValueOnce(pending.promise);
  render(<Harness client={client} />);
  const trigger = await beginPicker(client);
  fireEvent.click(screen.getByRole("button", { name: "Back" }));
  expect(trigger.isConnected).toBe(false);
  const composer = screen.getByRole("textbox", { name: "composer" });
  composer.focus();
  await act(async () => {
    if (outcome === "reject") pending.reject(new Error("late synthetic failure"));
    else pending.resolve("/late target");
  });
  expect(document.activeElement).toBe(composer);
  expect(screen.queryByRole("alert")).toBeNull();
  expect(client.sessionImportWorkspaceSave).not.toHaveBeenCalled();
});

test("an old picker completion cannot settle a new mapping picker claim", async () => {
  const client = makeClient();
  const first = deferred<string | null>();
  const second = deferred<string | null>();
  vi.mocked(client.pickDirectory).mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
  render(<Harness client={client} />);
  const oldTrigger = await beginPicker(client);
  fireEvent.click(screen.getByRole("button", { name: "Back" }));
  fireEvent.click(screen.getByRole("button", { name: "Open workspace" }));
  const trigger = await screen.findByRole<HTMLButtonElement>("button", { name: "Choose target folder…: /one" });
  await waitFor(() => expect(trigger.disabled).toBe(false));
  trigger.focus();
  fireEvent.click(trigger);
  expect(trigger).not.toBe(oldTrigger);
  act(() => {
    document.body.tabIndex = -1;
    document.body.focus();
    document.body.removeAttribute("tabindex");
  });
  await act(async () => first.resolve("/late target"));
  expect(trigger.disabled).toBe(true);
  expect(document.activeElement).toBe(document.body);
  await act(async () => second.resolve("/current target"));
  await waitFor(() => expect(trigger.disabled).toBe(false));
  expect(document.activeElement).toBe(trigger);
  expect(screen.queryByText("/late target")).toBeNull();
  expect(screen.getByRole("button", { name: "Project mapping for /one: /current target" })).toBeTruthy();
});
