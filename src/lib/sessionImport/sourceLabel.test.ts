import { expect, test } from "vitest";
import { displaySourcePath } from "./sourceLabel";

test("verbatim Windows prefixes are not shown as part of the folder", () => {
  expect(displaySourcePath("\\\\?\\D:\\profile\\.codex")).toBe("D:\\profile\\.codex");
  expect(displaySourcePath("\\\\?\\UNC\\server\\share\\.codex")).toBe("\\\\server\\share\\.codex");
  expect(displaySourcePath("D:\\profile\\.codex")).toBe("D:\\profile\\.codex");
  expect(displaySourcePath("/home/me/.codex")).toBe("/home/me/.codex");
});
