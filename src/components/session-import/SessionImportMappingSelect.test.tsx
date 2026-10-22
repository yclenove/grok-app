/** @vitest-environment jsdom */
import "@testing-library/jest-dom/vitest";
import { useState } from "react";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import { createT } from "@/i18n";
import { LOCALES } from "@/i18n/messages";
import { SessionImportMappingSelect } from "./SessionImportMappingSelect";

afterEach(cleanup);

test.each(LOCALES)("%s labels both the source and displayed selection", (locale) => {
  const tr = createT(locale);
  const sourceLabel = "vscode-remote://ssh-remote+alpha/repo";
  const props = { tr, sourceLabel, onChange: vi.fn(), options: [
    { value: "", label: tr("sessionImport.workspace.unassigned") },
    { value: "project", label: "Design project" },
    { value: "__directory__", label: "/new target" },
    { value: "missing", label: `${tr("sessionImport.err.mappingConflict")}: missing` },
  ] };
  const { rerender } = render(<SessionImportMappingSelect {...props} value="" />);
  for (const option of props.options) {
    rerender(<SessionImportMappingSelect {...props} value={option.value} />);
    const trigger = screen.getByRole("button", {
      name: tr("sessionImport.mappingTarget", { source: sourceLabel, target: option.label }),
    });
    expect(trigger).toHaveTextContent(option.label);
    expect(trigger).toHaveAttribute("aria-haspopup", "listbox");
  }
  expect(props.onChange).not.toHaveBeenCalled();
  expect(document.querySelector("select")).toBeNull();
});

test("real keyboard selection updates one source, restores focus, and does not commit", async () => {
  const user = userEvent.setup();
  const choose = vi.fn();
  const commit = vi.fn();
  const tr = createT("en");
  function Harness() {
    const [value, setValue] = useState("");
    const options = [{ value: "", label: "No project mapping" }, { value: "p1", label: "Design project" }];
    return <div role="dialog" aria-label="Mappings">
      <SessionImportMappingSelect tr={tr} sourceLabel="/repo · alpha" value={value} options={options}
        onChange={(next) => { choose(next); setValue(next); }} />
      <SessionImportMappingSelect tr={tr} sourceLabel="/repo · beta" value="" options={options} onChange={choose} />
      <button onClick={commit}>Import selected</button>
    </div>;
  }
  render(<Harness />);
  const trigger = screen.getByRole("button", { name: "Project mapping for /repo · alpha: No project mapping" });
  await user.tab();
  expect(trigger).toHaveFocus();
  await user.keyboard("{ArrowDown}");
  const menu = await screen.findByRole("listbox", { name: "Project mapping for /repo · alpha: No project mapping" });
  expect(document.body).toContainElement(menu);
  expect(screen.getByRole("dialog")).not.toContainElement(menu);
  await waitFor(() => expect(within(menu).getByRole("button", { name: "No project mapping" })).toHaveFocus());
  await user.keyboard("{ArrowDown}{Enter}");
  await waitFor(() => expect(screen.queryByRole("listbox")).toBeNull());
  expect(trigger).toHaveAccessibleName("Project mapping for /repo · alpha: Design project");
  await waitFor(() => expect(trigger).toHaveFocus());
  expect(screen.getByRole("button", { name: "Project mapping for /repo · beta: No project mapping" })).toBeEnabled();
  expect(choose).toHaveBeenCalledExactlyOnceWith("p1");
  expect(commit).not.toHaveBeenCalled();
  await user.keyboard("{ArrowDown}");
  await screen.findByRole("listbox", { name: "Project mapping for /repo · alpha: Design project" });
  await user.keyboard("{Escape}");
  await waitFor(() => expect(trigger).toHaveFocus());
  expect(screen.queryByRole("listbox")).toBeNull();
  expect(choose).toHaveBeenCalledTimes(1);
  expect(commit).not.toHaveBeenCalled();
});

test("a read-only mapping remains accurately named and cannot open or change", async () => {
  const onChange = vi.fn();
  render(<SessionImportMappingSelect tr={createT("en")} sourceLabel="/repo" value="directory" disabled
    options={[{ value: "directory", label: "/mapped directory" }]} onChange={onChange} />);
  const trigger = screen.getByRole("button", { name: "Project mapping for /repo: /mapped directory" });
  expect(trigger).toBeDisabled();
  await userEvent.click(trigger);
  expect(screen.queryByRole("listbox")).toBeNull();
  expect(onChange).not.toHaveBeenCalled();
});
