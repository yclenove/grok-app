/** @vitest-environment jsdom */
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { createT } from "@/i18n";
import type { ContinuationDraftView } from "@/lib/api/sessionMigration";
import { ContinuationDraftBanner } from "./ContinuationDraftBanner";

const { get, save, openHistory } = vi.hoisted(() => ({ get: vi.fn(), save: vi.fn(), openHistory: vi.fn() }));
vi.mock("@/lib/api/sessionMigration", () => ({ sessionImportContinuationGet: get, sessionImportContinuationSave: save }));
vi.mock("@/lib/sessionImport/open", () => ({ openImportedHistory: openHistory }));
vi.mock("@/lib/nativeWebviewCover", () => ({ acquireNativeWebviewCover: () => () => {} }));
const tr = createT("en");
const draft: ContinuationDraftView = {
  appSessionId: "native-a", importId: "imp:a", revisionId: "revision-a", leaf: "leaf-a",
  state: "prepared", text: "Saved synthetic context", textHash: "hash-a",
};
const props = { sessionId: "native-a", locale: "en" as const, empty: true, sessionState: "idle" };
beforeEach(() => { vi.clearAllMocks(); get.mockResolvedValue({ ...draft }); save.mockResolvedValue({ ...draft }); });
afterEach(cleanup);
async function review() { fireEvent.click(await screen.findByRole("button", { name: tr("sessionImport.continuationReview") })); }
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (cause: unknown) => void;
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}

test("restores durable context without changing composer or sending, and saves CAS edits", async () => {
  const composer = document.createElement("textarea");
  composer.value = "keep my new instruction";
  document.body.append(composer);
  render(<ContinuationDraftBanner {...props} />);
  expect(screen.queryByText(tr("automations.emptySession"))).toBeNull();
  await review();
  const editor = screen.getByRole("textbox", { name: tr("sessionImport.continuationReview") }) as HTMLTextAreaElement;
  expect(editor.value).toBe(draft.text);
  fireEvent.change(editor, { target: { value: "Revised context" } });
  save.mockResolvedValueOnce({ ...draft, text: "Revised context", textHash: "hash-b" });
  fireEvent.click(screen.getByRole("button", { name: tr("common.save") }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(save).toHaveBeenCalledExactlyOnceWith("native-a", "hash-a", "Revised context");
  expect(composer.value).toBe("keep my new instruction");
  await review();
  expect((screen.getByRole("textbox", { name: tr("sessionImport.continuationReview") }) as HTMLTextAreaElement).value).toBe("Revised context");
  composer.remove();
});

test("ordinary chats keep their empty state and do not expose draft controls", async () => {
  get.mockResolvedValue(null);
  render(<ContinuationDraftBanner {...props} />);
  await screen.findByText(tr("automations.emptySession"));
  expect(screen.queryByRole("button")).toBeNull();
  expect(save).not.toHaveBeenCalled();
});

test("read failures are visible and retryable, not mistaken for ordinary empty chats", async () => {
  get.mockRejectedValueOnce(new Error("unavailable"));
  render(<ContinuationDraftBanner {...props} />);
  await screen.findByRole("alert");
  expect(screen.queryByText(tr("automations.emptySession"))).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: tr("sessionImport.refresh") }));
  await screen.findByRole("button", { name: tr("sessionImport.continuationReview") });
  expect(get).toHaveBeenCalledTimes(2);
});

test("failed save retains text and original CAS hash until an explicit discard-and-reload", async () => {
  save.mockRejectedValue("HANDOFF_CHANGED: another window saved");
  const { rerender } = render(<ContinuationDraftBanner {...props} />);
  await review();
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "Unsaved work" } });
  fireEvent.click(screen.getByRole("button", { name: tr("common.save") }));
  await screen.findByRole("alert");
  expect((screen.getByRole("textbox") as HTMLTextAreaElement).value).toBe("Unsaved work");
  get.mockResolvedValue({ ...draft, text: "Other window", textHash: "hash-b" });
  rerender(<ContinuationDraftBanner {...props} sessionState="ready" />);
  await waitFor(() => expect(get).toHaveBeenCalledTimes(2));
  await waitFor(() => expect((screen.getByRole("button", { name: tr("common.save") }) as HTMLButtonElement).disabled).toBe(false));
  fireEvent.click(screen.getByRole("button", { name: tr("common.save") }));
  await waitFor(() => expect(save).toHaveBeenCalledTimes(2));
  expect(save.mock.calls[1]).toEqual(["native-a", "hash-a", "Unsaved work"]);
  await waitFor(() => expect((screen.getByRole("button", { name: tr("sessionImport.continuationReload") }) as HTMLButtonElement).disabled).toBe(false));
  fireEvent.click(screen.getByRole("button", { name: tr("sessionImport.continuationReload") }));
  await waitFor(() => expect((screen.getByRole("textbox") as HTMLTextAreaElement).value).toBe("Other window"));
  save.mockResolvedValue({ ...draft, text: "My merged work", textHash: "hash-c" });
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "My merged work" } });
  fireEvent.click(screen.getByRole("button", { name: tr("common.save") }));
  await waitFor(() => expect(save).toHaveBeenLastCalledWith("native-a", "hash-b", "My merged work"));
});

