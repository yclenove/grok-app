/** @vitest-environment jsdom */
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { createT, type Locale } from "@/i18n";
import { installDialogFocus } from "@/lib/a11yFocus";
import { emptyContentSelection, type ContentSelectionState } from "@/lib/sessionImport/contentSelection";
import { SessionImportContentDetail } from "./SessionImportContentDetail";

afterEach(cleanup);

function initialState(): ContentSelectionState {
  return {
    ...emptyContentSelection(),
    previewId: "synthetic-preview",
    generation: 1,
    leaf: "leaf-a",
    leaves: ["leaf-a"],
    events: [{ eventId: "old", kind: "message", role: "user", text: "Existing event" }],
    selectedEventIds: ["old"],
    nextCursor: "page-2",
  };
}

function fixture(locale: Locale = "en") {
  const tr = createT(locale);
  const initial = initialState();
  const onLoadMore = vi.fn();
  const view = (state: ContentSelectionState, busy: boolean, hidden = false) => (
    <div role="dialog" aria-label="Synthetic preview" hidden={hidden}>
      <SessionImportContentDetail
        tr={tr} state={state} busy={busy} empty={state.events.length === 0}
        onToggleEvent={vi.fn()} onSelectPage={vi.fn()} onInvertPage={vi.fn()}
        onField={vi.fn()} onSubagents={vi.fn()} onAsset={vi.fn()}
        onRequestLeaf={vi.fn()} onConfirmLeaf={vi.fn()} onCancelLeaf={vi.fn()}
        onLoadMore={onLoadMore}
      />
      <button type="button" data-testid="outside-detail">Another control</button>
    </div>
  );
  const rendered = render(view(initial, false));
  const more = screen.getByRole("button", { name: tr("sessionImport.loadMore") });
  const request = () => {
    more.focus();
    fireEvent.click(more);
    expect(onLoadMore).toHaveBeenCalledTimes(1);
  };
  return {
    initial, more, request, tr,
    update: (state: ContentSelectionState, busy = false, hidden = false) => rendered.rerender(view(state, busy, hidden)),
  };
}

function appended(state: ContentSelectionState, nextCursor: string | null = null): ContentSelectionState {
  return {
    ...state,
    events: [...state.events, { eventId: "new", kind: "message", role: "assistant", text: "New event" }],
    nextCursor,
  };
}

function loseBrowserFocus() {
  // jsdom's blur() is a no-op on a disabled button; emulate the browser's
  // focus loss explicitly, rather than accidentally testing a retained focus.
  const previous = document.body.getAttribute("tabindex");
  document.body.tabIndex = -1;
  document.body.focus();
  if (previous === null) document.body.removeAttribute("tabindex");
  else document.body.setAttribute("tabindex", previous);
  expect(document.activeElement).toBe(document.body);
}

test.each(["en", "zh", "de"] as const)("last preview page keeps keyboard focus on newly loaded content in %s", (locale) => {
  const f = fixture(locale);
  f.request();
  f.update(f.initial, true);
  f.update(appended(f.initial));
  expect(f.more.isConnected).toBe(false);
  expect(document.activeElement).toBe(screen.getByRole("checkbox", { name: f.tr("sessionImport.roleAssistant") }));
});

test("a remaining preview page restores the same load control after a busy blur", () => {
  const f = fixture();
  f.request();
  f.update(f.initial, true);
  loseBrowserFocus();
  f.update(appended(f.initial, "page-3"));
  expect(document.activeElement).toBe(f.more);
});

test("an already focused remaining load control stays focused", () => {
  const f = fixture();
  f.request();
  f.update(appended(f.initial, "page-3"));
  expect(document.activeElement).toBe(f.more);
});

test("failed pagination restores the enabled retry control without new rows", () => {
  const f = fixture();
  f.request();
  f.update(f.initial, true);
  loseBrowserFocus();
  f.update(f.initial);
  expect(document.activeElement).toBe(f.more);
});

test("a terminal empty page keeps focus near the existing event", () => {
  const f = fixture();
  f.request();
  f.update({ ...f.initial, nextCursor: null });
  expect(document.activeElement).toBe(screen.getByRole("checkbox", { name: f.tr("sessionImport.roleUser") }));
});

test("a terminal unselectable row remains a focusable reading target", () => {
  const f = fixture();
  f.request();
  const next = appended(f.initial);
  next.events[1] = { ...next.events[1]!, selectable: false };
  f.update(next);
  const row = screen.getByText("New event").closest("li");
  expect(row?.tabIndex).toBe(-1);
  expect(document.activeElement).toBe(row);
});

