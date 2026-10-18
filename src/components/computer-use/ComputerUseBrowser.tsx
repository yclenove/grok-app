/**
 * Built-in browser chrome. The shipped state is closed and not_run.
 * An open fixture is a UI test only; it does not create a content webview.
 */

import { useMemo, useReducer } from "react";
import { createT, type Locale } from "@/i18n";
import {
  browserReducer,
  closedBrowser,
  escapeUntrusted,
  type BrowserView,
} from "@/lib/computer-use/browser";

export type NavigateResult = "blocked" | "failed" | "loading";

export function ComputerUseBrowser({
  locale,
  initial,
  onNavigate,
}: {
  locale: Locale;
  initial?: BrowserView;
  onNavigate?: (url: string) => NavigateResult;
}) {
  const t = useMemo(() => createT(locale), [locale]);
  const [view, dispatch] = useReducer(browserReducer, initial ?? closedBrowser());
  const closed = view.availability !== "open";
  const title = escapeUntrusted(view.pageTitle);

  function submit() {
    if (closed) return;
    const result = onNavigate?.(view.draft) ?? "blocked";
    dispatch({ type: result });
  }

  return (
    <section className="sw-computer__browser" data-testid="cu-browser" data-availability={view.availability} data-proof={view.proof} aria-label={t("cu.browser.origin")}>
      {closed ? (
        <div data-testid="cu-browser-closed">
          <h3>{t("cu.browser.unavailableTitle")}</h3>
          <p>{t("cu.browser.unavailableBody")}</p>
        </div>
      ) : (
        <p data-testid="cu-browser-origin">{view.origin}</p>
      )}
      <form
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        <label>
          {t("cu.browser.address")}
          <input
            className="settings-input"
            data-testid="cu-browser-address"
            value={view.draft}
            disabled={closed}
            onChange={(event) => dispatch({ type: "draft", value: event.target.value })}
          />
        </label>
        <button type="submit" className="btn" data-testid="cu-browser-go" disabled={closed}>
          {t("cu.browser.address")}
        </button>
        <button type="button" className="btn" data-testid="cu-browser-back" disabled={closed} onClick={() => dispatch({ type: "blocked" })}>
          {t("cu.browser.back")}
        </button>
        <button type="button" className="btn" data-testid="cu-browser-forward" disabled={closed} onClick={() => dispatch({ type: "blocked" })}>
          {t("cu.browser.forward")}
        </button>
        <button type="button" className="btn" data-testid="cu-browser-reload" disabled={closed} onClick={() => dispatch({ type: "blocked" })}>
          {t("cu.browser.reload")}
        </button>
      </form>
      {view.status === "loading" ? <p role="status">{t("cu.browser.loading")}</p> : null}
      {view.status === "blocked" ? <p role="alert" data-testid="cu-browser-blocked">{t("cu.browser.blocked")}</p> : null}
      {view.status === "failed" ? <p role="alert" data-testid="cu-browser-failed">{t("cu.browser.failed")}</p> : null}
      <p data-testid="cu-browser-title" aria-label={t("cu.browser.pageTitle")}>{title}</p>
    </section>
  );
}