test("switching session rejects a late context read and never shows the previous session", async () => {
  const pending = deferred<ContinuationDraftView>();
  get.mockReturnValueOnce(pending.promise).mockResolvedValueOnce({ ...draft, appSessionId: "native-b", text: "B only", textHash: "hash-b" });
  const { rerender } = render(<ContinuationDraftBanner {...props} />);
  rerender(<ContinuationDraftBanner {...props} sessionId="native-b" />);
  await review();
  await act(async () => pending.resolve(draft));
  expect((screen.getByRole("textbox") as HTMLTextAreaElement).value).toBe("B only");
  fireEvent.click(screen.getByRole("button", { name: tr("common.save") }));
  await waitFor(() => expect(save).toHaveBeenCalledWith("native-b", "hash-b", "B only"));
});

test("close preserves local edits, cancel discards them, and blank saves are disabled", async () => {
  render(<ContinuationDraftBanner {...props} />);
  await review();
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "Unsaved" } });
  fireEvent.click(screen.getByRole("button", { name: tr("sessionImport.close") }));
  await review();
  expect((screen.getByRole("textbox") as HTMLTextAreaElement).value).toBe("Unsaved");
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "   " } });
  expect((screen.getByRole("button", { name: tr("common.save") }) as HTMLButtonElement).disabled).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: tr("common.cancel") }));
  await review();
  expect((screen.getByRole("textbox") as HTMLTextAreaElement).value).toBe(draft.text);
  expect(save).not.toHaveBeenCalled();
});

test("double save submits once while the first request is pending", async () => {
  const pending = deferred<ContinuationDraftView>();
  save.mockReturnValueOnce(pending.promise);
  render(<ContinuationDraftBanner {...props} />);
  await review();
  const button = screen.getByRole("button", { name: tr("common.save") });
  fireEvent.click(button); fireEvent.click(button);
  expect(save).toHaveBeenCalledTimes(1);
  expect((screen.getByRole("textbox") as HTMLTextAreaElement).disabled).toBe(true);
  await act(async () => pending.resolve(draft));
  expect(screen.queryByRole("dialog")).toBeNull();
});

test.each(["save-success", "save-failure", "reload"] as const)(
  "a stale %s cannot unlock Save while a newer context read is pending",
  async (operation) => {
    const previous = deferred<ContinuationDraftView>();
    const current = deferred<ContinuationDraftView>();
    const { rerender } = render(<ContinuationDraftBanner {...props} />);
    await review();
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "Keep my local edit" } });
    if (operation === "reload") get.mockReturnValueOnce(previous.promise);
    else save.mockReturnValueOnce(previous.promise);
    fireEvent.click(screen.getByRole("button", { name: tr(operation === "reload"
      ? "sessionImport.continuationReload" : "common.save") }));
    get.mockReturnValueOnce(current.promise);
    rerender(<ContinuationDraftBanner {...props} sessionState="ready" />);
    await waitFor(() => expect(get).toHaveBeenCalledTimes(operation === "reload" ? 3 : 2));
    await act(async () => {
      if (operation === "save-failure") previous.reject(new Error("stale failure"));
      else previous.resolve({ ...draft, text: "Stale response", textHash: "stale-hash" });
    });
    const saveButton = screen.getByRole("button", { name: tr("common.save") });
    expect((saveButton as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(saveButton);
    expect(save).toHaveBeenCalledTimes(operation === "reload" ? 0 : 1);
    expect((screen.getByRole("textbox") as HTMLTextAreaElement).value).toBe("Keep my local edit");
    expect(screen.queryByRole("alert")).toBeNull();
    await act(async () => current.resolve({ ...draft, state: "completed", text: null, textHash: null }));
    expect((saveButton as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole("textbox") as HTMLTextAreaElement).disabled).toBe(true);
    expect(screen.getAllByText(tr("sessionImport.continuationLocked"))).toHaveLength(2);
  },
);

