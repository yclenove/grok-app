/** @vitest-environment jsdom */
import "@testing-library/jest-dom/vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test } from "vitest";
import { createT } from "@/i18n";
import type { ImportedRevisionHealth } from "@/lib/api/sessionMigration";
import { ImportedRevisionNotice } from "./ImportedRevisionNotice";

afterEach(cleanup);
const tr = createT("en");
const partial: ImportedRevisionHealth = {
  outcome: "partial", issueCount: 1, diagnosticsLimited: false,
  issues: [{ code: "vendor-detail", message: "Skipped unsupported role" }],
};

test("partial revision exposes skipped-record warning even without any event", async () => {
  render(<ImportedRevisionNotice health={partial} tr={tr} />);
  expect(screen.getByRole("status")).toHaveTextContent("Recorded issues: 1");
  expect(screen.getByText(/saved revision, not just the visible/)).toBeInTheDocument();
  expect(screen.queryByText("Skipped unsupported role")).toBeNull();
  const user = userEvent.setup();
  await user.tab();
  const button = screen.getByRole("button", { name: "Import diagnostics" });
  expect(button).toHaveFocus();
  await user.keyboard("{Enter}");
  expect(button).toHaveAttribute("aria-expanded", "true");
  expect(screen.getByText("Skipped unsupported role")).toBeVisible();
  expect(document.getElementById(button.getAttribute("aria-controls")!)).not.toBeNull();
  await user.keyboard(" ");
  expect(button).toHaveAttribute("aria-expanded", "false");
  expect(screen.queryByRole("list")).toBeNull();
});

test("loading and verified complete revisions do not show an incorrect warning", () => {
  const view = render(<ImportedRevisionNotice health={null} tr={tr} />);
  expect(screen.queryByRole("region")).toBeNull();
  view.rerender(<ImportedRevisionNotice health={{ ...partial, outcome: "complete", issueCount: 0, issues: [] }} tr={tr} />);
  expect(screen.queryByRole("region")).toBeNull();
});

test("unknown legacy quality is not advertised as complete or zero issues", () => {
  render(<ImportedRevisionNotice health={{ outcome: "unknown", issueCount: null, issues: [], diagnosticsLimited: false }} tr={tr} />);
  expect(screen.getByRole("status")).toHaveTextContent("could not be confirmed");
  expect(screen.getByRole("status")).not.toHaveTextContent("Recorded issues: 0");
  expect(screen.queryByRole("button")).toBeNull();
});

test("stored diagnostics stay inert and disclose the bounded sample", async () => {
  const unsafeText = "<img src='https://example.invalid/track'> [open](grok://run)";
  render(<ImportedRevisionNotice health={{ ...partial, issueCount: 200, diagnosticsLimited: true,
    issues: [{ code: "vendor-detail", message: unsafeText }, { code: "", message: "" }],
  }} tr={tr} />);
  await userEvent.click(screen.getByRole("button"));
  expect(screen.getByText(unsafeText)).toBeVisible();
  expect(screen.getByText("No diagnostic description is available.")).toBeVisible();
  expect(screen.getByText(/Showing 2 of 200/)).toBeVisible();
  expect(document.querySelectorAll("a, img, iframe, video")).toHaveLength(0);
});

test("recorded issues still warn when an older manifest says complete", () => {
  render(<ImportedRevisionNotice health={{ ...partial, outcome: "complete" }} tr={tr} />);
  expect(screen.getByRole("status")).toHaveTextContent("has limitations");
});

test.each([
  ["trailing-incomplete", "trailingIncomplete"],
  ["broken-record", "brokenRecord"],
  ["invalid-utf8", "invalidUtf8"],
  ["binary-record", "binary"],
  ["unknown-record", "unknownRecord"],
  ["unknown-role", "unknownRecord"],
  ["unsupported-item", "unknownRecord"],
] as const)("explains %s in the saved revision even when skipped records are absent", async (code, suffix) => {
  render(<ImportedRevisionNotice health={{ ...partial, issues: [{ code, message: "raw backend diagnosis" }] }} tr={tr} />);
  expect(screen.getByRole("status")).toHaveTextContent("has limitations");
  await userEvent.click(screen.getByRole("button", { name: tr("sessionImport.revision.details") }));
  expect(screen.getByText(tr(`sessionImport.diagnostic.${suffix}`))).toBeVisible();
  expect(screen.queryByText("raw backend diagnosis")).not.toBeInTheDocument();
});
