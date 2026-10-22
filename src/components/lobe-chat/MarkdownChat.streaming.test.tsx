/** @vitest-environment jsdom */
import { StrictMode } from "react";
import type { Options } from "react-markdown";
import { act, cleanup, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const observed = vi.hoisted(() => ({ sources: [] as string[] }));
vi.mock("react-markdown", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react-markdown")>();
  return {
    ...actual,
    default: (props: Options) => {
      observed.sources.push(props.children ?? "");
      return actual.default(props);
    },
  };
});

import { MarkdownChat } from "./MarkdownChat";

beforeEach(() => {
  vi.useFakeTimers();
  observed.sources.length = 0;
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("MarkdownChat sustained streaming", () => {
  it("keeps painting a long paragraph while chunks arrive faster than parsing", () => {
    let text = "**Start** " + "word ".repeat(2800);
    const view = render(<MarkdownChat streaming>{text}</MarkdownChat>);
    for (let tick = 0; tick < 20; tick += 1) {
      act(() => vi.advanceTimersByTime(40));
      text += ` delta${tick} `;
      view.rerender(<MarkdownChat streaming>{text}</MarkdownChat>);
    }
    expect(view.container.textContent).toContain("delta6");
    expect(view.container.querySelector("strong")?.textContent).toBe("Start");
    act(() => vi.advanceTimersByTime(280));
    expect(view.container.textContent).toContain("delta19");
  });

  it("does not reparse unchanged painted text before the next deadline", () => {
    const initial = "**Start** " + "word ".repeat(2800);
    const view = render(<MarkdownChat streaming>{initial}</MarkdownChat>);
    const initialParses = observed.sources.length;
    view.rerender(<MarkdownChat streaming>{initial + " next"}</MarkdownChat>);
    act(() => vi.advanceTimersByTime(100));
    expect(observed.sources).toHaveLength(initialParses);
    act(() => vi.advanceTimersByTime(180));
    expect(view.container.textContent).toContain("next");
    expect(observed.sources).toHaveLength(initialParses + 1);
  });

  it("keeps the parsed tree when only wrapper appearance changes", () => {
    const text = "**Stored paragraph** " + "word ".repeat(200);
    const view = render(<MarkdownChat>{text}</MarkdownChat>);
    const initialParses = observed.sources.length;
    view.rerender(<MarkdownChat muted className="example">{text}</MarkdownChat>);
    expect(view.container.firstElementChild?.classList.contains("example")).toBe(true);
    expect(view.container.firstElementChild?.classList.contains("chat-md--muted")).toBe(true);
    expect(observed.sources).toHaveLength(initialParses);
  });

  it("paints exact final content immediately and cancels the pending stream", () => {
    const view = render(<MarkdownChat streaming>{"**start**"}</MarkdownChat>);
    view.rerender(<MarkdownChat streaming>{"**pending**"}</MarkdownChat>);
    view.rerender(<MarkdownChat>{"**final** and unmatched *"}</MarkdownChat>);
    expect(view.container.textContent).toBe("final and unmatched *");
    expect(vi.getTimerCount()).toBe(0);
    act(() => vi.advanceTimersByTime(1000));
    expect(view.container.textContent).toBe("final and unmatched *");
  });

  it("updates find occurrence ownership without reparsing the wrong segment", () => {
    const text = "foo and foo";
    const view = render(
      <MarkdownChat findQuery="foo" findActiveOccurrence={2} findOccurrenceBase={0}>
        {text}
      </MarkdownChat>,
    );
    expect(view.container.querySelectorAll("[data-find-mark='current']")).toHaveLength(0);
    view.rerender(
      <MarkdownChat findQuery="foo" findActiveOccurrence={2} findOccurrenceBase={2}>
        {text}
      </MarkdownChat>,
    );
    expect(view.container.querySelectorAll("[data-find-mark='current']")).toHaveLength(1);
    expect(view.container.querySelector("[data-find-mark='current']")?.textContent).toBe("foo");
    view.rerender(
      <MarkdownChat findQuery="and" findActiveOccurrence={0}>{text}</MarkdownChat>,
    );
    expect(view.container.querySelectorAll("[data-find-mark]")).toHaveLength(1);
    expect(view.container.querySelector("[data-find-mark='current']")?.textContent).toBe("and");
  });

  it("uses latest content at a pending deadline across the length threshold", () => {
    const first = "**first** " + "x".repeat(1980);
    const view = render(<MarkdownChat streaming>{first}</MarkdownChat>);
    view.rerender(<MarkdownChat streaming>{first + " A"}</MarkdownChat>);
    act(() => vi.advanceTimersByTime(60));
    view.rerender(<MarkdownChat streaming>{first + " A" + "y".repeat(30)}</MarkdownChat>);
    act(() => vi.advanceTimersByTime(50));
    expect(view.container.textContent?.endsWith("y".repeat(30))).toBe(true);
  });

  it("cleans pending work on unmount and supports StrictMode replay", () => {
    const view = render(<StrictMode><MarkdownChat streaming>{"**first**"}</MarkdownChat></StrictMode>);
    view.rerender(<StrictMode><MarkdownChat streaming>{"**second**"}</MarkdownChat></StrictMode>);
    act(() => vi.advanceTimersByTime(110));
    expect(view.container.textContent).toBe("second");
    view.rerender(<StrictMode><MarkdownChat streaming>{"**third**"}</MarkdownChat></StrictMode>);
    view.unmount();
    expect(vi.getTimerCount()).toBe(0);
  });
});
