/** @vitest-environment jsdom */
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import type { MessageKey, Vars } from "@/i18n";
import { ImportedTextReader } from "./ImportedTextReader";

afterEach(cleanup);
const tr = (key: MessageKey, vars?: Vars) => vars ? `${key}:${JSON.stringify(vars)}` : key;
const body = () => screen.getByRole("dialog").querySelector("pre")!;

test("pages preserve every character without splitting emoji or rendering more than the budget", () => {
  const text = "x".repeat(3999) + "🙂" + "y".repeat(3999) + "🧪tail";
  render(<ImportedTextReader text={text} tr={tr} onClose={vi.fn()} />);
  const parts = [body().textContent!];
  const next = screen.getByRole("button", { name: "sessionImport.nextPage" }) as HTMLButtonElement;
  while (!next.disabled) {
    fireEvent.click(next);
    expect(body().textContent!.length).toBeLessThanOrEqual(4000);
    parts.push(body().textContent!);
  }
  expect(parts.join("")).toBe(text);
  expect(parts[0].endsWith("x")).toBe(true);
  expect(parts[1].startsWith("🙂")).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "sessionImport.previousPage" }));
  expect(body().textContent).toBe(parts[1]);
});

test("literal case-insensitive find navigates and wraps emoji matches without changing offsets", () => {
  const text = "🙂" + "a".repeat(4000) + "🙂 Needle.*[x]" + "b".repeat(4000);
  render(<ImportedTextReader text={text} tr={tr} onClose={vi.fn()} />);
  const input = screen.getByRole("searchbox");
  fireEvent.change(input, { target: { value: "🙂" } });
  fireEvent.click(screen.getByRole("button", { name: "sessionImport.search" }));
  expect(body().querySelector("mark")?.textContent).toBe("🙂");
  fireEvent.click(screen.getByRole("button", { name: "sessionImport.nextHit" }));
  expect(body().textContent).toContain("Needle.*[x]");
  fireEvent.click(screen.getByRole("button", { name: "sessionImport.nextHit" }));
  expect(body().textContent).not.toContain("Needle.*[x]");
  fireEvent.click(screen.getByRole("button", { name: "sessionImport.prevHit" }));
  expect(body().textContent).toContain("Needle.*[x]");
  fireEvent.change(input, { target: { value: "needle.*[x]" } });
  fireEvent.click(screen.getByRole("button", { name: "sessionImport.search" }));
  expect(body().querySelector("mark")?.textContent).toBe("Needle.*[x]");
  fireEvent.change(input, { target: { value: "MISSING_SENTINEL" } });
  fireEvent.click(screen.getByRole("button", { name: "sessionImport.search" }));
  expect(screen.getByText("sessionImport.textSearchNoHits")).toBeTruthy();
  expect(body().textContent).toContain("Needle.*[x]");
  fireEvent.click(screen.getByRole("button", { name: "sessionImport.clearSearch" }));
  expect(screen.queryByText("sessionImport.textSearchNoHits")).toBeNull();
});

test("IME candidate confirmation does not search, and search results never execute HTML or Markdown", async () => {
  const close = vi.fn();
  render(<ImportedTextReader text={"a".repeat(4100) + "<script>你好</script> [run](grok://run)"} tr={tr} onClose={close} />);
  const input = screen.getByRole("searchbox") as HTMLInputElement;
  fireEvent.compositionStart(input);
  fireEvent.change(input, { target: { value: "你好" } });
  fireEvent.keyDown(input, { key: "Enter", isComposing: true });
  fireEvent.submit(input.form!);
  expect(body().textContent).not.toContain("你好");
  fireEvent.compositionEnd(input);
  fireEvent.keyUp(input, { key: "Enter" });
  fireEvent.submit(input.form!);
  expect(body().textContent).toContain("你好");
  expect(document.querySelectorAll("script, a, img, iframe, video")).toHaveLength(0);
  input.focus();
  await userEvent.keyboard("{Escape}");
  expect(close).toHaveBeenCalledOnce();
});

test("rejects over-budget UTF-8 searches on submission without losing text or editing the input", () => {
  const text = "x".repeat(4100) + "可恢复";
  render(<ImportedTextReader text={text} tr={tr} onClose={vi.fn()} />);
  const input = screen.getByRole("searchbox") as HTMLInputElement;
  const before = body().textContent;
  const oversized = "中".repeat(1366);
  fireEvent.change(input, { target: { value: oversized } });
  expect(screen.queryByRole("alert")).toBeNull();
  fireEvent.submit(input.form!);
  expect(screen.queryByRole("alert")?.textContent).toBe("sessionImport.searchQueryTooLong");
  expect(input.value).toBe(oversized);
  expect(body().textContent).toBe(before);
  expect(screen.queryByRole("button", { name: "sessionImport.nextHit" })).toBeNull();
  fireEvent.change(input, { target: { value: "可恢复" } });
  fireEvent.submit(input.form!);
  expect(screen.queryByRole("alert")).toBeNull();
  expect(body().querySelector("mark")?.textContent).toBe("可恢复");
});

test("accepts the exact local literal budget and clears invalid searches without resetting the page", () => {
  render(<ImportedTextReader text={"x".repeat(4100) + "needle"} tr={tr} onClose={vi.fn()} />);
  const input = screen.getByRole("searchbox") as HTMLInputElement;
  fireEvent.change(input, { target: { value: "x".repeat(4096) } });
  fireEvent.submit(input.form!);
  expect(screen.queryByRole("alert")).toBeNull();
  expect(body().querySelector("mark")?.textContent).toHaveLength(4000);
  fireEvent.click(screen.getByRole("button", { name: "sessionImport.nextPage" }));
  const before = body().textContent;
  // Local full-text searches are literal: surrounding spaces must not be
  // trimmed away to make an otherwise over-budget regex appear acceptable.
  fireEvent.change(input, { target: { value: " ".repeat(4097) } });
  fireEvent.submit(input.form!);
  expect(screen.queryByRole("alert")?.textContent).toBe("sessionImport.searchQueryTooLong");
  expect(body().textContent).toBe(before);
  fireEvent.click(screen.getByRole("button", { name: "sessionImport.clearSearch" }));
  expect(input.value).toBe("");
  expect(screen.queryByRole("alert")).toBeNull();
  expect(body().textContent).toBe(before);
});

test("a very large pasted query never reaches the regex compiler", () => {
  const text = "x".repeat(4100);
  render(<ImportedTextReader text={text} tr={tr} onClose={vi.fn()} />);
  const input = screen.getByRole("searchbox") as HTMLInputElement;
  const oversized = "x".repeat(65536);
  fireEvent.change(input, { target: { value: oversized } });
  fireEvent.submit(input.form!);
  expect(screen.queryByRole("alert")?.textContent).toBe("sessionImport.searchQueryTooLong");
  expect(input.value).toBe(oversized);
  expect(body().textContent).toBe(text.slice(0, 4000));
  expect(body().querySelector("mark")).toBeNull();
});
