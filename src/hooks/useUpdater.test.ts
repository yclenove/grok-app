// @vitest-environment jsdom
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const host = vi.hoisted(() => ({
  check: vi.fn(),
  invoke: vi.fn(),
  relaunch: vi.fn(),
  download: vi.fn(),
  install: vi.fn(),
  close: vi.fn(),
  prepare: vi.fn(),
  pending: vi.fn(),
  resume: vi.fn(),
  sim: vi.fn(),
}));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: host.check }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: host.relaunch }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: host.invoke }));
vi.mock("@/lib/api", () => ({ isDesktopHost: () => true }));
vi.mock("@/lib/updateSim", () => ({
  UPDATE_SIM_CHANGE_EVENT: "update-sim-test",
  UPDATE_SIM_VERSION: "sim-version",
  clearUpdateSimIfDeveloperModeOff: vi.fn(),
  installDeveloperModeSimCleanup: vi.fn(),
  installUpdateSimConsoleApi: vi.fn(),
  readUpdateSimMode: host.sim,
  sleepMs: vi.fn(),
}));

import { useUpdater } from "./useUpdater";

beforeEach(() => {
  vi.resetAllMocks();
  host.pending.mockResolvedValue(null);
  host.sim.mockReturnValue("off");
  host.check.mockResolvedValue({
    version: "9.9.9-test",
    download: host.download,
    install: host.install,
    close: host.close,
  });
  host.invoke.mockImplementation(async (command: string, args?: unknown) => {
    if (command === "plugin:updater|pending_install") return host.pending(args);
    if (command === "plugin:updater|resume_install") return host.resume(args);
    if (command === "prepare_for_app_update") return host.prepare();
    if (command === "updater_status") {
      return { channel: "silent", pluginEnabled: true, platformSupported: true, endpoint: "" };
    }
    if (command === "is_auto_update_supported" || command === "is_updater_plugin_enabled") return true;
    throw new Error(`unexpected test command: ${command}`);
  });
});
afterEach(() => { cleanup(); vi.useRealTimers(); });

async function ready() {
  const view = renderHook(() => useUpdater());
  await waitFor(() => expect(view.result.current.status.state).toBe("ready"));
  return view;
}

