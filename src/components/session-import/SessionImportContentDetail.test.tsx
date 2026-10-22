/** @vitest-environment jsdom */
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { createT } from "@/i18n";
import { emptyContentSelection } from "@/lib/sessionImport/contentSelection";
import { SessionImportContentDetail } from "./SessionImportContentDetail";

afterEach(cleanup);

test("preview formats assistant content safely and preserves selection", () => {
  const onToggleEvent = vi.fn();
  render(<SessionImportContentDetail
    tr={createT("en")}
    busy={false}
    empty={false}
    state={{
      ...emptyContentSelection(),
      events: [
        { eventId: "a", kind: "message", role: "assistant", text: "## Preview\n\n![tracking](https://example.invalid/img)" },
        { eventId: "s", kind: "message", role: "system", text: "## Literal system" },
      ],
      selectedEventIds: ["a"],
    }}
    onToggleEvent={onToggleEvent}
    onSelectPage={vi.fn()}
    onInvertPage={vi.fn()}
    onField={vi.fn()}
    onSubagents={vi.fn()}
    onAsset={vi.fn()}
    onRequestLeaf={vi.fn()}
    onConfirmLeaf={vi.fn()}
    onCancelLeaf={vi.fn()}
    onLoadMore={vi.fn()}
  />);
  expect(screen.getByRole("heading", { name: "Preview" })).toBeTruthy();
  expect(screen.getByText("## Literal system").tagName).toBe("PRE");
  expect(document.querySelectorAll("a, img, video")).toHaveLength(0);
  fireEvent.click(screen.getByRole("checkbox", { name: "Assistant" }));
  expect(onToggleEvent).toHaveBeenCalledWith("a");
});

test.each(["en", "zh", "de"] as const)("preview keeps roles accessible without visible message labels in %s", (locale) => {
  const tr = createT(locale);
  const onToggleEvent = vi.fn();
  render(<SessionImportContentDetail
    tr={tr} busy={false} empty={false}
    state={{ ...emptyContentSelection(), events: [
      { eventId: "u", kind: "message", role: "user", text: "## Literal user text" },
      { eventId: "a", kind: "message", role: "assistant", text: "## Formatted assistant" },
    ], selectedEventIds: ["u", "a"] }}
    onToggleEvent={onToggleEvent} onSelectPage={vi.fn()} onInvertPage={vi.fn()}
    onField={vi.fn()} onSubagents={vi.fn()} onAsset={vi.fn()}
    onRequestLeaf={vi.fn()} onConfirmLeaf={vi.fn()} onCancelLeaf={vi.fn()} onLoadMore={vi.fn()}
  />);
  const userText = screen.getByText("## Literal user text");
  expect(userText.tagName).toBe("PRE");
  expect(userText.classList.contains("session-import__user-body")).toBe(true);
  expect(userText.closest("li")?.dataset.role).toBe("user");
  expect(screen.getByRole("heading", { name: "Formatted assistant" })).toBeTruthy();
  expect(screen.queryByText(tr("sessionImport.roleUser"))).toBeNull();
  expect(screen.queryByText(tr("sessionImport.roleAssistant"))).toBeNull();
  expect(screen.getByRole("checkbox", { name: tr("sessionImport.roleAssistant") })).toBeTruthy();
  fireEvent.click(screen.getByRole("checkbox", { name: tr("sessionImport.roleUser") }));
  expect(onToggleEvent).toHaveBeenCalledWith("u");
});
