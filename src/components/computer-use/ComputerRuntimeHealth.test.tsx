/** @vitest-environment jsdom */
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, expect, it } from "vitest";
import "@testing-library/jest-dom/vitest";
import { createT, loadAllLocaleCatalogs, LOCALES } from "@/i18n";
import { ComputerRuntimeHealth } from "./ComputerRuntimeHealth";

afterEach(cleanup);
beforeAll(loadAllLocaleCatalogs);

it.each(LOCALES)("distinguishes unknown runtime state from unavailable in %s", locale => {
  const tr = createT(locale);
  render(<ComputerRuntimeHealth runtime={null} loading={false} tr={tr} />);
  expect(screen.getByRole("status")).toHaveTextContent(tr("cu.settings.runtimeUnknown"));
  expect(screen.getByRole("status")).toHaveAttribute("data-state", "unknown");
  expect(screen.queryByText(tr("cu.status.unavailable"))).toBeNull();
  expect(screen.queryByText(tr("cu.settings.runtimeOk"))).toBeNull();
});

it("does not replace active loading with an unknown or healthy result", () => {
  const tr = createT("en");
  render(<ComputerRuntimeHealth runtime={null} loading tr={tr} />);
  expect(screen.getByRole("status")).toHaveTextContent(tr("cu.status.checking"));
  expect(screen.queryByText(tr("cu.settings.runtimeUnknown"))).toBeNull();
});
