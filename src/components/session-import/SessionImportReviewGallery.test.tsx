/** @vitest-environment jsdom */
import "@testing-library/jest-dom/vitest";
import { StrictMode } from "react";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { createT } from "@/i18n";

const { get, save, live } = vi.hoisted(() => ({
  live: vi.fn(() => { throw new Error("live import call"); }),
  get: vi.fn(() => {
    throw new Error("live continuation get");
  }),
  save: vi.fn(() => {
    throw new Error("live continuation save");
  }),
}));

vi.mock("@/lib/nativeWebviewCover", () => ({
  acquireNativeWebviewCover: () => () => {}, rectOverlapsNativeWebviewHost: () => false,
}));
vi.mock("@/components/MarkdownBody", () => ({
  MarkdownBody: ({ children }: { children: string }) => <div className="md-body">{children}</div>,
}));
vi.mock("@/lib/api/sessionMigration", () => ({
  isImportedHistoryId: (id: string) => id.startsWith("imp:"),
  sessionImportPreview: live,
  sessionImportPreviewEvents: live,
  sessionImportEvents: live,
  sessionImportEventContext: live,
  sessionImportSearch: live,
  sessionImportExportMarkdown: live,
  sessionImportContinuationGet: () => get(),
  sessionImportContinuationSave: () => save(),
}));
vi.mock("@/lib/api/workspace", () => ({ workspacesList: live }));
// Gallery clients own synthetic data. Fail closed if a new control reaches
// a native command or picker instead of those clients.
vi.mock("@/lib/api/host", () => ({ invoke: live }));
vi.mock("@/lib/api", () => ({
  pickDirectory: live,
  sessionImportContinuePrepare: live, sessionImportContinueWorkspace: live,
  sessionImportContinueSelection: live, sessionImportContinueRestore: live,
  sessionImportContinueDiscard: live, sessionImportContinueCreate: live,
}));

import { SessionImportReviewGallery } from "./SessionImportReviewGallery";

const tr = createT("en");
const originalScroll = Object.getOwnPropertyDescriptor(Element.prototype, "scrollIntoView");
beforeEach(() => {
  Object.defineProperty(Element.prototype, "scrollIntoView", { configurable: true, value: vi.fn() });
});

afterEach(() => {
  cleanup();
  window.history.replaceState(null, "", "/");
  get.mockClear();
  save.mockClear();
  live.mockClear();
  if (originalScroll) Object.defineProperty(Element.prototype, "scrollIntoView", originalScroll);
  else Reflect.deleteProperty(Element.prototype, "scrollIntoView");
});

test("synthetic selection, filtering, preview and message paging use production interactions", async () => {
  const user = userEvent.setup();
  render(<SessionImportReviewGallery />);
  const dialog = await screen.findByRole("dialog");
  await user.click(within(dialog).getByRole("button", { name: "Clear selection" }));
  expect(within(dialog).getByRole("button", { name: "Next" })).toBeDisabled();
  await user.type(within(dialog).getByRole("searchbox", { name: "Filter this page…" }), "Synthetic archived");
  await user.click(within(dialog).getByRole("button", { name: "Select visible" }));
  expect(within(dialog).getByRole("button", { name: "Next" })).toBeEnabled();
  await user.click(within(dialog).getByRole("button", { name: "Preview" }));
  expect(await within(dialog).findByText("Synthetic user note 🙂 锚点")).toBeInTheDocument();
  expect(within(dialog).getByText(/Synthetic \*\*assistant\*\* note/)).toBeInTheDocument();
  await user.click(within(dialog).getByRole("button", { name: "Load more" }));
  expect(await within(dialog).findByText("Synthetic follow-up. Not sent.")).toBeInTheDocument();
  expect(live).not.toHaveBeenCalled();
});