test("an unfocused programmatic load does not acquire keyboard ownership", () => {
  const f = fixture();
  screen.getByTestId("outside-detail").focus();
  fireEvent.click(f.more);
  f.update(appended(f.initial));
  expect(document.activeElement).toBe(screen.getByTestId("outside-detail"));
});

test("a deliberate focus move during loading is not pulled back", () => {
  const f = fixture();
  f.request();
  f.update(f.initial, true);
  screen.getByTestId("outside-detail").focus();
  f.update(appended(f.initial));
  expect(document.activeElement).toBe(screen.getByTestId("outside-detail"));
});

test("a deliberate focus move stays released even if that control later loses focus", () => {
  const f = fixture();
  f.request();
  f.update(f.initial, true);
  screen.getByTestId("outside-detail").focus();
  loseBrowserFocus();
  f.update(appended(f.initial));
  expect(document.activeElement).toBe(document.body);
});

test("a pointer move elsewhere releases pagination ownership without needing focus", () => {
  const f = fixture();
  f.request();
  f.update(f.initial, true);
  fireEvent.pointerDown(screen.getByText("Existing event"));
  f.update(appended(f.initial));
  expect(document.activeElement).toBe(document.body);
});

test("a deliberate pointer move onto the page background releases ownership", () => {
  const f = fixture();
  f.request();
  f.update(f.initial, true);
  fireEvent.pointerDown(document.body);
  f.update(appended(f.initial));
  expect(document.activeElement).toBe(document.body);
});

test("terminal pagination scrolls the message row, not only its checkbox", () => {
  const scroll = vi.fn();
  const previous = HTMLElement.prototype.scrollIntoView;
  HTMLElement.prototype.scrollIntoView = function () { scroll(this); };
  try {
    const f = fixture();
    f.request();
    f.update(appended(f.initial));
    expect(scroll).toHaveBeenCalledWith(screen.getByText("New event").closest("li"));
  } finally {
    HTMLElement.prototype.scrollIntoView = previous;
  }
});

test.each(["inert", "aria-hidden", "display", "visibility"])("a preview concealed by %s cannot restore focus", (concealment) => {
  const f = fixture();
  const dialog = screen.getByRole("dialog");
  f.request();
  f.update(f.initial, true);
  if (concealment === "inert") dialog.setAttribute("inert", "");
  else if (concealment === "aria-hidden") dialog.setAttribute("aria-hidden", "true");
  else if (concealment === "display") dialog.style.display = "none";
  else dialog.style.visibility = "hidden";
  f.update(appended(f.initial));
  expect(document.activeElement).toBe(document.body);
});

test("an empty preview terminal page falls back to its existing controls", () => {
  const f = fixture();
  const initial = { ...f.initial, events: [] };
  f.update(initial);
  f.request();
  f.update({ ...initial, nextCursor: null });
  expect(document.activeElement).toBe(screen.getByRole("button", { name: f.tr("sessionImport.selectLeaf") }));
});

test("a pending branch confirmation drops the pagination claim", () => {
  const f = fixture();
  f.request();
  f.update({ ...appended(f.initial), pendingLeaf: "other-leaf" });
  expect(document.querySelector(".session-import__events")?.contains(document.activeElement)).toBe(false);
});

test.each(["previewId", "generation", "leaf"] as const)("a changed %s drops the old pagination claim", (field) => {
  const f = fixture();
  f.request();
  const next = appended(f.initial);
  if (field === "generation") next.generation += 1;
  else next[field] = "replacement";
  f.update(next);
  expect(document.activeElement).toBe(document.body);
});

test("a hidden preview cannot take focus after its request finishes", () => {
  const f = fixture();
  f.request();
  f.update(appended(f.initial), false, true);
  expect(document.activeElement).toBe(document.body);
});

test("a newer dialog retains ownership even if its focus is temporarily lost", () => {
  const f = fixture();
  f.request();
  const overlay = document.createElement("div");
  overlay.setAttribute("role", "dialog");
  document.body.append(overlay);
  const release = installDialogFocus(() => overlay, { initialFocus: "none", restoreFocus: false });
  try {
    f.update(appended(f.initial));
    expect(document.activeElement).toBe(document.body);
  } finally {
    release();
    overlay.remove();
  }
});
