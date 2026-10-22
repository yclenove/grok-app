/** @vitest-environment jsdom */
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { useState } from "react";
import { createPortal } from "react-dom";
import { useReviewMetrics } from "./useReviewMetrics";

const originalHeight = Object.getOwnPropertyDescriptor(window, "innerHeight");

function Harness() {
  const metrics = useReviewMetrics({
    open: true, surface: "content", locale: "zh", theme: "dark", reader: "idle",
    layout: "wide", measureWidth: 1040, localeReady: true,
  });
  return <>
    <div className="session-import-review-frame">
      <div className="modal-body"><div data-import-candidate="1">Synthetic row</div></div>
      <div className="session-import__footer">Synthetic footer</div>
    </div>
    <pre data-testid="metrics">{metrics}</pre>
  </>;
}

afterEach(() => {
  cleanup();
  if (originalHeight) Object.defineProperty(window, "innerHeight", originalHeight);
});

test("height-only viewport changes update recorded metrics", async () => {
  Object.defineProperty(window, "innerHeight", { configurable: true, value: 800 });
  render(<Harness />);
  expect(JSON.parse(screen.getByTestId("metrics").textContent!).viewport.height).toBe(800);
  Object.defineProperty(window, "innerHeight", { configurable: true, value: 480 });
  fireEvent(window, new Event("resize"));
  await waitFor(() => expect(JSON.parse(screen.getByTestId("metrics").textContent!).viewport.height).toBe(480));
  const metrics = JSON.parse(screen.getByTestId("metrics").textContent!);
  expect(metrics.localeReady).toBe(true);
  expect(metrics.footerInBody).toBe(false);
});

test("content geometry changes trigger a new measurement without a width change", async () => {
  render(<Harness />);
  const body = document.querySelector(".modal-body")!;
  Object.defineProperty(body, "scrollHeight", { configurable: true, value: 800 });
  Object.defineProperty(body, "clientHeight", { configurable: true, value: 400 });
  body.append(document.createElement("p"));
  await waitFor(() => expect(JSON.parse(screen.getByTestId("metrics").textContent!).bodyOverflow).toBe(true));
  const metrics = JSON.parse(screen.getByTestId("metrics").textContent!);
  expect(metrics.bodyScrollHeight).toBe(800);
  expect(metrics.bodyClientHeight).toBe(400);
});

test("a late portal is observed for later geometry changes and removal", async () => {
  function DeferredHarness() {
    const [open, setOpen] = useState(false);
    const metrics = useReviewMetrics({
      open: true, surface: "continuation", locale: "en", theme: "dark", reader: "idle",
      layout: "wide", measureWidth: 0, localeReady: true,
    });
    return <>
      <button type="button" onClick={() => setOpen((value) => !value)}>Toggle synthetic portal</button>
      <pre data-testid="metrics">{metrics}</pre>
      {open ? createPortal(<div className="session-import-review-frame">
        <div className="modal-body">Synthetic deferred body</div>
        <div className="modal-actions">Synthetic creation footer</div>
      </div>, document.body) : null}
    </>;
  }
  render(<DeferredHarness />);
  const read = () => JSON.parse(screen.getByTestId("metrics").textContent!);
  expect(read().modal).toBeNull();
  fireEvent.click(screen.getByRole("button"));
  await waitFor(() => expect(read().modal).not.toBeNull());
  const body = document.querySelector(".modal-body")!;
  Object.defineProperty(body, "scrollHeight", { configurable: true, value: 600 });
  Object.defineProperty(body, "clientHeight", { configurable: true, value: 300 });
  body.append(document.createElement("p"));
  await waitFor(() => expect(read().bodyScrollHeight).toBe(600));
  expect(read().footerInBody).toBe(false);
  fireEvent.click(screen.getByRole("button"));
  await waitFor(() => expect(read().modal).toBeNull());
});
