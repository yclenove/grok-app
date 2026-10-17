/**
 * @vitest-environment jsdom
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { ImageViewerApi } from "@/components/ImageViewerContext";
import { ComputerUseToolCard } from "./ComputerUseToolCard";
import {
  isComputerUseTool,
  previewUrl,
  ShotBook,
  type AdmittedShot,
} from "@/lib/computer-use/events";
import { setMediaEndpoint } from "@/lib/imageSrc";
import type { MessageToolSegment } from "@/lib/session";

afterEach(() => {
  cleanup();
  setMediaEndpoint(null);
});

function tool(over: Partial<MessageToolSegment> = {}): MessageToolSegment {
  return {
    kind: "tool",
    toolCallId: "call-1",
    title: "grok-computer-use__click",
    status: "completed",
    ...over,
  };
}

function payload(action: Record<string, unknown>, state?: Record<string, unknown>): string {
  return JSON.stringify({ action, ...(state ? { state } : {}) });
}

const clickDone = {
  tool: "click",
  target: { element_index: 3, label: "Save" },
  outcome: "done",
  effect: "changed",
  duration_ms: 420,
};

function viewer(): ImageViewerApi {
  return {
    open: vi.fn(),
    close: vi.fn(),
    isOpen: () => false,
    copyImage: async () => false,
  };
}

function shot(sessionId: string, id = "i_1"): AdmittedShot {
  return {
    sessionId,
    id,
    thumbPath: "/tmp/cu-shot-thumb.png",
    fullPath: "/tmp/cu-shot-full.png",
    source: "redacted",
  };
}

describe("ComputerUseToolCard", () => {
  it("ignores a bare tool name without the action result shape", () => {
    expect(isComputerUseTool({ title: "click", status: "completed" })).toBe(false);
    expect(isComputerUseTool(tool({ status: "pending" }))).toBe(true);
  });

  it("shows a pending badge without an effect", () => {
    render(<ComputerUseToolCard tool={tool({ status: "pending", output: "" })} locale="en" />);
    expect(screen.getByTestId("cu-tool-badge")).toHaveTextContent("In progress");
    expect(screen.getByTestId("cu-tool-badge")).not.toHaveTextContent("Changed");
    expect(screen.getByTestId("cu-tool-card")).toHaveClass("is-running");
    expect(screen.queryByRole("img")).toBeNull();
  });

  it("shows done, the target, and a sub-second duration", () => {
    render(
      <ComputerUseToolCard
        locale="en"
        tool={tool({
          input: JSON.stringify({ app: "Notes" }),
          output: payload(clickDone, { app_id: "Notes" }),
        })}
      />,
    );
    expect(screen.getByTestId("cu-tool-badge")).toHaveTextContent("Done · Changed");
    expect(screen.getByTestId("cu-tool-target")).toHaveTextContent("Notes · Save");
    expect(screen.getByTestId("cu-tool-duration")).toHaveTextContent("420 ms");
    expect(screen.getByTestId("cu-tool-card")).not.toHaveClass("is-error");
  });

  it("shows refused as an error", () => {
    render(
      <ComputerUseToolCard
        locale="en"
        tool={tool({
          output: payload({ ...clickDone, outcome: "refused", effect: "no_visible_change" }),
        })}
      />,
    );
    expect(screen.getByTestId("cu-tool-badge")).toHaveTextContent("Refused · No visible change");
    expect(screen.getByTestId("cu-tool-card")).toHaveClass("is-error");
  });

  it("shows failed when the host status failed", () => {
    render(
      <ComputerUseToolCard locale="en" tool={tool({ status: "failed", output: "" })} />,
    );
    expect(screen.getByTestId("cu-tool-badge")).toHaveTextContent("Failed · Not checked");
    expect(screen.getByTestId("cu-tool-card")).toHaveClass("is-error");
  });

  it("shows unknown with effect and delivery even when status is completed", () => {
    render(
      <ComputerUseToolCard
        locale="en"
        tool={tool({
          status: "completed",
          output: payload(
            {
              ...clickDone,
              outcome: "unknown",
              effect: "changed",
              delivery: "accessibility",
              duration_ms: 10,
            },
            { app_id: "Mail", screenshot: { id: "i_9", width: 1, height: 1, mime: "image/png" } },
          ),
        })}
      />,
    );
    expect(screen.getByTestId("cu-tool-badge")).toHaveTextContent(
      "Unknown · Changed · Accessibility",
    );
    expect(screen.getByTestId("cu-tool-card")).toHaveClass("is-warning");
    expect(screen.getByTestId("cu-tool-duration")).toHaveTextContent("10 ms");
  });

  it("does not use a screenshot path printed in the raw result", () => {
    const leaked = "Screenshot file: /tmp/leaked.png";
    render(
      <ComputerUseToolCard
        locale="en"
        tool={tool({ output: `${leaked}\nAction: click — done (changed).` })}
      />,
    );
    expect(screen.queryByRole("img")).toBeNull();
    expect(document.querySelector("[src*='leaked']")).toBeNull();
    expect(screen.queryByTestId("cu-tool-raw")).toBeNull();
    fireEvent.click(screen.getByTestId("cu-tool-expand"));
    expect(screen.getByTestId("cu-tool-expand")).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByTestId("cu-tool-raw")).toHaveTextContent("Screenshot file:");
    expect(document.querySelector("img")).toBeNull();
  });

  it("opens the existing viewer on the full path and previews over loopback", async () => {
    setMediaEndpoint({ baseUrl: "http://127.0.0.1:9", token: "tok" });
    const book = new ShotBook();
    const eyes = viewer();
    const admitted = shot("sess-a");
    render(
      <ComputerUseToolCard
        locale="en"
        sessionId="sess-a"
        book={book}
        viewer={eyes}
        loadShot={async () => admitted}
        tool={tool({
          output: payload(clickDone, {
            app_id: "Notes",
            screenshot: { id: "i_1", width: 2, height: 2, mime: "image/png" },
          }),
        })}
      />,
    );
    const thumb = await screen.findByTestId("cu-tool-thumb");
    const img = thumb.querySelector("img");
    expect(img?.getAttribute("src") ?? "").toMatch(/^http:\/\/127\.0\.0\.1:9\/v1\/media\?/);
    expect(img?.getAttribute("src") ?? "").not.toContain("media://");
    fireEvent.click(thumb);
    expect(eyes.open).toHaveBeenCalledWith([
      expect.objectContaining({ src: "/tmp/cu-shot-full.png" }),
    ]);
  });

  it("refuses a screenshot owned by another session", async () => {
    setMediaEndpoint({ baseUrl: "http://127.0.0.1:9", token: "tok" });
    const book = new ShotBook();
    expect(book.admit(shot("sess-a"))).toBeNull();
    const eyes = viewer();
    render(
      <ComputerUseToolCard
        locale="en"
        sessionId="sess-b"
        book={book}
        viewer={eyes}
        loadShot={async () => shot("sess-b")}
        tool={tool({
          output: payload(clickDone, {
            screenshot: { id: "i_1", width: 2, height: 2, mime: "image/png" },
          }),
        })}
      />,
    );
    expect(await screen.findByText("This screenshot belongs to another chat.")).toBeInTheDocument();
    expect(screen.queryByTestId("cu-tool-thumb")).toBeNull();
    expect(eyes.open).not.toHaveBeenCalled();
    expect(book.resolve("sess-a", "i_1")).toEqual({
      thumb: "/tmp/cu-shot-thumb.png",
      full: "/tmp/cu-shot-full.png",
    });
  });

  it("drops the thumbnail when the session ledger is cleared", async () => {
    setMediaEndpoint({ baseUrl: "http://127.0.0.1:9", token: "tok" });
    const book = new ShotBook();
    book.admit(shot("sess-a"));
    render(
      <ComputerUseToolCard
        locale="en"
        sessionId="sess-a"
        book={book}
        loadShot={async () => shot("sess-a")}
        tool={tool({
          output: payload(clickDone, {
            screenshot: { id: "i_1", width: 2, height: 2, mime: "image/png" },
          }),
        })}
      />,
    );
    expect(await screen.findByTestId("cu-tool-thumb")).toBeInTheDocument();
    act(() => book.dropSession("sess-a"));
    expect(screen.queryByTestId("cu-tool-thumb")).toBeNull();
    expect(book.resolve("sess-a", "i_1")).toBe("missing");
  });

  it("rejects a non-loopback preview url", () => {
    expect(previewUrl("media://localhost/tmp/cu-shot-full.png")).toBeNull();
    expect(previewUrl("/tmp/leaked.png")).toBeNull();
  });
});
