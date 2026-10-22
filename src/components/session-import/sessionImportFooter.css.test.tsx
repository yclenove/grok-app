/** @vitest-environment jsdom */
import "@testing-library/jest-dom/vitest";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { GlassModal } from "@/components/GlassModal";
import { SessionImportSourceActions, SessionImportSourceStep } from "./SessionImportSourceStep";
import type { SourceChoice } from "@/lib/sessionImport/sourceChoice";

const stylesDir = join(dirname(fileURLToPath(import.meta.url)), "../../styles");
const modalStyles = readFileSync(join(stylesDir, "chat.part4.css"), "utf8");
const importStyles = readFileSync(join(stylesDir, "session-import.css"), "utf8");
const workbenchStyles = readFileSync(join(stylesDir, "session-import-workbench.css"), "utf8");

vi.mock("@/lib/nativeWebviewCover", () => ({ acquireNativeWebviewCover: () => () => {} }));

let style: HTMLStyleElement;
beforeEach(() => {
  style = document.createElement("style");
  // Use the shipped rules in import order, without triggering stylesheet fetches.
  const modalRules = ["modal", "modal-body", "modal-actions"].map((name) => {
    const rule = modalStyles.match(new RegExp(`^\\.${name}\\s*\\{[^}]+\\}`, "m"))?.[0];
    expect(rule).toBeTruthy();
    return rule;
  });
  style.textContent = [...modalRules, workbenchStyles,
    importStyles.replace(/^@import[^\n]+\n/gm, "")].join("\n");
  document.head.append(style);
});
afterEach(() => { cleanup(); style.remove(); });

function renderFooter(step: string, sibling: boolean) {
  const onBack = vi.fn();
  const onNext = vi.fn();
  const footer = <div className="session-import__toolbar session-import__footer">
    <button type="button" className="btn" onClick={onBack}>Back</button>
    <button type="button" className="btn btn--primary" onClick={onNext}>Next</button>
  </div>;
  render(<GlassModal open wrapBody onClose={vi.fn()} title="Synthetic import layout" closeLabel="Close"
    className={`session-import-modal session-import-wizard session-import-wizard--${step}`}
    footer={sibling ? footer : undefined}>
    <div {...(step === "mapping" ? { "data-import-mapping": "1" } : {})}>
      <p>Synthetic content</p>
      {!sibling ? footer : null}
    </div>
  </GlassModal>);
  return { onBack, onNext };
}

test.each(["prepare", "mapping", "import", "result"])(
  "%s sticky footer reserves the complete outside focus ring inside its scrollport", async (step) => {
    const user = userEvent.setup();
    const { onBack, onNext } = renderFooter(step, false);
    await waitFor(() => expect(screen.getByRole("button", { name: "Close" })).toHaveFocus());
    await user.tab();
    const back = screen.getByRole("button", { name: "Back" });
    expect(back).toHaveFocus();
    // jsdom does not reliably apply :focus-visible to computed styles. Check
    // the real declared ring, then compare it with the footer's computed inset.
    // This is a CSS contract test, not pixel or native focus-ring acceptance.
    const focusRule = Array.from(style.sheet!.cssRules).find((rule) =>
      rule instanceof CSSStyleRule &&
      rule.selectorText === ".session-import-modal :is(button, input, summary, [tabindex]):focus-visible",
    ) as CSSStyleRule;
    expect(focusRule).toBeDefined();
    const outline = focusRule.style.getPropertyValue("outline");
    const offset = focusRule.style.getPropertyValue("outline-offset");
    expect(outline).toMatch(/^2px solid /);
    expect(offset).toBe("2px");
    const extent = Number.parseFloat(outline) + Number.parseFloat(offset);
    const footer = back.closest(".session-import__footer")!;
    const layout = window.getComputedStyle(footer);
    expect(footer.closest(".modal-body")).not.toBeNull();
    expect(layout.position).toBe("sticky");
    expect(layout.bottom).toBe("0px");
    expect(layout.flexWrap).toBe("wrap");
    for (const padding of [layout.paddingTop, layout.paddingRight, layout.paddingBottom, layout.paddingLeft]) {
      expect(Number.parseFloat(padding)).toBeGreaterThanOrEqual(extent);
    }
    await user.keyboard("{Enter}");
    expect(onBack).toHaveBeenCalledOnce();
    expect(onNext).not.toHaveBeenCalled();
    await user.tab();
    expect(screen.getByRole("button", { name: "Next" })).toHaveFocus();
    await user.keyboard(" ");
    expect(onNext).toHaveBeenCalledOnce();
  },
);

test.each(["source", "content"])("%s sibling footer keeps its existing layout and modal keyboard loop", async (step) => {
  const user = userEvent.setup();
  renderFooter(step, true);
  const close = screen.getByRole("button", { name: "Close" });
  const next = screen.getByRole("button", { name: "Next" });
  const footer = next.closest(".session-import__footer")!;
  expect(footer.closest(".modal-body")).toBeNull();
  expect(footer.closest(".modal-actions")).not.toBeNull();
  const layout = window.getComputedStyle(footer);
  expect(layout.position).toBe("static");
  expect(layout.padding).toBe("0px");
  await waitFor(() => expect(close).toHaveFocus());
  await user.tab({ shift: true });
  expect(next).toHaveFocus();
  await user.tab();
  expect(close).toHaveFocus();
});