describe("Windows native install preflight recovery", () => {
  it.each(["launch_unknown", "task"])("never replays an unconfirmed %s outcome even without a catalog snapshot", async phase => {
    host.install.mockRejectedValue({ code: "update_install_pending", phase, message: "unknown outcome" });
    const view = await ready();
    await act(() => view.result.current.installAndRelaunch());
    expect(view.result.current.status).toMatchObject({ state: "error", installPending: true, installBlocked: true });
    await act(async () => {
      await view.result.current.installAndRelaunch();
      expect(await view.result.current.applyAvailableUpdate()).toEqual({ kind: "busy" });
      await view.result.current.checkForUpdate();
    });
    expect(host.install).toHaveBeenCalledTimes(1);
    expect(host.check).toHaveBeenCalledTimes(1);
  });
  it.each(["cleanup", "launch"])(
    "retains the exact downloaded update after native %s refusal without claiming it is installed",
    async phase => {
      host.install.mockRejectedValue({ code: "update_install_pending", phase, message: `native ${phase} pending` });
      const view = await ready();
      await act(() => view.result.current.installAndRelaunch());
      expect(view.result.current.status).toEqual({ state: "error", message: `native ${phase} pending`, version: "9.9.9-test", installPending: true });
      expect(host.close).not.toHaveBeenCalled();
      expect(host.prepare).not.toHaveBeenCalled();
      expect(host.relaunch).not.toHaveBeenCalled();
      await act(async () => {
        await view.result.current.checkForUpdate();
        window.dispatchEvent(new Event("update-sim-test"));
      });
      expect(view.result.current.status.state).toBe("error");
      await act(() => view.result.current.applyAvailableUpdate());
      expect(host.check).toHaveBeenCalledTimes(1);
      expect(host.download).toHaveBeenCalledTimes(1);
      expect(host.install).toHaveBeenCalledTimes(2);
      expect(host.close).not.toHaveBeenCalled();
      expect(host.relaunch).not.toHaveBeenCalled();
    },
  );

  it("cannot race a second install request against the original native owner", async () => {
    let refuse!: (reason: unknown) => void;
    host.install.mockImplementationOnce(() => new Promise<void>((_resolve, reject) => { refuse = reject; }));
    const view = await ready();
    let first!: Promise<void>;
    act(() => { first = view.result.current.installAndRelaunch(); });
    await waitFor(() => expect(host.install).toHaveBeenCalledTimes(1));
    await act(async () => {
      expect(await view.result.current.applyAvailableUpdate()).toEqual({ kind: "busy" });
      await view.result.current.installAndRelaunch();
      await view.result.current.checkForUpdate();
      window.dispatchEvent(new Event("update-sim-test"));
    });
    expect(host.check).toHaveBeenCalledTimes(1);
    expect(host.install).toHaveBeenCalledTimes(1);
    await act(async () => {
      refuse({ code: "update_install_pending", phase: "cleanup", message: "original cleanup refused" });
      await first;
    });
    expect(view.result.current.status).toMatchObject({ installPending: true });
    expect(host.prepare).not.toHaveBeenCalled();
    expect(host.relaunch).not.toHaveBeenCalled();
  });

  it("does not infer native ownership from an ordinary error string", async () => {
    host.install.mockRejectedValueOnce(new Error("update_install_pending is just text"));
    const view = await ready();
    await act(() => view.result.current.installAndRelaunch());
    expect(view.result.current.status).toEqual({ state: "error", message: "update_install_pending is just text" });
  });

  it("retains pending identity even if a later retry has an ordinary transport error", async () => {
    host.install.mockRejectedValueOnce({ code: "update_install_pending", phase: "cleanup", message: "pending" });
    const view = await ready();
    await act(() => view.result.current.installAndRelaunch());
    host.install.mockRejectedValueOnce(new Error("transport unavailable"));
    await act(() => view.result.current.installAndRelaunch());
    expect(view.result.current.status).toEqual({ state: "error", message: "transport unavailable", version: "9.9.9-test", installPending: true });
    expect(host.check).toHaveBeenCalledTimes(1);
    expect(host.download).toHaveBeenCalledTimes(1);
    expect(host.install).toHaveBeenCalledTimes(2);
    expect(host.relaunch).not.toHaveBeenCalled();
  });
});

const retained = {
  candidateId: "opaque-original-candidate",
  version: "8.8.8-original",
  state: "retryable" as const,
  phase: "cleanup",
  message: "original cleanup refused",
};

