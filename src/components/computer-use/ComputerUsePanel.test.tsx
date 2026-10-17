/**
 * @vitest-environment jsdom
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ImageViewerApi } from "@/components/ImageViewerContext";
import { ShotBook, type AdmittedShot } from "@/lib/computer-use/events";
import { setMediaEndpoint } from "@/lib/imageSrc";
import { applyComputerUseNotice } from "@/hooks/useComputerUseEvents";
import { TimelineStore, type TimelineInput } from "@/lib/computer-use/store";
import { ComputerUsePanel } from "./ComputerUsePanel";

afterEach(() => {
  cleanup();
  setMediaEndpoint(null);
  window.location.hash = "";
});

function step(sessionId: string, target: string, over: Partial<TimelineInput> = {}): TimelineInput {
  return { sessionId, tool: "click", target, outcome: "done", durationMs: 12, ...over };
}

function viewer(): ImageViewerApi {
  return { open: vi.fn(), close: vi.fn(), isOpen: () => false, copyImage: async () => false };
}

function shot(sessionId: string, id: string): AdmittedShot {
  return {
    sessionId,
    id,
    thumbPath: "/tmp/cu-shot-thumb.png",
    fullPath: "/tmp/cu-shot-full.png",
    source: "redacted",
  };
}

function panel(store: TimelineStore, props: Partial<Parameters<typeof ComputerUsePanel>[0]> = {}) {
  return render(
    <ComputerUsePanel sessionId={props.sessionId ?? "sess-a"} locale="en" store={store} onControl={vi.fn()} {...props} />,
  );
}

describe("ComputerUsePanel", () => {
  it("keeps ten steps and shows the throttled remainder", () => {
    const store = new TimelineStore();
    const sample = step("sess-a", "Save");
    for (let index = 0; index < 11; index += 1) store.push(sample, 50);
    panel(store);
    expect(screen.getAllByTestId("cu-panel-step")).toHaveLength(10);
    expect(screen.getByTestId("cu-panel-dropped")).toHaveTextContent("1");
    expect(screen.queryByTestId("cu-panel-empty")).toBeNull();
  });

  it("shows only the selected session", () => {
    const store = new TimelineStore();
    store.push(step("sess-a", "Alpha"), 0);
    store.push(step("sess-b", "Beta"), 0);
    panel(store, { sessionId: "sess-b" });
    expect(screen.getByText("Beta")).toBeInTheDocument();
    expect(screen.queryByText("Alpha")).toBeNull();
  });

  it("offers settings from the empty state", () => {
    panel(new TimelineStore());
    expect(screen.getByTestId("cu-panel-empty")).toHaveTextContent("Computer Use is idle");
    fireEvent.click(screen.getByTestId("cu-panel-settings"));
    expect(window.location.hash).toBe("#/settings/extensions/computer");
  });

  it("shows a failed step and a control error without the raw message", async () => {
    const store = new TimelineStore();
    store.push(step("sess-a", "Save", { outcome: "failed" }), 0);
    const onControl = vi.fn().mockRejectedValue(new Error("secret diagnostic"));
    panel(store, { onControl });
    expect(screen.getByTestId("cu-panel")).toHaveAttribute("data-phase", "failed");
    fireEvent.click(screen.getByTestId("cu-panel-stop"));
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not update Computer Use.");
    expect(screen.queryByText("secret diagnostic")).toBeNull();
    expect(onControl).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("cu-panel")).toHaveAttribute("data-phase", "failed");
    expect(store.userResume()).toBe(false);
  });

  it("stops on the button and ignores a later done step until Resume", async () => {
    const store = new TimelineStore();
    store.push(step("sess-a", "Save"), 0);
    expect(store.userResume()).toBe(false);
    const onControl = vi.fn().mockResolvedValue(undefined);
    panel(store, { onControl });
    fireEvent.click(screen.getByTestId("cu-panel-stop"));
    expect(await screen.findByTestId("cu-panel-resume")).toBeInTheDocument();
    expect(screen.getByTestId("cu-panel")).toHaveAttribute("data-phase", "stopped");
    store.push(step("sess-a", "Again", { outcome: "done" }), 2_000);
    expect(screen.getByTestId("cu-panel")).toHaveAttribute("data-phase", "stopped");
    fireEvent.click(screen.getByTestId("cu-panel-resume"));
    expect(onControl).toHaveBeenLastCalledWith("resume", expect.any(Object));
    await waitFor(() => {
      expect(screen.getByTestId("cu-panel")).toHaveAttribute("data-phase", "running");
    });
  });

  it("opens the shared viewer on the full path and previews over loopback", async () => {
    setMediaEndpoint({ baseUrl: "http://127.0.0.1:9", token: "tok" });
    const store = new TimelineStore();
    const book = new ShotBook();
    const eyes = viewer();
    store.push(step("sess-a", "Save", { shotId: "shot-a" }), 0);
    panel(store, { book, viewer: eyes, loadShot: async () => shot("sess-a", "shot-a") });
    const thumb = await screen.findByTestId("cu-panel-thumb");
    const img = thumb.querySelector("img");
    expect(img?.getAttribute("src") ?? "").toMatch(/^http:\/\/127\.0\.0\.1:9\/v1\/media\?/);
    expect(img?.getAttribute("src") ?? "").not.toContain("media://");
    fireEvent.click(thumb);
    expect(eyes.open).toHaveBeenCalledWith([expect.objectContaining({ src: "/tmp/cu-shot-full.png" })]);
  });

  it("drops one session screenshot and keeps the other", async () => {
    setMediaEndpoint({ baseUrl: "http://127.0.0.1:9", token: "tok" });
    const store = new TimelineStore();
    const book = new ShotBook();
    book.admit(shot("sess-a", "shot-a"));
    book.admit(shot("sess-b", "shot-b"));
    store.push(step("sess-a", "Alpha", { shotId: "shot-a" }), 0);
    store.push(step("sess-b", "Beta", { shotId: "shot-b" }), 0);
    applyComputerUseNotice(store, { kind: "forgotten", sessionId: "sess-a", revision: 1 }, book);
    const { unmount } = panel(store, { sessionId: "sess-b", book, loadShot: async () => "missing" });
    expect(await screen.findByTestId("cu-panel-thumb")).toBeInTheDocument();
    unmount();
    panel(store, { sessionId: "sess-a", book, loadShot: async () => shot("sess-a", "shot-a") });
    expect(screen.queryByTestId("cu-panel-thumb")).toBeNull();
    expect(book.resolve("sess-b", "shot-b")).toEqual({
      thumb: "/tmp/cu-shot-thumb.png",
      full: "/tmp/cu-shot-full.png",
    });
  });

  it("refuses a screenshot owned by another session", async () => {
    const store = new TimelineStore();
    const book = new ShotBook();
    book.admit(shot("sess-a", "shot-a"));
    store.push(step("sess-b", "Beta", { shotId: "shot-a" }), 0);
    panel(store, { sessionId: "sess-b", book, loadShot: async () => shot("sess-b", "shot-a") });
    expect(await screen.findByText("This screenshot belongs to another chat.")).toBeInTheDocument();
    expect(screen.queryByTestId("cu-panel-thumb")).toBeNull();
  });

  it("shows approval ahead of handoff and restart", () => {
    const store = new TimelineStore();
    store.push(step("sess-a", "Save"), 0);
    panel(store, { waiting: true, waitingTarget: "Mail", handoff: true });
    expect(screen.getByTestId("cu-panel")).toHaveAttribute("data-phase", "waiting");
    expect(screen.getByTestId("cu-panel-status")).toHaveTextContent("Waiting for your approval of Mail");
  });

  it("activates Stop from the keyboard and ignores Enter elsewhere", async () => {
    const user = userEvent.setup();
    const store = new TimelineStore();
    store.push(step("sess-a", "Save"), 0);
    const onControl = vi.fn().mockResolvedValue(undefined);
    panel(store, { onControl });
    fireEvent.keyDown(document, { key: "Enter" });
    expect(onControl).not.toHaveBeenCalled();
    await user.tab();
    expect(screen.getByTestId("cu-panel-stop")).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(await screen.findByTestId("cu-panel-resume")).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(onControl).toHaveBeenLastCalledWith("resume", expect.any(Object));
  });
});