test("prepare retry and mapping lead to a synthetic import result", async () => {
  const user = userEvent.setup();
  window.history.replaceState(null, "", "/gallery.html?surface=prepare");
  render(<SessionImportReviewGallery />);
  const dialog = await screen.findByRole("dialog");
  await user.click(within(dialog).getByRole("button", { name: tr("sessionImport.retry") }));
  await user.click(within(dialog).getByRole("button", { name: tr("sessionImport.prepare.continue") }));
  expect(document.querySelector("[data-import-mapping='1']")).toBeInTheDocument();
  await user.click(within(dialog).getByRole("button", { name: tr("sessionImport.importSelected") }));
  expect(document.querySelector("[data-review-surface='import']")).toBeInTheDocument();
  await waitFor(() => expect(document.querySelector("[data-review-surface='result']")).toBeInTheDocument(), { timeout: 2500 });
  expect(live).not.toHaveBeenCalled();
});

test("reader searches across pages and reveals a hit using the actual reader hook", async () => {
  const user = userEvent.setup();
  window.history.replaceState(null, "", "/gallery.html?surface=reader");
  render(<SessionImportReviewGallery />);
  const dialog = await screen.findByRole("dialog");
  await within(dialog).findByText("Synthetic user note 🙂 锚点");
  await user.type(within(dialog).getByRole("searchbox"), "second page");
  await user.click(within(dialog).getByRole("button", { name: "Search" }));
  const hit = await within(dialog).findByRole("button", { name: "Message text Synthetic second page 🙂 锚点" });
  await waitFor(() => expect(JSON.parse(document.getElementById("session-import-review-metrics")!.textContent!).observedSearchState).toBe("hits"));
  await user.click(hit);
  await waitFor(() => expect(document.getElementById("import-ev-reader-3")).toHaveAttribute("data-highlight", "1"));
  await user.click(within(dialog).getByRole("button", { name: "Clear" }));
  expect(within(dialog).getByRole("searchbox")).toHaveValue("");
  await waitFor(() => expect(JSON.parse(document.getElementById("session-import-review-metrics")!.textContent!).observedSearchState).toBe("idle"));
  expect(live).not.toHaveBeenCalled();
});

test("saved context edits persist only in the synthetic client and can be reviewed again", async () => {
  const user = userEvent.setup();
  window.history.replaceState(null, "", "/gallery.html?surface=saved-context&review=1");
  render(<SessionImportReviewGallery />);
  const editor = await screen.findByRole("textbox");
  await user.clear(editor);
  await user.type(editor, "Synthetic revised handoff");
  const review = editor.closest("[role='dialog']") as HTMLElement;
  await user.click(within(review).getByRole("button", { name: "Save" }));
  await waitFor(() => expect(screen.queryByRole("textbox")).not.toBeInTheDocument());
  await user.click(screen.getByRole("button", { name: tr("sessionImport.continuationReview") }));
  expect(await screen.findByRole("textbox")).toHaveValue("Synthetic revised handoff");
  expect(get).not.toHaveBeenCalled(); expect(save).not.toHaveBeenCalled(); expect(live).not.toHaveBeenCalled();
});

test("the review gallery renders a synthetic content composition", async () => {
  render(<SessionImportReviewGallery />);
  expect(await screen.findByRole("dialog")).toBeTruthy();
  expect(document.querySelector("[data-review-surface='content']")).toBeTruthy();
  expect(document.querySelector("[data-review-gate='synthetic']")).toBeTruthy();
  expect(screen.getByText("Synthetic design preview. These records are not imported.")).toBeTruthy();
  expect(get).not.toHaveBeenCalled();
  expect(save).not.toHaveBeenCalled();
});

test("continuation conflict stays on the synthetic client", async () => {
  window.history.replaceState(null, "", "/gallery.html?surface=saved-context&conflict=1&locale=de");
  render(<SessionImportReviewGallery />);
  expect(await screen.findByRole("alert")).toBeTruthy();
  expect(document.querySelector("[data-review-surface='saved-context']")).toBeTruthy();
  expect(get).not.toHaveBeenCalled();
  expect(save).not.toHaveBeenCalled();
});

