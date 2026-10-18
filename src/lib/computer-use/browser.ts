/** Address-bar view. Production starts closed. `not_run` is not a native pass. */

export type BrowserStatus = "idle" | "loading" | "blocked" | "failed";

export type BrowserView = {
  availability: "closed" | "open";
  proof: "not_run";
  origin: string;
  draft: string;
  pageTitle: string;
  status: BrowserStatus;
};

export type BrowserAction =
  | { type: "draft"; value: string }
  | { type: "blocked" }
  | { type: "failed" }
  | { type: "loading" }
  | { type: "title"; value: string }
  | { type: "commit"; origin: string };

export function closedBrowser(): BrowserView {
  return {
    availability: "closed",
    proof: "not_run",
    origin: "",
    draft: "",
    pageTitle: "",
    status: "idle",
  };
}

/** UI fixture. Proof stays `not_run`; this does not open a WebView. */
export function openBrowserFixture(origin: string): BrowserView {
  return { ...closedBrowser(), availability: "open", origin, draft: origin };
}

export function browserReducer(state: BrowserView, action: BrowserAction): BrowserView {
  if (state.availability !== "open") return state;
  switch (action.type) {
    case "draft":
      return { ...state, draft: action.value };
    case "loading":
      return { ...state, status: "loading" };
    case "blocked":
      return { ...state, status: "blocked" };
    case "failed":
      return { ...state, status: "failed" };
    case "title":
      return { ...state, pageTitle: action.value };
    case "commit":
      return { ...state, origin: action.origin, draft: action.origin, status: "idle" };
    default:
      return state;
  }
}

/** Escape bidi and control characters in untrusted page text. */
export function escapeUntrusted(text: string): string {
  let out = "";
  for (const ch of text) {
    const code = ch.codePointAt(0) ?? 0;
    const hidden =
      code < 0x20 ||
      code === 0x7f ||
      code === 0xfeff ||
      (code >= 0x200b && code <= 0x200f) ||
      (code >= 0x202a && code <= 0x202e) ||
      (code >= 0x2066 && code <= 0x2069);
    out += hidden ? `\\u{${code.toString(16)}}` : ch;
  }
  return out;
}
