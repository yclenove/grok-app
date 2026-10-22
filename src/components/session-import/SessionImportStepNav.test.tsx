/** @vitest-environment jsdom */
import "@testing-library/jest-dom/vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import { createT } from "@/i18n";
import type { ImportStep } from "@/hooks/useSessionImport";
import { SessionImportStepNav } from "./SessionImportStepNav";

afterEach(cleanup);

const tr = createT("en");

test("a disabled step does not change the wizard", async () => {
  const onGo = vi.fn();
  const user = userEvent.setup();
  render(<SessionImportStepNav
    tr={tr}
    step="source"
    busy={false}
    layout="wide"
    stepOpenable={(step) => step === "source"}
    onGo={onGo}
  />);
  const sessions = screen.getByRole("button", { name: "Sessions" });
  expect(sessions).toBeDisabled();
  await user.click(sessions);
  expect(onGo).not.toHaveBeenCalled();
  await user.click(screen.getByRole("button", { name: "Source" }));
  expect(onGo).toHaveBeenCalledExactlyOnceWith("source");
});

test("the narrow menu only opens a reachable step", async () => {
  const onGo = vi.fn();
  const user = userEvent.setup();
  const openable = (step: ImportStep) => step === "source" || step === "content";
  render(<SessionImportStepNav
    tr={tr}
    step="content"
    busy={false}
    layout="narrow"
    stepOpenable={openable}
    onGo={onGo}
  />);
  expect(screen.queryByRole("list")).toBeNull();
  await user.click(screen.getByRole("button", { name: "Import steps" }));
  const prepare = await screen.findByRole("menuitem", { name: "3. Prepare" });
  expect(prepare).toBeDisabled();
  await user.click(prepare);
  expect(onGo).not.toHaveBeenCalled();
  await user.click(screen.getByRole("menuitem", { name: "1. Source" }));
  expect(onGo).toHaveBeenCalledExactlyOnceWith("source");
});
