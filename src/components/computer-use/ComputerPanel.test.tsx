/** @vitest-environment jsdom */
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { ComputerPanel } from "./ComputerPanel";
import * as api from "@/lib/api/computerUse";
import * as system from "@/lib/api/system";

vi.mock("@/lib/api/computerUse", () => ({
  computerStatus: vi.fn(), computerListTargets: vi.fn(), computerSetPreview: vi.fn(),
  computerAuthorizeSurface: vi.fn(), computerCancelAuthorization: vi.fn(),
  computerObserve: vi.fn(), computerPause: vi.fn(),
  computerResume: vi.fn(), computerTakeover: vi.fn(), computerStop: vi.fn(),
  computerRetryCleanup: vi.fn(),
  computerBeginPairing: vi.fn(), computerConfirmPairingApp: vi.fn(),
  computerRevokePairing: vi.fn(), computerListSharedTabs: vi.fn(),
  computerUnbindWebview: vi.fn(),
}));

vi.mock("@/lib/api/system", () => ({
  sideBrowserList: vi.fn(),
}));

const state = (patch: Partial<api.ComputerStatus> = {}): api.ComputerStatus => ({
  featureEnabled: true, enabled: false, runId: null, targetId: null, targetName: null,
  paused: false, stopState: "stopped", backend: "test", notes: [], traces: [],
  recovery: null, timings: null, targetAlive: false, mcpCatalog: null, ...patch,
});

const running = () => state({ runId: "run", enabled: true, targetId: "window", targetName: "Editor", stopState: "running", targetAlive: true });

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(api.computerStatus).mockResolvedValue(state());
  vi.mocked(api.computerListTargets).mockResolvedValue([{ targetId: "window", title: "Editor", appName: "Editor", kind: "window" }]);
  vi.mocked(api.computerCancelAuthorization).mockResolvedValue(false);
  vi.mocked(api.computerSetPreview).mockResolvedValue();
  vi.mocked(api.computerObserve).mockResolvedValue({ snapshotId: "frame", geometryRevision: 1, previewDataUrl: "data:image/png;base64,AAAA" });
  vi.mocked(api.computerBeginPairing).mockResolvedValue({ nonce: "n1", instanceId: "inst", verificationCode: "ABCDE-ABCDE-ABCDE-ABCDE", endpoint: "http://127.0.0.1:12345", expiresAtMs: Date.now() + 300_000, installedExtensionId: "ext" });
  vi.mocked(api.computerConfirmPairingApp).mockResolvedValue();
  vi.mocked(api.computerRevokePairing).mockResolvedValue();
  vi.mocked(api.computerListSharedTabs).mockResolvedValue([]);
  vi.mocked(api.computerUnbindWebview).mockResolvedValue();
  vi.mocked(api.computerRetryCleanup).mockResolvedValue({
    desiredGeneration: 1,
    appliedGeneration: 1,
    desiredPresent: false,
    pending: false,
    cleanupPending: false,
    lastError: null,
  });
  vi.mocked(api.computerAuthorizeSurface).mockImplementation(async (opts) => ({
    attemptId: opts.attemptId,
    selectorRevision: opts.selectorRevision,
    runId: "authorized-run",
    target: opts.surface === "managed-browser"
      ? { targetId: "managed:alice:page-1", title: "managed", appName: "managed-browser", kind: "managed-tab", surface: "managed-browser" }
      : opts.surface === "existing-tabs"
        ? { targetId: "tab-1", title: "Shared", appName: "existing-tabs", kind: "existing-tab", surface: "existing-tabs" }
        : opts.surface === "app-webview"
          ? { targetId: "wv|tab", title: "CU-D5", appName: "webview", kind: "webview", surface: "app-webview" }
          : { targetId: opts.targetId, title: "Editor", appName: "Editor", kind: "window", surface: "desktop" },
  }));
  vi.mocked(system.sideBrowserList).mockResolvedValue([{ label: "resource-browser-cu" }]);
  Object.defineProperty(document, "hidden", { configurable: true, value: false });
});
afterEach(() => { cleanup(); vi.useRealTimers(); });