test("the sticky-footer adjustment does not style unrelated modal actions", () => {
  render(<GlassModal open wrapBody onClose={vi.fn()} title="Unrelated dialog"
    footer={<button type="button">Save</button>}><p>Unrelated content</p></GlassModal>);
  const actions = screen.getByRole("button", { name: "Save" }).closest(".modal-actions")!;
  expect(window.getComputedStyle(actions).position).not.toBe("sticky");
  expect(actions.querySelector(".session-import__footer")).toBeNull();
});

test("custom source actions wrap long labels without changing unrelated buttons or keyboard actions", async () => {
  const label = "JSON, Markdown அல்லது VS Code பணியிடத்தை இறக்குமதி செய்";
  const onPickFile = vi.fn();
  render(<>
    <SessionImportSourceStep
      tr={(key) => key === "sessionImport.fileMode" ? label : key}
      roots={[]} registrations={[]} choice={null} busy={false}
      onChoose={vi.fn()} onPickFolder={vi.fn()} onPickFile={onPickFile}
      onPickWorkspaceFolder={vi.fn()} onWorkspaces={vi.fn()}
      onRelocate={vi.fn()} onScan={vi.fn()}
    />
    <button type="button" className="btn">Unrelated action</button>
  </>);
  const actions = document.querySelectorAll<HTMLButtonElement>(
    ".session-import__custom-source > .session-import__toolbar > .btn",
  );
  expect(actions).toHaveLength(4);
  for (const action of actions) {
    const layout = window.getComputedStyle(action);
    expect(layout.boxSizing).toBe("border-box");
    expect(layout.minWidth).toBe("0px");
    expect(layout.maxWidth).toBe("100%");
    expect(layout.height).toBe("auto");
    expect(layout.whiteSpace).toBe("normal");
    expect(layout.overflowWrap).toBe("anywhere");
  }
  expect(window.getComputedStyle(screen.getByRole("button", { name: "Unrelated action" })).whiteSpace)
    .not.toBe("normal");
  const file = screen.getByRole("button", { name: label });
  file.focus();
  await userEvent.setup().keyboard("{Enter}");
  expect(onPickFile).toHaveBeenCalledOnce();
});

test.each(["none", "registered", "custom-folder"] as const)(
  "%s source summary preserves natural body height and chains root scrolling to the wizard", async (kind) => {
    const choice: SourceChoice | null = kind === "none" ? null : kind === "registered"
      ? { kind, provider: "cursor", path: "/synthetic/source-2", sourceId: "synthetic-2" }
      : { kind, provider: "cursor", path: "/synthetic/source-2" };
    const onScan = vi.fn();
    const onChoose = vi.fn();
    render(<GlassModal open wrapBody onClose={vi.fn()} title="Synthetic source scroll"
      closeLabel="Close" className="session-import-modal session-import-wizard session-import-wizard--source"
      footer={<SessionImportSourceActions tr={(key) => key} choice={choice} busy={false}
        scanStarting={false} onChoose={onChoose} onScan={onScan} />}>
      <div className="session-import__wizard-measure">
        <p>Synthetic step header</p>
        <SessionImportSourceStep tr={(key) => key} roots={[]} choice={choice} busy={false}
          registrations={Array.from({ length: 3 }, (_, index) => ({
            sourceId: `synthetic-${index}`, provider: "cursor", profile: "default",
            rootPath: `/synthetic/source-${index}`,
          }))}
          onChoose={onChoose} onPickFolder={vi.fn()} onPickFile={vi.fn()}
          onWorkspaces={vi.fn()} onPickWorkspaceFolder={vi.fn()}
          onRelocate={vi.fn()} onScan={onScan} />
      </div>
    </GlassModal>);
    // CSS contracts only: jsdom cannot measure pixel clipping or wheel input.
    // Native validation must still check actual short/narrow/zoomed scrollports.
    const measure = document.querySelector(".session-import__wizard-measure")!;
    const sizing = window.getComputedStyle(measure);
    expect(sizing.flexGrow).toBe("0");
    expect(sizing.flexShrink).toBe("0");
    expect(sizing.flexBasis).toBe("auto");
    const body = measure.closest(".modal-body")!;
    expect(window.getComputedStyle(body).overflow).toBe("auto");
    const roots = measure.querySelectorAll(".session-import__roots");
    expect(roots).toHaveLength(2);
    for (const root of roots) {
      const layout = window.getComputedStyle(root);
      expect(layout.overflow).toBe("auto");
      expect(layout.getPropertyValue("overscroll-behavior")).toBe("auto");
      expect(layout.maxHeight).toBe("min(32dvh, 264px)");
    }
    expect(screen.getAllByRole("button", { name: "sessionImport.locations.title" })).toHaveLength(3);
    const scan = screen.getByRole("button", { name: choice ? "sessionImport.scan" : "sessionImport.scanAll" });
    expect(scan.closest(".modal-body")).toBeNull();
    await waitFor(() => expect(screen.getByRole("button", { name: "Close" })).toHaveFocus());
    scan.focus();
    expect(scan).toHaveFocus();
    await userEvent.setup().keyboard("{Enter}");
    expect(onScan).toHaveBeenCalledOnce();
    expect(onChoose).not.toHaveBeenCalled();
  },
);

test("source scrolling does not change the content workbench flex owner or unrelated root containment", () => {
  render(<>
    <div className="session-import-wizard--content"><div className="session-import__wizard-measure" /></div>
    <div className="session-import__source"><ul className="session-import__roots" /></div>
  </>);
  const measure = document.querySelector(".session-import__wizard-measure")!;
  expect(window.getComputedStyle(measure).flex).toBe("1 1 auto");
  const root = document.querySelector(".session-import__roots")!;
  expect(window.getComputedStyle(root).getPropertyValue("overscroll-behavior")).toBe("contain");
});
