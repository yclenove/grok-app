/** @vitest-environment jsdom */
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { ComputerTaskCardLive } from "./ComputerTaskCardLive";
import * as api from "@/lib/api/computerUse";

vi.mock("@/lib/api/computerUse", () => ({
  computerStatus: vi.fn(),
  computerPause: vi.fn(),
  computerTakeover: vi.fn(),
  computerStop: vi.fn(),
}));

afterEach(() => { cleanup(); vi.useRealTimers(); });

describe("ComputerTaskCardLive", () => {
  beforeEach(() => {
    vi.resetAllMocks();
  });

  it("keeps the same run stoppable after polling fails, but never across chats", async () => {
    vi.useFakeTimers();
    const status: api.ComputerStatus = {
      runId: "private-run", targetId: "private-target", targetName: "Private document",
      featureEnabled: true, enabled: true, paused: false, stopState: "running", backend: "windows",
      notes: [], traces: [], recovery: null, timings: null, targetAlive: true, mcpCatalog: null,
    };
    vi.mocked(api.computerStatus).mockResolvedValueOnce(status).mockRejectedValue(new Error("offline"));
    const view = render(<ComputerTaskCardLive locale="en" sessionId="first-chat" />);
    await act(async () => {});
    await act(async () => { await vi.advanceTimersByTimeAsync(800); });
    expect(screen.getByText(/Status unknown/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Pause" })).toBeDisabled();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Stop" })); });
    expect(api.computerStop).toHaveBeenCalledWith({ sessionId: "first-chat", runId: "private-run" });
    view.rerender(<ComputerTaskCardLive locale="en" sessionId="second-chat" />);
    expect(screen.queryByText("Private document")).toBeNull();
    await act(async () => {});
    expect(screen.queryByRole("button", { name: "Stop" })).toBeNull();
  });

  it("deduplicates Pause while allowing Stop to overtake it", async () => {
    let finish!: () => void;
    vi.mocked(api.computerPause).mockImplementation(() => new Promise<void>((resolve) => { finish = resolve; }));
    vi.mocked(api.computerStatus).mockResolvedValue({
      runId: "run", targetId: "window", targetName: "Editor", featureEnabled: true,
      enabled: true, paused: false, stopState: "running", backend: "windows",
      notes: [], traces: [], recovery: null, timings: null, targetAlive: true, mcpCatalog: null,
    });
    render(<ComputerTaskCardLive locale="en" sessionId="chat" />);
    const pause = await screen.findByRole("button", { name: "Pause" });
    fireEvent.click(pause); fireEvent.click(pause);
    expect(api.computerPause).toHaveBeenCalledTimes(1);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Stop" })); });
    expect(api.computerStop).toHaveBeenCalledTimes(1);
    await act(async () => { finish(); });
  });

  it("expires the green status while a subsequent Host query is hung", async () => {
    vi.useFakeTimers();
    vi.mocked(api.computerStatus).mockResolvedValueOnce({
      runId: "run", targetId: "window", targetName: "Editor", featureEnabled: true,
      enabled: true, paused: false, stopState: "running", backend: "windows",
      notes: [], traces: [], recovery: null, timings: null, targetAlive: true, mcpCatalog: null,
    }).mockImplementation(() => new Promise(() => {}));
    render(<ComputerTaskCardLive locale="en" sessionId="chat" />);
    await act(async () => {});
    await act(async () => { await vi.advanceTimersByTimeAsync(3900); });
    expect(screen.getByText(/Status unknown/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Stop" })).toBeEnabled();
    expect(api.computerStatus).toHaveBeenCalledTimes(2);
  });

  it("maps a dead authorized target onto the closed task copy", async () => {
    vi.mocked(api.computerStatus).mockResolvedValue({
      featureEnabled: true,
      enabled: true,
      runId: "run",
      targetId: "window",
      targetName: "Editor",
      paused: false,
      stopState: "running",
      backend: "test",
      notes: [],
      traces: [],
      recovery: null,
      timings: null,
      targetAlive: false,
      mcpCatalog: null,
    });
    render(<ComputerTaskCardLive locale="en" sessionId="chat" />);
    expect(await screen.findByText("The window closed. Choose a target again.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Pause$/ })).toBeDisabled();
  });

  it("keeps Pause enabled while the Host target is still alive", async () => {
    vi.mocked(api.computerStatus).mockResolvedValue({
      featureEnabled: true,
      enabled: true,
      runId: "run",
      targetId: "window",
      targetName: "Editor",
      paused: false,
      stopState: "running",
      backend: "test",
      notes: [],
      traces: [],
      recovery: null,
      timings: null,
      targetAlive: true,
      mcpCatalog: null,
    });
    render(<ComputerTaskCardLive locale="en" sessionId="chat" />);
    await waitFor(() => expect(screen.getByRole("button", { name: /^Pause$/ })).toBeEnabled());
  });

  it("shows Host target, backend, and failure instead of a hardcoded running label", async () => {
    vi.mocked(api.computerStatus).mockResolvedValue({
      featureEnabled: true,
      enabled: true,
      runId: "run",
      targetId: "window",
      targetName: "Editor",
      paused: false,
      stopState: "running",
      backend: "windows",
      notes: [],
      traces: [{
        kind: "act",
        runId: "run",
        detail: "click Rejected executed=false",
        ms: 12,
        audience: "ui",
      }],
      recovery: null,
      timings: null,
      targetAlive: true,
      mcpCatalog: null,
    });
    render(<ComputerTaskCardLive locale="en" sessionId="chat" />);
    expect(await screen.findByText(/Editor/)).toBeInTheDocument();
    expect(screen.getByText("Backend: windows")).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("Could not run that action.");
    expect(screen.queryByText(/^Running$/)).toBeNull();
  });

  it("maps Host stopped onto Stopped and still names the backend", async () => {
    vi.mocked(api.computerStatus).mockResolvedValue({
      featureEnabled: true,
      enabled: true,
      runId: "run",
      targetId: "window",
      targetName: "Editor",
      paused: false,
      stopState: "stopped",
      backend: "windows",
      notes: [],
      traces: [],
      recovery: null,
      timings: null,
      targetAlive: true,
      mcpCatalog: null,
    });
    render(<ComputerTaskCardLive locale="en" sessionId="chat" />);
    expect(await screen.findByText("Stopped")).toBeInTheDocument();
    expect(screen.getByText("Backend: windows")).toBeInTheDocument();
    expect(screen.queryByText(/Operating/)).toBeNull();
  });

  it("does not report stopped while MCP cleanup is still pending", async () => {
    vi.mocked(api.computerStatus).mockResolvedValue({
      featureEnabled: true,
      enabled: false,
      runId: "run",
      targetId: "window",
      targetName: "Editor",
      paused: false,
      stopState: "stopped",
      backend: "windows",
      notes: [],
      traces: [],
      recovery: null,
      timings: null,
      targetAlive: true,
      mcpCatalog: {
        desiredGeneration: 3,
        appliedGeneration: 2,
        desiredPresent: false,
        pending: true,
        cleanupPending: true,
        lastError: "timeout",
      },
    });
    render(<ComputerTaskCardLive locale="en" sessionId="chat" />);
    expect(await screen.findByText("Removing Computer Use tools…")).toBeInTheDocument();
    expect(screen.queryByText("Stopped")).toBeNull();
  });

  it("falls back to the Host target id when the title is empty", async () => {
    vi.mocked(api.computerStatus).mockResolvedValue({
      featureEnabled: true,
      enabled: true,
      runId: "run",
      targetId: "hwnd:42",
      targetName: null,
      paused: false,
      stopState: "running",
      backend: "windows",
      notes: [],
      traces: [],
      recovery: null,
      timings: null,
      targetAlive: true,
      mcpCatalog: null,
    });
    render(<ComputerTaskCardLive locale="en" sessionId="chat" />);
    expect(await screen.findByText(/hwnd:42/)).toBeInTheDocument();
    expect(screen.getByText("Backend: windows")).toBeInTheDocument();
  });
});
