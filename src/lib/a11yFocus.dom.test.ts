/** @vitest-environment jsdom */
import { expect, test } from "vitest";
import { listFocusable } from "./a11yFocus";

test("listFocusable skips controls under hidden, inert, or aria-hidden ancestors", () => {
  const root = document.createElement("div");
  root.innerHTML = [
    "<section hidden><button id='hidden-action'>hidden</button></section>",
    "<section inert><button id='inert-action'>inert</button></section>",
    "<section aria-hidden='true'><button id='aria-action'>aria</button></section>",
    "<section style='display:none'><button id='display-action'>display</button></section>",
    "<button id='visible-action'>visible</button>",
    "<button id='fixed-action' style='position:fixed'>fixed</button>",
  ].join("");
  document.body.append(root);
  try {
    expect(listFocusable(root).map((el) => el.id)).toEqual([
      "visible-action",
      "fixed-action",
    ]);
  } finally {
    root.remove();
  }
});
