import { describe, expect, it } from "vitest";
import type { ComputerStatus } from "@/lib/api/computerUse";
import { computerTaskState, computerTaskView } from "./taskCard";

const base = (patch: Partial<ComputerStatus> = {}): ComputerStatus => ({
  featureEnabled: true,
  enabled: true,
  runId: "run",
  targetId: "window",
  targetName: "Editor",
  paused: false,
  stopState: "running",
  backend: "windows",
  notes: [],
  traces: [],
  recovery: null,
  timings: null,
  targetAlive: true,
  mcpCatalog: null,
  ...patch,
});

describe("computerTaskView", () => {
  it("keeps Host target, backend, and stop state instead of hardcoding running", () => {
    expect(computerTaskView(base({ featureEnabled: false }))?.state).toBe("unauthorized");
    expect(computerTaskView(base({ featureEnabled: false, stopState: "stopped" }))).toBeNull();
    expect(computerTaskView(base({ runId: null }))).toBeNull();
    expect(computerTaskView(base({ targetName: null, targetId: "hwnd:42" }))).toEqual({
      targetName: "hwnd:42",
      backend: "windows",
      state: "running",
      failure: null,
    });
    expect(computerTaskView(base({ stopState: "stopped" }))?.state).toBe("stopped");
    expect(computerTaskView(base({
      stopState: "stopped",
      mcpCatalog: {
        desiredGeneration: 4,
        appliedGeneration: 3,
        desiredPresent: false,
        pending: true,
        cleanupPending: true,
        lastError: "update timed out",
      },
    }))?.state).toBe("mcp_cleanup_pending");
    expect(computerTaskView(base({
      mcpCatalog: {
        desiredGeneration: 2,
        appliedGeneration: 1,
        desiredPresent: true,
        pending: true,
        cleanupPending: false,
        lastError: null,
      },
    }))?.state).toBe("mcp_pending");
    expect(computerTaskView(base({ paused: true }))?.state).toBe("paused");
    expect(computerTaskView(base({ targetAlive: false }))?.state).toBe("closed");
    expect(computerTaskView(base({ recovery: "user" }))?.state).toBe("unauthorized");
    expect(computerTaskState(base(), false)).toBe("unknown");
    expect(computerTaskState(base({
      stopState: "stop_requested",
      mcpCatalog: { desiredGeneration: 3, appliedGeneration: 2, desiredPresent: false,
        pending: true, cleanupPending: true, lastError: null },
    }))).toBe("stop_requested");
    expect(
      computerTaskView(
        base({
          traces: [{
            kind: "act",
            runId: "run",
            detail: "click Verified executed=true",
            ms: 1,
            audience: "model",
          }, {
            kind: "act",
            runId: "run",
            detail: "click Rejected executed=false",
            ms: 2,
            audience: "ui",
          }],
        }),
      )?.failure,
    ).toContain("Rejected");
    expect(
      computerTaskView(
        base({
          traces: [{
            kind: "act",
            runId: "run",
            detail: "click Rejected executed=false",
            ms: 1,
            audience: "ui",
          }, {
            kind: "act",
            runId: "run",
            detail: "click Verified executed=true",
            ms: 2,
            audience: "model",
          }],
        }),
      )?.failure,
    ).toBeNull();
  });
});
