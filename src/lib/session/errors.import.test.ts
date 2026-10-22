import { describe, expect, it } from "vitest";
import { LOCALES, t } from "../../i18n";
import { applyTurnError, formatTurnErrorBody, presentErrorBanner } from "./errors";

const changed = "HANDOFF_CHANGED: handoff changed before dispatch; review and retry";

describe("handoff changed on the normal chat send surface", () => {
  it.each(LOCALES)("localizes coded turn errors in %s", (locale) => {
    const copy = t(locale, "sessionImport.err.handoffChanged");
    expect(formatTurnErrorBody({ code: "HANDOFF_CHANGED" }, locale)).toBe(copy);
    expect(formatTurnErrorBody({ message: changed }, locale)).toBe(copy);
    expect(formatTurnErrorBody({ content: changed }, locale)).toBe(copy);
  });

  it.each(LOCALES)("shows review guidance without reconnect in %s", (locale) => {
    const banner = presentErrorBanner(null, changed, locale);
    expect(banner).toMatchObject({
      code: "HANDOFF_CHANGED",
      summary: t(locale, "sessionImport.err.handoffChanged"),
      cause: null,
      detail: null,
      reconnectHint: false,
      primary: { id: "dismiss" },
      secondary: null,
      deck: null,
    });
    expect(banner?.primary?.label).toBeTruthy();
  });

  it("replaces the pending assistant with the review message", () => {
    const messages = applyTurnError(
      [
        { id: "user", role: "user", content: "continue" },
        { id: "pending", role: "assistant", content: "", streaming: true },
      ],
      { message: changed },
      "zh",
    );
    expect(messages).toHaveLength(2);
    expect(messages[1]).toMatchObject({
      content: t("zh", "sessionImport.err.handoffChanged"),
      streaming: false,
      isError: true,
    });
  });

  it("honors the stable prefix before generic network heuristics", () => {
    const message = "\u001b[31mHANDOFF_CHANGED: timeout in diagnostic detail\u001b[0m";
    expect(formatTurnErrorBody({ message }, "en")).toBe(
      t("en", "sessionImport.err.handoffChanged"),
    );
    expect(presentErrorBanner(null, message, "en")?.reconnectHint).toBe(false);
  });

  it.each(["discuss HANDOFF_CHANGED", "HANDOFF_CHANGED_EXTRA: details"])(
    "does not recognize a substring or extended code: %s",
    (message) => {
      expect(formatTurnErrorBody({ message }, "en")).toBe(message);
      expect(presentErrorBanner(null, message, "en")?.code).toBeNull();
    },
  );

  it("keeps an existing agent error ahead of unrelated local errors", () => {
    expect(presentErrorBanner(
      { code: "CLI_NOT_FOUND", message: "CLI missing" }, changed, "en",
    )?.code).toBe("CLI_NOT_FOUND");
  });
});