describe("process-owned update recovery after UI reload", () => {
  it("releases the verified original completion without reinstall or relaunch and checks the next update", async () => {
    host.pending.mockResolvedValue({ ...retained, state: "blocked", phase: "installer_exited" });
    const view = renderHook(() => useUpdater());
    await waitFor(() => expect(view.result.current.status).toHaveProperty("installBlocked", true));
    host.pending.mockResolvedValue({ ...retained, state: "completed", phase: "completed", message: null });
    await waitFor(() => expect(view.result.current.status).toEqual({ state: "idle" }), { timeout: 2500 });
    expect(host.check).not.toHaveBeenCalled();
    expect(host.resume).not.toHaveBeenCalled();
    expect(host.prepare).not.toHaveBeenCalled();
    expect(host.relaunch).not.toHaveBeenCalled();
    await act(() => view.result.current.checkForUpdate());
    await waitFor(() => expect(view.result.current.status.state).toBe("ready"));
    expect(host.check).toHaveBeenCalledTimes(1);
    expect(host.install).not.toHaveBeenCalled();
  });

  it("a completed receipt on remount does not block normal discovery", async () => {
    host.pending.mockResolvedValue({ ...retained, state: "completed", phase: "completed", message: null });
    const view = renderHook(() => useUpdater());
    await waitFor(() => expect(view.result.current.status.state).toBe("ready"));
    expect(host.check).toHaveBeenCalledTimes(1);
    expect(host.resume).not.toHaveBeenCalled();
    expect(host.relaunch).not.toHaveBeenCalled();
  });

  it("queries the original completion by nonce when another window already owns the next update", async () => {
    host.pending.mockResolvedValue({ ...retained, state: "blocked", phase: "installer_exited" });
    const view = renderHook(() => useUpdater());
    await waitFor(() => expect(view.result.current.status).toHaveProperty("installBlocked", true));
    const next = { ...retained, candidateId: "next-candidate", version: "10.0.0", state: "retryable" };
    host.pending.mockImplementation(async (args?: { candidateId: string }) => args?.candidateId === retained.candidateId
      ? { ...retained, state: "completed", phase: "completed", message: null } : next);
    await waitFor(() => expect(view.result.current.status).toEqual({ state: "idle" }), { timeout: 2500 });
    expect(host.pending).toHaveBeenCalledWith({ candidateId: retained.candidateId });
    await act(() => view.result.current.checkForUpdate());
    expect(view.result.current.status).toMatchObject({ state: "error", version: next.version, installPending: true });
    expect(host.check).not.toHaveBeenCalled();
    expect(host.install).not.toHaveBeenCalled();
    expect(host.resume).not.toHaveBeenCalled();
  });

  it.each([{ candidateId: "wrong-owner" }, { version: "wrong-version" }])("does not release an original owner for another completed identity: %s", async replacement => {
    host.pending.mockResolvedValue({ ...retained, state: "blocked", phase: "installer_exited" });
    const view = renderHook(() => useUpdater());
    await waitFor(() => expect(view.result.current.status).toHaveProperty("installBlocked", true));
    host.pending.mockResolvedValue({ ...retained, ...replacement, state: "completed", phase: "completed", message: null });
    await waitFor(() => expect(view.result.current.status).toHaveProperty("message", "Original native update recovery owner is missing"), { timeout: 2500 });
    await act(() => view.result.current.checkForUpdate());
    expect(view.result.current.status).toMatchObject({ installBlocked: true, version: retained.version });
    expect(host.check).not.toHaveBeenCalled();
  });

  it("discovers original ownership before any network check or download", async () => {
    host.pending.mockResolvedValue(retained);
    const view = renderHook(() => useUpdater());
    await waitFor(() => expect(view.result.current.status).toMatchObject({ state: "error", version: retained.version, installPending: true }));
    await act(() => view.result.current.checkForUpdate());
    expect(host.check).not.toHaveBeenCalled();
    expect(host.download).not.toHaveBeenCalled();
    expect(host.install).not.toHaveBeenCalled();
    expect(host.resume).not.toHaveBeenCalled();
  });

  it("survives actual unmount/remount and resumes only the original nonce", async () => {
    host.install.mockImplementation(async () => {
      host.pending.mockResolvedValue(retained);
      throw { code: "update_install_pending", phase: "cleanup", message: "refused" };
    });
    const first = await ready();
    await act(() => first.result.current.installAndRelaunch());
    expect(first.result.current.status).toMatchObject({ version: retained.version, installPending: true });
    first.unmount();
    await waitFor(() => expect(host.close).toHaveBeenCalledTimes(1));
    const second = renderHook(() => useUpdater());
    await waitFor(() => expect(second.result.current.status).toMatchObject({ version: retained.version, installPending: true }));
    host.resume.mockRejectedValueOnce({ code: "update_install_pending", phase: "launch", message: "launch refused" });
    await act(() => second.result.current.installAndRelaunch());
    expect(host.resume).toHaveBeenCalledExactlyOnceWith({ candidateId: retained.candidateId });
    expect(host.check).toHaveBeenCalledTimes(1);
    expect(host.download).toHaveBeenCalledTimes(1);
    expect(host.install).toHaveBeenCalledTimes(1);
    expect(host.prepare).not.toHaveBeenCalled();
    expect(host.relaunch).not.toHaveBeenCalled();
    expect(second.result.current.status).not.toHaveProperty("restartPending");
  });

  it.each([new Error("recovery IPC unavailable"), undefined, { invalid: true }])("fails closed on discovery failure or invalid metadata: %s", async value => {
    if (value instanceof Error) host.pending.mockRejectedValue(value);
    else host.pending.mockResolvedValue(value);
    const view = renderHook(() => useUpdater());
    await waitFor(() => expect(view.result.current.status.state).toBe("error"));
    expect(host.check).not.toHaveBeenCalled();
    expect(host.download).not.toHaveBeenCalled();
    expect(host.invoke.mock.calls.some(([command]) => command === "app_check_update")).toBe(false);
    // Only a later authoritative empty snapshot permits ordinary discovery.
    host.pending.mockResolvedValue(null);
    await act(() => view.result.current.checkForUpdate());
    await waitFor(() => expect(view.result.current.status.state).toBe("ready"));
    expect(host.check).toHaveBeenCalledTimes(1);
  });

  it("polls a running owner without a duplicate mutation and recovers after its refusal", async () => {
    host.pending.mockResolvedValue({ ...retained, state: "running", message: null });
    const view = renderHook(() => useUpdater());
    await waitFor(() => expect(view.result.current.status.state).toBe("installing"));
    await act(async () => {
      await view.result.current.installAndRelaunch();
      expect(await view.result.current.applyAvailableUpdate()).toEqual({ kind: "busy" });
    });
    expect(host.resume).not.toHaveBeenCalled();
    host.pending.mockResolvedValue(retained);
    await waitFor(() => expect(view.result.current.status).toMatchObject({ state: "error", installPending: true }), { timeout: 2500 });
    expect(host.check).not.toHaveBeenCalled();
    expect(host.relaunch).not.toHaveBeenCalled();
  });

  it("requires verified plugin availability rather than silently falling back", async () => {
    const implementation = host.invoke.getMockImplementation()!;
    host.invoke.mockImplementation(async (command: string, args?: unknown) => {
      if (command === "is_updater_plugin_enabled") throw new Error("availability transport failed");
      return implementation(command, args);
    });
    const view = renderHook(() => useUpdater());
    await waitFor(() => expect(view.result.current.status).toMatchObject({ state: "error", message: "availability transport failed" }));
    expect(host.check).not.toHaveBeenCalled();
    expect(host.invoke.mock.calls.some(([command]) => command === "app_check_update")).toBe(false);
  });

  it.each(["silent", "manual"])("discovers native ownership even with persisted %s simulation", async mode => {
    host.sim.mockReturnValue(mode);
    host.pending.mockResolvedValue(retained);
    const view = renderHook(() => useUpdater());
    await waitFor(() => expect(view.result.current.status).toMatchObject({ version: retained.version, installPending: true }));
    expect(host.check).not.toHaveBeenCalled();
    expect(host.download).not.toHaveBeenCalled();
    expect(host.resume).not.toHaveBeenCalled();
  });

  it("releases UI recovery only for an explicit preparation-failed receipt", async () => {
    host.pending.mockResolvedValue({ ...retained, state: "running", phase: "staging" });
    const view = renderHook(() => useUpdater());
    await waitFor(() => expect(view.result.current.status.state).toBe("installing"));
    host.pending.mockResolvedValue({ ...retained, state: "failed", phase: "staging", message: "invalid format" });
    await waitFor(() => expect(view.result.current.status).toEqual({ state: "error", message: "invalid format" }), { timeout: 2500 });
    await act(() => view.result.current.checkForUpdate());
    await waitFor(() => expect(view.result.current.status.state).toBe("ready"));
    expect(host.check).toHaveBeenCalledTimes(1);
    expect(host.resume).not.toHaveBeenCalled();
    expect(host.install).not.toHaveBeenCalled();
  });

  it("poll errors disable mutation until the same original owner is verified again", async () => {
    host.pending.mockResolvedValue(retained);
    const view = renderHook(() => useUpdater());
    await waitFor(() => expect(view.result.current.status).toHaveProperty("installPending", true));
    host.pending.mockRejectedValue(new Error("snapshot temporarily unavailable"));
    await waitFor(() => expect(view.result.current.status).toHaveProperty("installBlocked", true), { timeout: 2500 });
    await act(() => view.result.current.installAndRelaunch());
    expect(host.resume).not.toHaveBeenCalled();
    host.pending.mockResolvedValue(retained);
    await waitFor(() => expect(view.result.current.status).not.toHaveProperty("installBlocked"), { timeout: 2500 });
    await act(() => view.result.current.installAndRelaunch());
    expect(host.resume).toHaveBeenCalledExactlyOnceWith({ candidateId: retained.candidateId });
    expect(host.check).not.toHaveBeenCalled();
  });

  it("quarantines unknown launch results in every public action", async () => {
    host.pending.mockResolvedValue({ ...retained, state: "blocked", phase: "launch_unknown" });
    const view = renderHook(() => useUpdater());
    await waitFor(() => expect(view.result.current.status).toMatchObject({ installBlocked: true }));
    await act(async () => {
      await view.result.current.installAndRelaunch();
      await view.result.current.checkForUpdate();
      expect(await view.result.current.applyAvailableUpdate()).toEqual({ kind: "busy" });
      window.dispatchEvent(new Event("update-sim-test"));
    });
    expect(host.resume).not.toHaveBeenCalled();
    expect(host.check).not.toHaveBeenCalled();
    expect(host.install).not.toHaveBeenCalled();
  });

  it("does not turn an observed installer exit zero into installed state or retry permission", async () => {
    host.pending.mockResolvedValue({ ...retained, state: "blocked", phase: "installer_running", message: "exact process still running" });
    const view = renderHook(() => useUpdater());
    await waitFor(() => expect(view.result.current.status).toMatchObject({ state: "error", installBlocked: true }));
    host.pending.mockResolvedValue({ ...retained, state: "blocked", phase: "installer_exited", message: "process exited with code 0; installed files still unverified" });
    await waitFor(() => expect(view.result.current.status).toMatchObject({
      state: "error", version: retained.version, installPending: true, installBlocked: true,
      message: "process exited with code 0; installed files still unverified",
    }), { timeout: 2500 });
    await act(async () => {
      await view.result.current.installAndRelaunch();
      await view.result.current.checkForUpdate();
      expect(await view.result.current.applyAvailableUpdate()).toEqual({ kind: "busy" });
    });
    expect(view.result.current.status).not.toHaveProperty("restartPending");
    for (const operation of [host.resume, host.check, host.download, host.install, host.prepare, host.relaunch]) {
      expect(operation).not.toHaveBeenCalled();
    }
  });

  it("does not infer installation success from a fulfilled resume IPC", async () => {
    host.pending.mockResolvedValue(retained);
    const view = renderHook(() => useUpdater());
    await waitFor(() => expect(view.result.current.status).toHaveProperty("installPending", true));
    host.resume.mockResolvedValue(undefined);
    await act(() => view.result.current.installAndRelaunch());
    expect(view.result.current.status).toMatchObject({ state: "error", installPending: true });
    expect(host.prepare).not.toHaveBeenCalled();
    expect(host.relaunch).not.toHaveBeenCalled();
  });

  it.each([null, { ...retained, candidateId: "replacement" }])("blocks a missing/replaced owner after resume", async outcome => {
    host.pending.mockResolvedValue(retained);
    const view = renderHook(() => useUpdater());
    await waitFor(() => expect(view.result.current.status).toHaveProperty("installPending", true));
    host.resume.mockImplementation(async () => { host.pending.mockResolvedValue(outcome); });
    await act(() => view.result.current.installAndRelaunch());
    expect(view.result.current.status).toMatchObject({ state: "error", installPending: true, installBlocked: true, version: retained.version });
    await act(() => view.result.current.installAndRelaunch());
    expect(host.resume).toHaveBeenCalledTimes(1);
    expect(host.check).not.toHaveBeenCalled();
  });

  it("prevents concurrent resumes and discards a poll taken before a new attempt", async () => {
    host.pending.mockResolvedValue(retained);
    const view = renderHook(() => useUpdater());
    await waitFor(() => expect(view.result.current.status).toHaveProperty("installPending", true));
    let stale!: (value: unknown) => void;
    host.pending.mockImplementationOnce(() => new Promise(resolve => { stale = resolve; }));
    await waitFor(() => expect(stale).toBeDefined(), { timeout: 2500 });
    let finish!: () => void;
    host.resume.mockImplementationOnce(() => new Promise<void>(resolve => { finish = resolve; }));
    let first!: Promise<void>;
    act(() => { first = view.result.current.installAndRelaunch(); });
    await waitFor(() => expect(host.resume).toHaveBeenCalledTimes(1));
    await act(async () => {
      stale(retained);
      await Promise.resolve();
      await view.result.current.installAndRelaunch();
      expect(await view.result.current.applyAvailableUpdate()).toEqual({ kind: "busy" });
    });
    expect(view.result.current.status.state).toBe("installing");
    expect(host.resume).toHaveBeenCalledTimes(1);
    await act(async () => { finish(); await first; });
    expect(view.result.current.status).toHaveProperty("installPending", true);
  });

  it("ignores discovery results after unmount", async () => {
    let result!: (value: unknown) => void;
    host.pending.mockImplementationOnce(() => new Promise(resolve => { result = resolve; }));
    const view = renderHook(() => useUpdater());
    await waitFor(() => expect(result).toBeDefined());
    view.unmount();
    await act(async () => { result(retained); await Promise.resolve(); });
    expect(host.check).not.toHaveBeenCalled();
    expect(host.resume).not.toHaveBeenCalled();
  });
});

