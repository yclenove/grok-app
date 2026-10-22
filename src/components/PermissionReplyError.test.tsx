/** @vitest-environment jsdom */
import "@/test/jsdomStubs";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { createT } from "@/i18n";
import { PermissionReplyError } from "./PermissionReplyError";

afterEach(cleanup);

test.each(["zh", "en"] as const)("a restored failed card keeps a localized visible alert (%s)", (locale) => {
  const tr = createT(locale);
  const view = render(<PermissionReplyError error={null} responseError="写入失败" tr={tr} />);
  expect(screen.getByRole("alert").textContent).toBe(tr("perm.replyFailed", { error: "写入失败" }));
  view.rerender(<PermissionReplyError error={null} tr={tr} />);
  expect(screen.queryByRole("alert")).toBeNull();
});

test("a user's failed retry shows its current error before the automatic failure", () => {
  const tr = createT("zh");
  const view = render(<PermissionReplyError error="当前重试失败" responseError="早先失败" tr={tr} />);
  expect(screen.getByRole("alert").textContent).toBe("当前重试失败");
  view.rerender(<PermissionReplyError error={null} responseError="早先失败" tr={tr} />);
  expect(screen.getByRole("alert").textContent).toBe(tr("perm.replyFailed", { error: "早先失败" }));
});

test("ordinary and blank-error permission cards do not show a failure alert", () => {
  render(<PermissionReplyError error={null} responseError=" " tr={createT("en")} />);
  expect(screen.queryByRole("alert")).toBeNull();
});