test("catalog filters, opens Reader and reports export failure without a clipboard write", async () => {
  const user = userEvent.setup();
  const clipboard = vi.spyOn(navigator.clipboard, "writeText");
  window.history.replaceState(null, "", "/gallery.html?surface=catalog");
  render(<SessionImportReviewGallery />);
  const dialog = await screen.findByRole("dialog");
  const list = within(dialog);
  await user.click(list.getByRole("button", { name: tr("sessionImport.source.claude") }));
  expect(list.queryByRole("button", { name: /Synthetic session 🙂/ })).toBeNull();
  expect(list.getByRole("button", { name: /Synthetic archived session/ })).toBeInTheDocument();
  await user.click(list.getByRole("button", { name: tr("sessionImport.providerAll") }));
  await user.type(list.getByRole("searchbox"), "🙂");
  expect(list.queryByRole("button", { name: /Synthetic archived session/ })).toBeNull();
  await user.click(list.getByRole("button", { name: tr("sessionImport.exportMarkdown") }));
  expect(await list.findByRole("alert")).toHaveTextContent(tr("sessionImport.err.generic"));
  expect(clipboard).not.toHaveBeenCalled();
  await user.click(list.getByRole("button", { name: /Synthetic session 🙂/ }));
  expect(await screen.findByText("Synthetic user note 🙂 锚点")).toBeInTheDocument();
  expect(live).not.toHaveBeenCalled();
  clipboard.mockRestore();
});

test("workspace association can be selected, saved and cleared through its real controls", async () => {
  const user = userEvent.setup();
  window.history.replaceState(null, "", "/gallery.html?surface=association");
  render(<SessionImportReviewGallery />);
  const dialog = await screen.findByRole("dialog");
  const list = within(dialog);
  await user.click(list.getByText(tr("sessionImport.association.title"), { selector: "summary" }));
  await user.click(list.getByRole("button", { name: tr("sessionImport.association.title") }));
  await user.click(within(await screen.findByRole("option", { name: /Synthetic workspace/ })).getByRole("button"));
  await user.click(list.getByRole("button", { name: tr("common.save") }));
  expect(await list.findByRole("status")).toHaveTextContent(tr("sessionImport.association.saved"));
  expect(list.getByText(/Synthetic workspace/, { selector: "summary" })).toBeInTheDocument();
  await user.click(list.getByRole("button", { name: tr("sessionImport.association.clear") }));
  expect(list.queryByRole("button", { name: tr("sessionImport.association.clear") })).toBeNull();
  expect(list.getByText(tr("sessionImport.association.title"), { selector: "summary" })).toBeInTheDocument();
  expect(live).not.toHaveBeenCalled();
});

test("continuation chooses a workspace, narrows scope, edits and creates saved context without sending", async () => {
  const user = userEvent.setup();
  window.history.replaceState(null, "", "/gallery.html?surface=continuation");
  render(<SessionImportReviewGallery />);
  const dialog = await screen.findByRole("dialog", { name: tr("sessionImport.handoffTitle") });
  const list = within(dialog);
  const workspace = list.getByRole("button", { name: tr("sessionImport.handoffWorkspace") });
  await waitFor(() => expect(workspace).toBeEnabled());
  await user.click(workspace);
  expect(await screen.findByRole("option", { name: "Synthetic blocked workspace" })).toHaveAttribute("aria-disabled", "true");
  await user.click(within(screen.getByRole("option", { name: "Synthetic execution workspace" })).getByRole("button"));
  await list.findByText(/reference/, { selector: "code" });
  await user.click(list.getByRole("button", { name: tr("sessionImport.scopeReview") }));
  const events = list.getByRole("region", { name: tr("sessionImport.scopeEvents") });
  await user.click(within(events).getByRole("checkbox", { name: "reader-3" }));
  await user.click(within(events).getAllByRole("button", { name: tr("sessionImport.preview") })[0]);
  expect(await list.findByText("Synthetic user note 🙂 锚点")).toBeInTheDocument();
  await user.click(list.getByRole("button", { name: tr("sessionImport.scopeApply") }));
  const editor = list.getByRole("textbox", { name: tr("sessionImport.handoffEdit") });
  expect(editor).toHaveValue("Synthetic handoff for reader-1, reader-2. Not sent.");
  await user.clear(editor); await user.type(editor, "Synthetic edited creation. Not sent.");
  const create = list.getByRole("button", { name: tr("sessionImport.createDraft") });
  expect(create.closest(".modal-body")).toBeNull();
  await user.click(create);
  await screen.findByRole("button", { name: tr("sessionImport.continuationReview") });
  await user.click(screen.getByRole("button", { name: tr("sessionImport.continuationReview") }));
  expect(await screen.findByRole("textbox")).toHaveValue("Synthetic edited creation. Not sent.");
  expect(get).not.toHaveBeenCalled(); expect(save).not.toHaveBeenCalled(); expect(live).not.toHaveBeenCalled();
});

