/** @vitest-environment jsdom */
import "@testing-library/jest-dom/vitest";
import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import { SessionImportDesignGallery } from "./SessionImportDesignGallery";

vi.mock("@/lib/nativeWebviewCover", () => ({ acquireNativeWebviewCover: () => () => {} }));
vi.mock("@/components/MarkdownBody", () => ({
  MarkdownBody: ({ children }: { children: string }) => <div className="md-body">{children}</div>,
}));

afterEach(cleanup);

test("the synthetic gallery keeps wide, narrow, short, theme, and long-translation frames distinct from a native import", async () => {
  const user = userEvent.setup();
  render(<SessionImportDesignGallery />);
  const root = document.querySelector("[data-design-gallery='synthetic']");
  expect(root).not.toBeNull();
  const wide = document.querySelector("[data-gallery-frame='wide']") as HTMLElement;
  const narrow = document.querySelector("[data-gallery-frame='narrow']") as HTMLElement;
  const minimum = document.querySelector("[data-gallery-frame='minimum']") as HTMLElement;
  const short = document.querySelector("[data-gallery-frame='short']") as HTMLElement;
  const long = document.querySelector("[data-gallery-frame='long']") as HTMLElement;
  expect(wide.dataset.theme).toBe("dark");
  expect(minimum.dataset.theme).toBe("light");
  expect(narrow.dataset.layout).toBe("narrow");
  expect(minimum.style.width).toBe("900px");
  expect(narrow.style.width).toBe("560px");
  expect(short.querySelector("[data-gallery-footer='1']")?.parentElement).toBe(short.querySelector(".session-import-gallery__short"));
  expect(short.querySelector(".session-import-gallery__scroll")?.contains(short.querySelector("[data-gallery-footer='1']") ?? null)).toBe(false);
  expect(within(narrow).getByRole("button", { name: "Importschritte" })).toBeTruthy();
  expect(within(long).getByText(/Synthetische Entwurfvorschau/)).toBeTruthy();
  expect(within(wide).getByText("Synthetic design preview. These records are not imported.")).toBeTruthy();
  expect(document.querySelector(".md-body")?.textContent).toContain("Not sent");
  expect(document.querySelector(".session-import__user-body")?.textContent).toContain("🙂");
  await user.click(within(narrow).getByRole("button", { name: "Importschritte" }));
  const prepare = await screen.findByRole("menuitem", { name: "3. Vorbereiten" });
  expect(prepare).toBeDisabled();
  await user.click(within(wide).getByRole("button", { name: "Review handoff context" }));
  const editor = await screen.findByRole("textbox", { name: "Edit the first message. Sending happens in the composer, not here." });
  await user.clear(editor);
  await user.type(editor, "Edited synthetic draft");
  await user.click(screen.getByRole("button", { name: "Save" }));
  expect(screen.queryByRole("textbox", { name: "Edit the first message. Sending happens in the composer, not here." })).toBeNull();
});