describe("Computer panel lifecycle", () => {
  it("offers explicit system consent without a target selector, fake preview or broad single-app promise", async () => {
    vi.mocked(api.computerStatus).mockResolvedValue(state({ backend: "wayland-portal", desktopSelection: "portal" }));
    vi.mocked(api.computerAuthorizeSurface).mockReturnValue(new Promise(() => {}));
    render(<ComputerPanel locale="en" sessionId="chat" runId={null} />);
    const consent = await screen.findByRole("button", { name: "Choose screen in system dialog" });
    expect(consent).toBeEnabled();
    expect(screen.getByText(/Input permission applies to the desktop session, not just one app\./)).toBeInTheDocument();
    expect(screen.getByText(/Wayland preview requires the GNOME policy helper\./)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Select target" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Refresh targets" })).toBeNull();
    expect(screen.queryByText("No available targets. Open a window or share a tab, then refresh.")).toBeNull();
    expect(api.computerListTargets).not.toHaveBeenCalled();
    expect(api.computerAuthorizeSurface).not.toHaveBeenCalled();
    expect(api.computerObserve).not.toHaveBeenCalled();
    fireEvent.click(consent);
    await waitFor(() => expect(api.computerAuthorizeSurface).toHaveBeenCalledWith(expect.objectContaining({ sessionId: "chat", surface: "desktop", targetId: "" })));
    expect(screen.getByRole("button", { name: /^Stop$/ })).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: /^Stop$/ }));
    await waitFor(() => expect(api.computerStop).toHaveBeenCalledWith({ sessionId: "chat", runId: null }));
  });

  it("system cancellation shows an error and permits a new explicit attempt, never auto-reopens", async () => {
    vi.mocked(api.computerStatus).mockResolvedValue(state({ backend: "wayland-portal", desktopSelection: "portal" }));
    vi.mocked(api.computerAuthorizeSurface).mockRejectedValue(new Error("native Wayland selection cancelled"));
    render(<ComputerPanel locale="en" sessionId="chat" runId={null} />);
    fireEvent.click(await screen.findByRole("button", { name: "Choose screen in system dialog" }));
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Choose screen in system dialog" })).toBeEnabled();
    expect(api.computerAuthorizeSurface).toHaveBeenCalledTimes(1);
    expect(api.computerObserve).not.toHaveBeenCalled();
  });

  it("retires stale target choices on refresh failure and recovers without stale errors", async () => {
    render(<ComputerPanel locale="en" sessionId="chat" runId={null} />);
    fireEvent.click(await screen.findByRole("button", { name: "Select target" }));
    fireEvent.click(await screen.findByRole("button", { name: /^Editor$/ }));
    expect(screen.getByRole("button", { name: "Authorize this target" })).toBeEnabled();
    vi.mocked(api.computerListTargets).mockRejectedValueOnce(new Error("target discovery unavailable"));
    fireEvent.click(screen.getByRole("button", { name: "Refresh targets" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not load targets. Refresh to try again.");
    await waitFor(() => expect(screen.getByRole("button", { name: "Refresh targets" })).toBeEnabled());
    expect(screen.getByRole("button", { name: "Authorize this target" })).toBeDisabled();
    expect(screen.queryByText("Selecting a target does not grant access.")).toBeNull();
    expect(screen.queryByText("No available targets. Open a window or share a tab, then refresh.")).toBeNull();
    expect(api.computerAuthorizeSurface).not.toHaveBeenCalled();
    vi.mocked(api.computerListTargets).mockResolvedValueOnce([
      { targetId: "new-window", title: "New editor", appName: "Editor", kind: "window" },
    ]);
    fireEvent.click(screen.getByRole("button", { name: "Refresh targets" }));
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
    expect(screen.getByRole("button", { name: "Authorize this target" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Select target" }));
    expect(screen.queryByRole("button", { name: /^Editor$/ })).toBeNull();
    fireEvent.click(await screen.findByRole("button", { name: "New editor" }));
    expect(screen.getByRole("button", { name: "Authorize this target" })).toBeEnabled();
    expect(api.computerAuthorizeSurface).not.toHaveBeenCalled();
    expect(api.computerObserve).not.toHaveBeenCalled();
  });

  it.each(["desktop", "managed-browser", "existing-tabs", "app-webview"] as const)(
    "%s discovery failure cannot masquerade as an empty list or disable current-run Stop", async (surface) => {
      vi.mocked(api.computerStatus).mockResolvedValue(running());
      vi.mocked(api.computerListTargets).mockRejectedValue(new Error("discovery unavailable"));
      vi.mocked(system.sideBrowserList).mockRejectedValue(new Error("discovery unavailable"));
      render(<ComputerPanel locale="en" sessionId="chat" runId="run" surface={surface} />);
      fireEvent.click(await screen.findByRole("button", { name: "Choose a target" }));
      expect(await screen.findByRole("alert")).toHaveTextContent("Could not load targets. Refresh to try again.");
      expect(screen.queryByText("Selecting a target does not grant access.")).toBeNull();
      expect(screen.queryByText("No available targets. Open a window or share a tab, then refresh.")).toBeNull();
      expect(screen.getByRole("button", { name: "Authorize this target" })).toBeDisabled();
      expect(screen.getByRole("button", { name: "Refresh targets" })).toBeEnabled();
      expect(screen.getByRole("button", { name: /^Stop$/ })).toBeEnabled();
      expect(screen.getByRole("button", { name: /^Pause$/ })).toBeEnabled();
      fireEvent.click(screen.getByRole("button", { name: /^Stop$/ }));
      await waitFor(() => expect(api.computerStop).toHaveBeenCalledWith({ sessionId: "chat", runId: "run" }));
      expect(api.computerAuthorizeSurface).not.toHaveBeenCalled();
    },
  );

  it("a successful target refresh does not erase an unrelated authorization failure", async () => {
    vi.mocked(api.computerAuthorizeSurface).mockRejectedValueOnce(new Error("yolo never grants desktop control"));
    render(<ComputerPanel locale="en" sessionId="chat" runId={null} />);
    fireEvent.click(await screen.findByRole("button", { name: "Select target" }));
    fireEvent.click(await screen.findByRole("button", { name: /^Editor$/ }));
    fireEvent.click(screen.getByRole("button", { name: "Authorize this target" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("YOLO does not grant desktop control.");
    fireEvent.click(screen.getByRole("button", { name: "Refresh targets" }));
    await waitFor(() => expect(api.computerListTargets).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(screen.getByRole("button", { name: "Refresh targets" })).toBeEnabled());
    expect(screen.getByRole("alert")).toHaveTextContent("YOLO does not grant desktop control.");
    expect(api.computerAuthorizeSurface).toHaveBeenCalledTimes(1);
  });

  it("shows no-session, off, and backend-unavailable states", async () => {
    render(<ComputerPanel locale="en" sessionId={null} runId={null} />);
    expect(
      screen.getByText("Open a local chat to use Computer Use."),
    ).toBeInTheDocument();
    cleanup();
    vi.mocked(api.computerStatus).mockResolvedValue(state({ featureEnabled: false }));
    render(<ComputerPanel locale="en" sessionId="chat" runId={null} />);
    expect(await screen.findByText("Computer Use is off")).toBeInTheDocument();
    expect(
      screen.getByText("Turn it on in Settings → Extensions → Computer Use."),
    ).toBeInTheDocument();
    cleanup();
    vi.mocked(api.computerStatus).mockResolvedValue(
      state({ featureEnabled: true, backend: "none" }),
    );
    render(<ComputerPanel locale="en" sessionId="chat" runId={null} />);
    expect(
      await screen.findByText("This desktop backend is not available here."),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Surface" })).toBeEnabled();
    expect(screen.getByRole("link", { name: "Settings" })).toBeInTheDocument();
  });

  it.each(["managed-browser", "existing-tabs", "app-webview"] as const)("does not hide %s setup behind desktop-only availability", async (surface) => {
    vi.mocked(api.computerStatus).mockResolvedValue(state({ backend: "none" }));
    render(<ComputerPanel locale="en" sessionId="chat" runId={null} surface={surface} />);
    expect(await screen.findByRole("button", { name: "Surface" })).toBeEnabled();
    expect(screen.queryByText("This desktop backend is not available here.")).toBeNull();
    expect(screen.getByRole("button", { name: "Authorize this target" })).toBeDisabled();
    expect(api.computerAuthorizeSurface).not.toHaveBeenCalled();
    expect(api.computerObserve).not.toHaveBeenCalled();
  });

  it("T24 Pause and Stop drive Host pause/stop including empty and busy paths", async () => {
    render(<ComputerPanel locale="en" sessionId="chat" runId={null} />);
    expect(await screen.findByRole("button", { name: /^Pause$/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: /^Stop$/ })).toBeDisabled();
    cleanup();
    vi.mocked(api.computerStatus).mockResolvedValue(running());
    render(<ComputerPanel locale="en" sessionId="chat" runId="run" />);
    fireEvent.click(await screen.findByRole("button", { name: /^Pause$/ }));
    await waitFor(() => expect(api.computerPause).toHaveBeenCalledWith({ sessionId: "chat", runId: "run" }));
    fireEvent.click(screen.getByRole("button", { name: /^Stop$/ }));
    await waitFor(() => expect(api.computerStop).toHaveBeenCalledWith({ sessionId: "chat", runId: "run" }));
    vi.mocked(api.computerPause).mockRejectedValueOnce(new Error("busy"));
    fireEvent.click(screen.getByRole("button", { name: /^Pause$/ }));
    await waitFor(() => expect(api.computerPause).toHaveBeenCalled());
  });

  it("shows truncated observation copy from the Host frame", async () => {
    vi.mocked(api.computerStatus).mockResolvedValue(running());
    vi.mocked(api.computerObserve).mockResolvedValue({
      snapshotId: "frame",
      geometryRevision: 1,
      previewDataUrl: "data:image/png;base64,AAAA",
      truncated: true,
    });
    render(<ComputerPanel locale="en" sessionId="chat" runId="run" />);
    expect(await screen.findByText("This observation was truncated.")).toBeInTheDocument();
  });

  it("maps Host YOLO refusal onto the dedicated panel copy", async () => {
    vi.mocked(api.computerAuthorizeSurface).mockRejectedValueOnce(
      new Error("yolo never grants desktop control"),
    );
    render(<ComputerPanel locale="en" sessionId="chat" runId={null} />);
    fireEvent.click(await screen.findByRole("button", { name: "Select target" }));
    fireEvent.click(await screen.findByRole("button", { name: /^Editor$/ }));
    fireEvent.click(screen.getByRole("button", { name: "Authorize this target" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "YOLO does not grant desktop control.",
    );
  });

  it("lists targets before per-session authorization and does not capture them", async () => {
    render(<ComputerPanel locale="en" sessionId="chat" runId={null} />);
    await waitFor(() => expect(api.computerListTargets).toHaveBeenCalled());
    expect(api.computerListTargets).toHaveBeenCalledWith({
      sessionId: "chat",
      runId: null,
      surface: "desktop",
    });
    expect(api.computerObserve).not.toHaveBeenCalled();
    expect(api.computerAuthorizeSurface).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Select target" })).toBeEnabled();
    expect(screen.getByRole("button", { name: /^Pause$/ })).toBeDisabled();
  });

  it("browser surface lists managed-browser targets instead of desktop-only listing", async () => {
    vi.mocked(api.computerListTargets).mockResolvedValue([
      { targetId: "managed-profile:p1", title: "p1", appName: "managed-browser", kind: "managed-profile" },
    ]);
    render(
      <ComputerPanel locale="en" sessionId="chat" runId={null} surface="managed-browser" />,
    );
    await waitFor(() =>
      expect(api.computerListTargets).toHaveBeenCalledWith({
        sessionId: "chat",
        runId: null,
        surface: "managed-browser",
      }),
    );
    expect(api.computerListTargets).not.toHaveBeenCalledWith(
      expect.objectContaining({ surface: "desktop" }),
    );
    expect(screen.getByText("Managed Browser")).toBeInTheDocument();
  });

  it("candidate selection neither authorizes nor captures until explicit consent", async () => {
    render(<ComputerPanel locale="en" sessionId="chat" runId={null} />);
    await waitFor(() => expect(screen.getByRole("button", { name: "Select target" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "Select target" }));
    fireEvent.click(await screen.findByRole("button", { name: /^Editor$/ }));
    expect(api.computerAuthorizeSurface).not.toHaveBeenCalled();
    expect(api.computerObserve).not.toHaveBeenCalled();
    const authorize = screen.getByRole("button", { name: "Authorize this target" });
    expect(authorize).toBeEnabled();
    fireEvent.click(authorize);
    fireEvent.click(authorize);
    await waitFor(() => expect(api.computerAuthorizeSurface).toHaveBeenCalledTimes(1));
  });

  it("preserves the exact Stop target when status becomes unknown", async () => {
    vi.useFakeTimers();
    vi.mocked(api.computerStatus).mockResolvedValueOnce(running()).mockRejectedValue(new Error("offline"));
    render(<ComputerPanel locale="en" sessionId="chat" runId="run" />);
    await act(async () => {});
    expect(screen.getByRole("status")).toHaveTextContent("Control authorized");
    await act(async () => { await vi.advanceTimersByTimeAsync(900); });
    expect(screen.getByRole("status")).toHaveTextContent("Status unknown");
    expect(screen.getByRole("button", { name: "Choose a target" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Pause" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Stop" })).toBeEnabled();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Stop" })); });
    expect(api.computerStop).toHaveBeenCalledWith({ sessionId: "chat", runId: "run" });
  });

  it.each(["known-run", null])("keeps the exact session stoppable after an initial status failure (run %s)", async (runId) => {
    vi.mocked(api.computerStatus).mockRejectedValue(new Error("offline"));
    render(<ComputerPanel locale="en" sessionId="chat" runId={runId} />);
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("Status unknown"));
    const stop = screen.getByRole("button", { name: "Stop" });
    expect(stop).toBeEnabled();
    fireEvent.click(stop);
    await waitFor(() => expect(api.computerStop).toHaveBeenCalledWith({ sessionId: "chat", runId }));
    expect(api.computerAuthorizeSurface).not.toHaveBeenCalled();
    expect(api.computerObserve).not.toHaveBeenCalled();
  });

  it("does not lock Stop behind a hung post-stop status read", async () => {
    vi.useFakeTimers();
    vi.mocked(api.computerStatus).mockResolvedValueOnce(running()).mockImplementation(() => new Promise(() => {}));
    vi.mocked(api.computerStop).mockResolvedValue();
    render(<ComputerPanel locale="en" sessionId="chat" runId="run" />);
    await act(async () => {});
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Stop" })); });
    await act(async () => { await vi.advanceTimersByTimeAsync(3100); });
    expect(screen.getByRole("status")).toHaveTextContent("Status unknown");
    expect(screen.getByRole("button", { name: "Pause" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Stop" })).toBeEnabled();
    expect(api.computerStop).toHaveBeenCalledTimes(1);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Stop" })); });
    expect(api.computerStop).toHaveBeenCalledTimes(2);
    expect(api.computerStop).toHaveBeenLastCalledWith({ sessionId: "chat", runId: "run" });
  });

  it("marks a hung status request unknown without queuing duplicate Host requests", async () => {
    vi.useFakeTimers();
    vi.mocked(api.computerStatus).mockResolvedValueOnce(running()).mockImplementation(() => new Promise(() => {}));
    render(<ComputerPanel locale="en" sessionId="chat" runId="run" />);
    await act(async () => {});
    await act(async () => { await vi.advanceTimersByTimeAsync(3900); });
    expect(screen.getByRole("status")).toHaveTextContent("Status unknown");
    expect(screen.getByRole("button", { name: "Stop" })).toBeEnabled();
    expect(api.computerStatus).toHaveBeenCalledTimes(2);
    await act(async () => { await vi.advanceTimersByTimeAsync(10000); });
    expect(api.computerStatus).toHaveBeenCalledTimes(2);
  });

  it("does not hide Stop when feature disablement still has a running Host", async () => {
    vi.mocked(api.computerStatus).mockResolvedValue({ ...running(), featureEnabled: false });
    render(<ComputerPanel locale="en" sessionId="chat" runId="run" />);
    expect(await screen.findByRole("button", { name: "Pause" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Stop" })).toBeEnabled();
    expect(screen.getByRole("status")).toHaveTextContent("This target is not authorized.");
  });

  it("allows Stop during authorization and ignores its late result", async () => {
    let finish!: (run: string) => void;
    vi.mocked(api.computerAuthorizeSurface).mockImplementation((opts) => new Promise((resolve) => {
      finish = (run) => resolve({
        attemptId: opts.attemptId,
        selectorRevision: opts.selectorRevision,
        runId: run,
        target: { targetId: "window", title: "Editor", appName: "Editor", kind: "window", surface: "desktop" },
      });
    }));
    render(<ComputerPanel locale="en" sessionId="chat" runId={null} />);
    fireEvent.click(await screen.findByRole("button", { name: "Select target" }));
    fireEvent.click(await screen.findByRole("button", { name: /^Editor$/ }));
    fireEvent.click(screen.getByRole("button", { name: "Authorize this target" }));
    await waitFor(() => expect(api.computerAuthorizeSurface).toHaveBeenCalled());
    fireEvent.click(screen.getByRole("button", { name: /^Stop$/ }));
    await waitFor(() => expect(api.computerStop).toHaveBeenCalled());
    await waitFor(() => expect(api.computerCancelAuthorization).toHaveBeenCalledWith({
      sessionId: "chat",
      attemptId: expect.any(String),
      selectorRevision: 1,
    }));
    const before = vi.mocked(api.computerStatus).mock.calls.length;
    await act(async () => { finish("revoked-run"); });
    expect(api.computerStatus).toHaveBeenCalledTimes(before);
    expect(api.computerObserve).not.toHaveBeenCalled();
  });

  it.each([
    { fromSession: "a", fromRun: null, toSession: "b", toRun: null },
    { fromSession: "a:b", fromRun: "run", toSession: "a", toRun: "b:run" },
  ])("cancels the exact Host authorization when switching $fromSession/$fromRun to $toSession/$toRun", async ({ fromSession, fromRun, toSession, toRun }) => {
    let finish!: (run: string) => void;
    vi.mocked(api.computerAuthorizeSurface).mockImplementation((opts) => new Promise((resolve) => {
      finish = (run) => resolve({
        attemptId: opts.attemptId,
        selectorRevision: opts.selectorRevision,
        runId: run,
        target: { targetId: "window", title: "Editor", appName: "Editor", kind: "window", surface: "desktop" },
      });
    }));
    const view = render(<ComputerPanel locale="en" sessionId={fromSession} runId={fromRun} />);
    fireEvent.click(await screen.findByRole("button", { name: "Select target" }));
    fireEvent.click(await screen.findByRole("button", { name: /^Editor$/ }));
    fireEvent.click(screen.getByRole("button", { name: "Authorize this target" }));
    await waitFor(() => expect(api.computerAuthorizeSurface).toHaveBeenCalled());

    view.rerender(<ComputerPanel locale="en" sessionId={toSession} runId={toRun} />);
    await waitFor(() => expect(api.computerCancelAuthorization).toHaveBeenCalledWith({
      sessionId: fromSession,
      attemptId: expect.any(String),
      selectorRevision: 1,
    }));
    await act(async () => { finish("late-run"); });
    expect(api.computerStatus).not.toHaveBeenCalledWith({ sessionId: fromSession, runId: "late-run" });
    expect(screen.getByRole("button", { name: "Authorize this target" })).toBeDisabled();
    expect(api.computerObserve).not.toHaveBeenCalled();
  });

  it("keeps the requested run bound to the exact tuple after a colliding chat switch", async () => {
    vi.mocked(api.computerStatus).mockImplementation(async ({ runId }) => ({ ...running(), runId }));
    const view = render(<ComputerPanel locale="en" sessionId="a:b" runId="run" />);
    await waitFor(() => expect(api.computerObserve).toHaveBeenCalledWith({ sessionId: "a:b", runId: "run" }));
    view.rerender(<ComputerPanel locale="en" sessionId="a" runId="b:run" />);
    await waitFor(() => expect(api.computerStatus).toHaveBeenCalledWith({ sessionId: "a", runId: "b:run" }));
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    await waitFor(() => expect(api.computerStop).toHaveBeenLastCalledWith({ sessionId: "a", runId: "b:run" }));
    expect(api.computerObserve).not.toHaveBeenCalledWith({ sessionId: "a", runId: "run" });
  });

  it("retains real stopping state and cannot resume until Host confirms", async () => {
    vi.mocked(api.computerStatus).mockResolvedValue(state({ ...running(), stopState: "stop_requested" }));
    render(<ComputerPanel locale="en" sessionId="chat" runId={null} />);
    expect(await screen.findByRole("button", { name: "Stopping…" })).toBeDisabled();
    expect(screen.getByRole("button", { name: /^Pause$/ })).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Choose a target" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Authorize this target" })).toBeNull();
    expect(api.computerObserve).not.toHaveBeenCalled();
  });

  it("does not infer a native Stop solely from pending tool cleanup", async () => {
    vi.mocked(api.computerStatus).mockResolvedValue(state({ ...running(), mcpCatalog: {
      desiredGeneration: 4, appliedGeneration: 3, desiredPresent: false,
      pending: true, cleanupPending: true, lastError: null,
    } }));
    render(<ComputerPanel locale="en" sessionId="chat" runId="run" />);
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("Removing Computer Use tools…"));
    expect(screen.queryByText("Local control is stopped. Tool cleanup is still pending.")).toBeNull();
    expect(screen.getByRole("button", { name: "Stop" })).toBeEnabled();
    expect(api.computerObserve).not.toHaveBeenCalled();
  });

  it("captures only while preview is visible and keeps the screenshot read-only", async () => {
    vi.mocked(api.computerStatus).mockResolvedValue(running());
    render(<ComputerPanel locale="en" sessionId="chat" runId={null} />);
    await waitFor(() => expect(api.computerObserve).toHaveBeenCalledTimes(1));
    const frame = document.querySelector(".cu-panel__frame");
    expect(frame?.tagName).toBe("DIV");
    vi.useFakeTimers();
    fireEvent.click(screen.getByRole("button", { name: "Hide preview" }));
    await act(async () => { await vi.advanceTimersByTimeAsync(3100); });
    expect(api.computerObserve).toHaveBeenCalledTimes(1);
    expect(api.computerSetPreview).toHaveBeenLastCalledWith({ sessionId: "chat", runId: "run", visible: false });
    fireEvent.click(screen.getByRole("button", { name: "Show preview" }));
    await act(async () => { await vi.advanceTimersByTimeAsync(0); });
    expect(api.computerObserve).toHaveBeenCalledTimes(2);
  });

  it("discards a late frame when switching chats", async () => {
    let finish!: (frame: api.ComputerObservation) => void;
    vi.mocked(api.computerStatus).mockImplementation(async ({ sessionId }) => sessionId === "a" ? running() : state());
    vi.mocked(api.computerObserve).mockImplementation(() => new Promise((resolve) => { finish = resolve; }));
    const view = render(<ComputerPanel locale="en" sessionId="a" runId={null} />);
    await waitFor(() => expect(api.computerObserve).toHaveBeenCalled());
    view.rerender(<ComputerPanel locale="en" sessionId="b" runId={null} />);
    await act(async () => { finish({ snapshotId: "old", geometryRevision: 1, previewDataUrl: "data:image/png;base64,PRIVATE" }); });
    expect(document.querySelector(".cu-panel__frame img")).toBeNull();
  });

  it("locks target selection and keeps the last preview while authorization is in flight", async () => {
    vi.mocked(api.computerStatus).mockResolvedValue(running());
    vi.mocked(api.computerListTargets).mockResolvedValue([
      { targetId: "window", title: "Editor", appName: "Editor", kind: "window" },
      { targetId: "tab", title: "Docs", appName: "Browser", kind: "tab" },
    ]);
    let finish!: (run: string) => void;
    vi.mocked(api.computerAuthorizeSurface).mockImplementation(
      (opts) => new Promise((resolve) => {
        finish = (run) => resolve({
          attemptId: opts.attemptId,
          selectorRevision: opts.selectorRevision,
          runId: run,
          target: { targetId: "tab", title: "Docs", appName: "Browser", kind: "tab", surface: "desktop" },
        });
      }),
    );
    render(<ComputerPanel locale="en" sessionId="chat" runId="run" />);
    expect(await screen.findByRole("img", { name: "Preview of the authorized target" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Choose a target" }));
    const picker = screen.getByRole("button", { name: "Select target" });
    fireEvent.click(picker);
    fireEvent.click(await screen.findByRole("button", { name: /^Docs$/ }));
    fireEvent.click(screen.getByRole("button", { name: "Authorize this target" }));
    await waitFor(() => expect(api.computerAuthorizeSurface).toHaveBeenCalled());
    expect(screen.getByRole("button", { name: "Select target" })).toBeDisabled();
    expect(screen.getByRole("status")).toHaveTextContent("Authorizing target…");
    expect(screen.getByRole("img", { name: "Preview of the authorized target" })).toBeInTheDocument();
    expect(screen.getByRole("toolbar", { name: "Computer controls" })).toBeInTheDocument();
    expect(screen.getByText("Technical details").closest("details")).not.toHaveAttribute("open");
    await act(async () => { finish("run-2"); });
  });

  it("retries desired-absent cleanup from the keyboard and locks conflicting controls", async () => {
    const pending = state({
      runId: "stopped-run",
      targetId: "window",
      targetName: "Editor",
      stopState: "stopped",
      mcpCatalog: {
        desiredGeneration: 4,
        appliedGeneration: 3,
        desiredPresent: false,
        pending: true,
        cleanupPending: true,
        lastError: "Computer Use MCP catalog update timed out",
      },
    });
    const settled = state({
      ...pending,
      mcpCatalog: {
        desiredGeneration: 4,
        appliedGeneration: 4,
        desiredPresent: false,
        pending: false,
        cleanupPending: false,
        lastError: null,
      },
    });
    let current = pending;
    vi.mocked(api.computerStatus).mockImplementation(async () => current);
    vi.mocked(api.computerRetryCleanup).mockImplementation(async () => {
      current = settled;
      return settled.mcpCatalog!;
    });
    const user = userEvent.setup();
    render(<ComputerPanel locale="en" sessionId="chat" runId="stopped-run" />);

    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent(
      "Local control is stopped. Tool cleanup is still pending.",
    ));
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Latest cleanup error: Computer Use MCP catalog update timed out",
    );
    expect(screen.queryByRole("button", { name: "Select target" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Refresh targets" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Authorize this target" })).toBeNull();
    const retry = screen.getByRole("button", { name: "Retry cleanup" });
    retry.focus();
    await user.keyboard("{Enter}");

    await waitFor(() => expect(api.computerRetryCleanup).toHaveBeenCalledTimes(1));
    expect(api.computerRetryCleanup).toHaveBeenCalledWith({ sessionId: "chat" });
    await waitFor(() => expect(screen.queryByRole("button", { name: "Retry cleanup" })).toBeNull());
    expect(screen.getByRole("button", { name: "Select target" })).toBeEnabled();
    expect(api.computerAuthorizeSurface).not.toHaveBeenCalled();
  });

  it("deduplicates cleanup retry clicks and ignores its late response after a chat switch", async () => {
    let finish!: (status: api.ComputerMcpCatalogStatus) => void;
    const pending = state({
      runId: "stopped-run",
      targetId: "window",
      targetName: "Editor",
      stopState: "stopped",
      mcpCatalog: {
        desiredGeneration: 2,
        appliedGeneration: 1,
        desiredPresent: false,
        pending: true,
        cleanupPending: true,
        lastError: "Computer Use MCP catalog update failed",
      },
    });
    vi.mocked(api.computerStatus).mockImplementation(async ({ sessionId }) =>
      sessionId === "a" ? pending : state(),
    );
    vi.mocked(api.computerRetryCleanup).mockImplementation(
      () => new Promise((resolve) => { finish = resolve; }),
    );
    const view = render(<ComputerPanel locale="en" sessionId="a" runId="stopped-run" />);
    const retry = await screen.findByRole("button", { name: "Retry cleanup" });
    fireEvent.click(retry);
    fireEvent.click(retry);
    expect(api.computerRetryCleanup).toHaveBeenCalledTimes(1);
    expect(await screen.findByRole("button", { name: "Retrying cleanup…" })).toBeDisabled();

    const oldStatusCalls = vi.mocked(api.computerStatus).mock.calls.filter(
      ([opts]) => opts.sessionId === "a",
    ).length;
    view.rerender(<ComputerPanel locale="en" sessionId="b" runId={null} />);
    await act(async () => {
      finish({
        desiredGeneration: 2,
        appliedGeneration: 2,
        desiredPresent: false,
        pending: false,
        cleanupPending: false,
        lastError: null,
      });
    });
    expect(screen.queryByText(/Latest cleanup error/)).toBeNull();
    expect(vi.mocked(api.computerStatus).mock.calls.filter(
      ([opts]) => opts.sessionId === "a",
    )).toHaveLength(oldStatusCalls);
  });

  it("keeps cleanup retry available and shows the latest redacted failure", async () => {
    const pending = state({
      runId: "stopped-run",
      stopState: "stopped",
      mcpCatalog: {
        desiredGeneration: 3,
        appliedGeneration: 2,
        desiredPresent: false,
        pending: true,
        cleanupPending: true,
        lastError: "Computer Use MCP catalog update failed",
      },
    });
    vi.mocked(api.computerStatus).mockResolvedValue(pending);
    vi.mocked(api.computerRetryCleanup).mockRejectedValue(
      new Error("Computer Use MCP catalog update timed out"),
    );
    render(<ComputerPanel locale="en" sessionId="chat" runId="stopped-run" />);

    fireEvent.click(await screen.findByRole("button", { name: "Retry cleanup" }));
    await waitFor(() => expect(api.computerRetryCleanup).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent(
      "Latest cleanup error: Computer Use MCP catalog update timed out",
    ));
    const retry = screen.getByRole("button", { name: "Retry cleanup" });
    expect(retry).toBeEnabled();
    fireEvent.click(retry);
    await waitFor(() => expect(api.computerRetryCleanup).toHaveBeenCalledTimes(2));
    expect(api.computerAuthorizeSurface).not.toHaveBeenCalled();
  });

  it("ignores cleanup retry completion after an explicit unmount", async () => {
    let finish!: (status: api.ComputerMcpCatalogStatus) => void;
    const pending = state({
      runId: "stopped-run",
      stopState: "stopped",
      mcpCatalog: {
        desiredGeneration: 5,
        appliedGeneration: 4,
        desiredPresent: false,
        pending: true,
        cleanupPending: true,
        lastError: "Computer Use MCP catalog update failed",
      },
    });
    vi.mocked(api.computerStatus).mockResolvedValue(pending);
    vi.mocked(api.computerRetryCleanup).mockImplementation(
      () => new Promise((resolve) => { finish = resolve; }),
    );
    const view = render(
      <ComputerPanel locale="en" sessionId="chat" runId="stopped-run" />,
    );
    fireEvent.click(await screen.findByRole("button", { name: "Retry cleanup" }));
    await waitFor(() => expect(api.computerRetryCleanup).toHaveBeenCalledTimes(1));
    const statusCallsBeforeUnmount = vi.mocked(api.computerStatus).mock.calls.length;
    view.unmount();

    await act(async () => {
      finish({
        desiredGeneration: 5,
        appliedGeneration: 5,
        desiredPresent: false,
        pending: false,
        cleanupPending: false,
        lastError: null,
      });
    });
    expect(api.computerStatus).toHaveBeenCalledTimes(statusCallsBeforeUnmount);
    expect(api.computerAuthorizeSurface).not.toHaveBeenCalled();
  });

  it("existing-tabs requires a separate App confirmation before displaying the code", async () => {
    vi.mocked(api.computerStatus).mockResolvedValue(running());
    render(<ComputerPanel locale="en" sessionId="chat" runId="run" surface="existing-tabs" />);
    fireEvent.click(await screen.findByRole("button", { name: /^Pair browser extension$/ }));
    await waitFor(() => expect(api.computerBeginPairing).toHaveBeenCalled());
    expect(api.computerConfirmPairingApp).not.toHaveBeenCalled();
    expect(screen.queryByTestId("cu-pairing-code")).not.toBeInTheDocument();
    fireEvent.click(await screen.findByRole("button", { name: /^Confirm pairing in App$/ }));
    await waitFor(() => expect(api.computerConfirmPairingApp).toHaveBeenCalledWith("n1"));
    const code = await screen.findByRole("textbox", { name: "Pairing code" });
    expect(code).toHaveValue("ABCDE-ABCDE-ABCDE-ABCDE");
    expect(code).toHaveAttribute("readonly");
    fireEvent.click(screen.getByRole("button", { name: /^Revoke pairing$/ }));
    await waitFor(() => expect(api.computerRevokePairing).toHaveBeenCalled());
    await waitFor(() => expect(screen.queryByTestId("cu-pairing-code")).not.toBeInTheDocument());
  });

  it("managed-browser selection uses one Host authorization transaction", async () => {
    vi.mocked(api.computerListTargets).mockResolvedValue([
      {
        targetId: "managed-profile:p1",
        title: "p1",
        appName: "managed-browser",
        kind: "managed-profile",
        surface: "managed-browser",
      },
    ]);
    render(
      <ComputerPanel locale="en" sessionId="chat" runId={null} surface="managed-browser" />,
    );
    fireEvent.click(await screen.findByRole("button", { name: "Select target" }));
    fireEvent.click(await screen.findByRole("button", { name: /^p1$/ }));
    fireEvent.click(screen.getByRole("button", { name: "Authorize this target" }));
    await waitFor(() =>
      expect(api.computerAuthorizeSurface).toHaveBeenCalledWith({
        sessionId: "chat",
        runId: null,
        targetId: "managed-profile:p1",
        surface: "managed-browser",
        attemptId: expect.any(String),
        selectorRevision: 1,
      }),
    );
  });

  it("existing-tabs selection uses one Host authorization transaction", async () => {
    vi.mocked(api.computerListTargets).mockResolvedValue([
      {
        targetId: "existing-tab:tab-1",
        title: "Shared",
        appName: "existing-tabs",
        kind: "existing-tab",
        surface: "existing-tabs",
      },
    ]);
    render(
      <ComputerPanel locale="en" sessionId="chat" runId={null} surface="existing-tabs" />,
    );
    fireEvent.click(await screen.findByRole("button", { name: "Select target" }));
    fireEvent.click(await screen.findByRole("button", { name: /^Shared$/ }));
    fireEvent.click(screen.getByRole("button", { name: "Authorize this target" }));
    await waitFor(() =>
      expect(api.computerAuthorizeSurface).toHaveBeenCalledWith({
        sessionId: "chat",
        runId: null,
        targetId: "existing-tab:tab-1",
        surface: "existing-tabs",
        attemptId: expect.any(String),
        selectorRevision: 1,
      }),
    );
  });

  it("app-webview bind/unbind drives Host webview commands", async () => {
    vi.mocked(api.computerStatus).mockResolvedValue(running());
    render(<ComputerPanel locale="en" sessionId="chat" runId="run" surface="app-webview" />);
    fireEvent.click(await screen.findByRole("button", { name: "Choose a target" }));
    fireEvent.click(await screen.findByRole("button", { name: "Select App browser tab" }));
    fireEvent.click(await screen.findByRole("button", { name: "resource-browser-cu" }));
    fireEvent.click(screen.getByRole("button", { name: "Authorize this target" }));
    await waitFor(() =>
      expect(api.computerAuthorizeSurface).toHaveBeenCalledWith({
        sessionId: "chat",
        runId: "run",
        targetId: "resource-browser-cu",
        surface: "app-webview",
        attemptId: expect.any(String),
        selectorRevision: 1,
      }),
    );
    fireEvent.click(await screen.findByRole("button", { name: "Choose a target" }));
    fireEvent.click(screen.getByRole("button", { name: /^Unbind App browser$/ }));
    await waitFor(() => expect(api.computerUnbindWebview).toHaveBeenCalledWith({
      sessionId: "chat",
      runId: "authorized-run",
    }));
  });
});