test("a failed creation stays locked across hide and reopen, then retries the same local request", async () => {
  const user = userEvent.setup();
  window.history.replaceState(null, "", "/gallery.html?surface=continuation&creation=retry");
  render(<SessionImportReviewGallery />);
  const dialog = await screen.findByRole("dialog", { name: tr("sessionImport.handoffTitle") });
  await user.click(within(dialog).getByRole("button", { name: tr("sessionImport.createDraft") }));
  await within(dialog).findByRole("alert");
  expect(within(dialog).getByRole("textbox", { name: tr("sessionImport.handoffEdit") })).toBeDisabled();
  await user.click(within(dialog).getByRole("button", { name: tr("sessionImport.close") }));
  await user.click(screen.getByRole("button", { name: tr("sessionImport.handoffTitle") }));
  const reopened = await screen.findByRole("dialog");
  expect(within(reopened).getByRole("textbox", { name: tr("sessionImport.handoffEdit") })).toBeDisabled();
  await user.click(within(reopened).getByRole("button", { name: tr("sessionImport.createDraft") }));
  await screen.findByRole("button", { name: tr("sessionImport.continuationReview") });
  expect(live).not.toHaveBeenCalled();
});

test("a cold continuation under the actual StrictMode entry initializes and remains editable", async () => {
  window.history.replaceState(null, "", "/gallery.html?surface=continuation");
  render(<StrictMode><SessionImportReviewGallery /></StrictMode>);
  const dialog = await screen.findByRole("dialog", { name: tr("sessionImport.handoffTitle") });
  const editor = within(dialog).getByRole("textbox", { name: tr("sessionImport.handoffEdit") });
  expect(editor).toBeEnabled();
  expect(editor).toHaveValue("Synthetic continuation draft. Not sent. 🙂 锚点");
  expect(live).not.toHaveBeenCalled();
});

test("opening a Claude catalog row retains its history identity through Reader and continuation", async () => {
  const user = userEvent.setup();
  window.history.replaceState(null, "", "/gallery.html?surface=catalog");
  render(<SessionImportReviewGallery />);
  const dialog = await screen.findByRole("dialog");
  await user.click(within(dialog).getByRole("button", { name: /Synthetic archived session/ }));
  const reader = await screen.findByRole("dialog", { name: "Synthetic archived session" });
  await within(reader).findByText("Synthetic user note 🙂 锚点");
  await user.click(within(reader).getByRole("button", { name: tr("sessionImport.continueInGrok") }));
  const continuation = await screen.findByRole("dialog", { name: tr("sessionImport.handoffTitle") });
  await user.click(within(continuation).getByRole("button", { name: tr("sessionImport.createDraft") }));
  expect(document.querySelector("[data-review-surface='saved-context']")).toBeInTheDocument();
  await user.click(await screen.findByRole("button", { name: tr("sessionImport.continuationHistory") }));
  expect(await screen.findByRole("dialog", { name: "Synthetic archived session" })).toBeInTheDocument();
  expect(live).not.toHaveBeenCalled();
});