describe("installed update cleanup barrier", () => {
  it("does not stop services or relaunch when install fails", async () => {
    host.install.mockRejectedValueOnce(new Error("install refused"));
    const view = await ready();
    await act(() => view.result.current.installAndRelaunch());
    expect(view.result.current.status).toMatchObject({ state: "error", message: "install refused" });
    expect(host.prepare).not.toHaveBeenCalled();
    expect(host.relaunch).not.toHaveBeenCalled();
  });

  it("does not relaunch when Computer Use cleanup fails", async () => {
    host.prepare.mockRejectedValueOnce(new Error("Computer Use cleanup incomplete"));
    const view = await ready();
    await act(() => view.result.current.installAndRelaunch());
    expect(host.relaunch).not.toHaveBeenCalled();
    expect(view.result.current.status).toMatchObject({ state: "error", message: "Computer Use cleanup incomplete" });
  });

  it("retries cleanup for the installed version without check, download or reinstall", async () => {
    host.prepare.mockRejectedValueOnce(new Error("Computer Use cleanup incomplete"));
    const view = await ready();
    await act(() => view.result.current.installAndRelaunch());
    expect(host.relaunch).not.toHaveBeenCalled();
    await act(() => view.result.current.applyAvailableUpdate());
    expect(host.check).toHaveBeenCalledTimes(1);
    expect(host.download).toHaveBeenCalledTimes(1);
    expect(host.install).toHaveBeenCalledTimes(1);
    expect(host.prepare).toHaveBeenCalledTimes(2);
    expect(host.relaunch).toHaveBeenCalledTimes(1);
  });

  it("cannot bypass an in-flight cleanup with a second install request", async () => {
    let release!: () => void;
    host.prepare.mockImplementationOnce(() => new Promise<void>(resolve => { release = resolve; }));
    const view = await ready();
    let first!: Promise<void>;
    act(() => { first = view.result.current.installAndRelaunch(); });
    await waitFor(() => expect(host.prepare).toHaveBeenCalledTimes(1));
    await act(() => view.result.current.applyAvailableUpdate());
    expect(host.relaunch).not.toHaveBeenCalled();
    expect(host.install).toHaveBeenCalledTimes(1);
    await act(async () => { release(); await first; });
    expect(host.relaunch).toHaveBeenCalledTimes(1);
  });

  it("keeps restart recovery visible and a check cannot reinstall or erase its error", async () => {
    host.prepare.mockRejectedValue(new Error("cleanup still pending"));
    const view = await ready();
    await act(() => view.result.current.installAndRelaunch());
    expect(view.result.current.status).toEqual({ state: "error", message: "cleanup still pending", version: "9.9.9-test", restartPending: true });
    expect(host.close).toHaveBeenCalledTimes(1);
    await act(() => view.result.current.checkForUpdate());
    expect(host.check).toHaveBeenCalledTimes(1);
    expect(view.result.current.status.state).toBe("error");
    await act(() => view.result.current.installAndRelaunch());
    expect(host.install).toHaveBeenCalledTimes(1);
    expect(host.prepare).toHaveBeenCalledTimes(2);
    expect(host.relaunch).not.toHaveBeenCalled();
  });

  it("a failed relaunch requires a fresh cleanup barrier but never reinstalls", async () => {
    host.relaunch.mockRejectedValueOnce(new Error("relaunch rejected"));
    const view = await ready();
    await act(() => view.result.current.installAndRelaunch());
    expect(view.result.current.status).toMatchObject({ state: "error", restartPending: true });
    await act(() => view.result.current.applyAvailableUpdate());
    expect(host.prepare).toHaveBeenCalledTimes(2);
    expect(host.install).toHaveBeenCalledTimes(1);
    expect(host.relaunch).toHaveBeenCalledTimes(2);
  });

  it("preference reseeding cannot clear an owned pending cleanup", async () => {
    let release!: () => void;
    host.prepare.mockImplementationOnce(() => new Promise<void>(resolve => { release = resolve; }));
    const view = await ready();
    let first!: Promise<void>;
    act(() => { first = view.result.current.installAndRelaunch(); });
    await waitFor(() => expect(host.prepare).toHaveBeenCalledTimes(1));
    act(() => { window.dispatchEvent(new Event("update-sim-test")); });
    await act(() => view.result.current.applyAvailableUpdate());
    expect(host.prepare).toHaveBeenCalledTimes(1);
    expect(host.install).toHaveBeenCalledTimes(1);
    expect(host.check).toHaveBeenCalledTimes(1);
    await act(async () => { release(); await first; });
  });

  it("does not close an active install or relaunch after its UI owner unmounts", async () => {
    let release!: () => void;
    host.install.mockImplementationOnce(() => new Promise<void>(resolve => { release = resolve; }));
    const view = await ready();
    let first!: Promise<void>;
    act(() => { first = view.result.current.installAndRelaunch(); });
    await waitFor(() => expect(host.install).toHaveBeenCalledTimes(1));
    view.unmount();
    expect(host.close).not.toHaveBeenCalled();
    await act(async () => { release(); await first; });
    expect(host.close).toHaveBeenCalledTimes(1);
    expect(host.prepare).not.toHaveBeenCalled();
    expect(host.relaunch).not.toHaveBeenCalled();
  });

  it("keeps the cleanup wait visible during retry without pretending to reinstall", async () => {
    host.prepare.mockRejectedValueOnce(new Error("cleanup pending"));
    const view = await ready();
    await act(() => view.result.current.installAndRelaunch());
    let release!: () => void;
    host.prepare.mockImplementationOnce(() => new Promise<void>(resolve => { release = resolve; }));
    let retry!: Promise<unknown>;
    act(() => { retry = view.result.current.applyAvailableUpdate(); });
    await waitFor(() => expect(host.prepare).toHaveBeenCalledTimes(2));
    expect(view.result.current.status).toEqual({ state: "preparing-restart", version: "9.9.9-test" });
    await act(async () => {
      expect(await view.result.current.applyAvailableUpdate()).toEqual({ kind: "busy" });
      await view.result.current.checkForUpdate();
    });
    expect(host.install).toHaveBeenCalledTimes(1);
    expect(host.check).toHaveBeenCalledTimes(1);
    expect(host.relaunch).not.toHaveBeenCalled();
    await act(async () => { release(); await retry; });
    expect(host.relaunch).toHaveBeenCalledTimes(1);
  });

  it("never relaunches a detached UI when its original cleanup completes", async () => {
    let release!: () => void;
    host.prepare.mockImplementationOnce(() => new Promise<void>(resolve => { release = resolve; }));
    const view = await ready();
    let first!: Promise<void>;
    act(() => { first = view.result.current.installAndRelaunch(); });
    await waitFor(() => expect(host.prepare).toHaveBeenCalledTimes(1));
    view.unmount();
    await act(async () => { release(); await first; });
    expect(host.prepare).toHaveBeenCalledTimes(1);
    expect(host.install).toHaveBeenCalledTimes(1);
    expect(host.close).toHaveBeenCalledTimes(1);
    expect(host.relaunch).not.toHaveBeenCalled();
  });
});
