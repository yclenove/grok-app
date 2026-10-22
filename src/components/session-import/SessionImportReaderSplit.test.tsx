/** @vitest-environment jsdom */
import { useState } from "react";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import "@testing-library/jest-dom/vitest";
import { afterEach, expect, test } from "vitest";
import { createT } from "@/i18n";
import { SessionImportReaderSplit, type ReaderPane } from "./SessionImportReaderSplit";

afterEach(cleanup);

function Harness() {
  const tr = createT("en");
  const [query, setQuery] = useState("锚点 🙂");
  const [pane, setPane] = useState<ReaderPane>("events");
  return (
    <div>
      <input aria-label="Search imported history" value={query} onChange={(event) => setQuery(event.target.value)} />
      <SessionImportReaderSplit
        tr={tr}
        narrow
        pane={pane}
        onPane={setPane}
        matches={<p>page matches only</p>}
        events={<p>event body</p>}
      />
    </div>
  );
}

test("narrow pane switching keeps the search query and reveals the selected body", async () => {
  const user = userEvent.setup();
  render(<Harness />);
  const input = screen.getByRole("textbox", { name: "Search imported history" });
  expect(screen.getByText("event body")).toBeVisible();
  expect(screen.getByText("page matches only")).not.toBeVisible();
  await user.click(screen.getByRole("button", { name: "Matches" }));
  expect(screen.getByText("page matches only")).toBeVisible();
  expect(screen.getByText("event body")).not.toBeVisible();
  expect(input).toHaveValue("锚点 🙂");
  await user.click(screen.getByRole("button", { name: "Messages" }));
  expect(screen.getByText("event body")).toBeVisible();
  expect(input).toHaveValue("锚点 🙂");
});

test("null matches omit the results column", () => {
  const tr = createT("en");
  render(
    <SessionImportReaderSplit
      tr={tr}
      narrow={false}
      pane="events"
      onPane={() => {}}
      matches={null}
      events={<p>event body</p>}
    />,
  );
  const split = document.querySelector(".session-import__reader-split");
  expect(split?.getAttribute("data-matches")).toBe("0");
  expect(split?.getAttribute("data-layout")).toBe("wide");
  expect(document.querySelector(".session-import__reader-matches")).toBeNull();
  expect(screen.queryByRole("button", { name: "Matches" })).toBeNull();
  expect(screen.getByText("event body")).toBeTruthy();
});