test("the current explicit reload releases the loading state it superseded", async () => {
  const stale = deferred<ContinuationDraftView>();
  const reload = deferred<ContinuationDraftView>();
  const { rerender } = render(<ContinuationDraftBanner {...props} />);
  await review();
  get.mockReturnValueOnce(stale.promise).mockReturnValueOnce(reload.promise);
  rerender(<ContinuationDraftBanner {...props} sessionState="ready" />);
  fireEvent.click(screen.getByRole("button", { name: tr("sessionImport.continuationReload") }));
  await act(async () => reload.resolve({ ...draft, text: "Explicitly reloaded", textHash: "hash-current" }));
  const saveButton = screen.getByRole("button", { name: tr("common.save") });
  expect((saveButton as HTMLButtonElement).disabled).toBe(false);
  await act(async () => stale.resolve({ ...draft, state: "completed", text: null, textHash: null }));
  expect((saveButton as HTMLButtonElement).disabled).toBe(false);
  expect((screen.getByRole("textbox") as HTMLTextAreaElement).value).toBe("Explicitly reloaded");
  fireEvent.click(saveButton);
  await waitFor(() => expect(save).toHaveBeenCalledWith("native-a", "hash-current", "Explicitly reloaded"));
});

test("a lost save reply retains edits and retries the same CAS request", async () => {
  save.mockRejectedValueOnce(new Error("reply lost after save"))
    .mockResolvedValueOnce({ ...draft, text: "Durably saved edit", textHash: "hash-b" });
  render(<ContinuationDraftBanner {...props} />);
  await review();
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "Durably saved edit" } });
  fireEvent.click(screen.getByRole("button", { name: tr("common.save") }));
  await screen.findByRole("alert");
  expect((screen.getByRole("textbox") as HTMLTextAreaElement).value).toBe("Durably saved edit");
  fireEvent.click(screen.getByRole("button", { name: tr("common.save") }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(save.mock.calls).toEqual([
    ["native-a", "hash-a", "Durably saved edit"],
    ["native-a", "hash-a", "Durably saved edit"],
  ]);
  await review();
  expect((screen.getByRole("textbox") as HTMLTextAreaElement).value).toBe("Durably saved edit");
});

test("an open editor locks during sending and remains locked after dispatch", async () => {
  const { rerender } = render(<ContinuationDraftBanner {...props} />);
  await review();
  fireEvent.change(screen.getByRole("textbox"), { target: { value: "Local edit" } });
  get.mockResolvedValue({ ...draft, state: "running", text: null, textHash: null });
  rerender(<ContinuationDraftBanner {...props} sessionState="streaming" />);
  expect((screen.getByRole("textbox") as HTMLTextAreaElement).disabled).toBe(true);
  expect((screen.getByRole("button", { name: tr("common.save") }) as HTMLButtonElement).disabled).toBe(true);
  await waitFor(() => expect(screen.getAllByText(tr("sessionImport.continuationLocked"))).toHaveLength(2));
  rerender(<ContinuationDraftBanner {...props} sessionState="ready" />);
  await waitFor(() => expect(get).toHaveBeenCalledTimes(3));
  expect((screen.getByRole("textbox") as HTMLTextAreaElement).disabled).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: tr("common.save") }));
  expect(save).not.toHaveBeenCalled();
});

test.each(["dispatching", "running", "completed", "unknown"] as const)("%s context is never editable or retried by the panel", async (state) => {
  get.mockResolvedValue({ ...draft, state, text: null, textHash: null });
  render(<ContinuationDraftBanner {...props} />);
  await screen.findByText(tr("sessionImport.continuationLocked"));
  expect(screen.queryByRole("button", { name: tr("sessionImport.continuationReview") })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: tr("sessionImport.continuationHistory") }));
  expect(openHistory).toHaveBeenCalledWith({ importId: "imp:a", revision: "revision-a", leaf: "leaf-a" });
  expect(save).not.toHaveBeenCalled();
});

test("pre-dispatch failure stays reviewable even with an existing journal row", async () => {
  get.mockResolvedValue({ ...draft, state: "failed-before-dispatch" });
  render(<ContinuationDraftBanner {...props} empty={false} />);
  await review();
  expect((screen.getByRole("textbox") as HTMLTextAreaElement).value).toBe(draft.text);
});
