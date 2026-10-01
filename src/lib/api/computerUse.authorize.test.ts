import { describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("./host", () => ({ isTauri: () => true, invoke }));

import { computerAuthorizeSurface } from "./computerUse";

describe("Computer Use authorization IPC", () => {
  it("passes the typed request under Tauri's request argument", async () => {
    const request = {
      sessionId: "local-chat",
      runId: null,
      attemptId: "ui-attempt-1",
      selectorRevision: 3,
      surface: "desktop",
      targetId: "win:fixture",
    };
    const result = {
      attemptId: request.attemptId,
      selectorRevision: request.selectorRevision,
      runId: "run-1",
      target: {
        targetId: request.targetId,
        title: "Fixture",
        appName: "Fixture",
        kind: "window",
        surface: request.surface,
      },
    };
    invoke.mockResolvedValueOnce(result);

    await expect(computerAuthorizeSurface(request)).resolves.toEqual(result);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("computer_use_authorize_surface", {
      request,
    });
  });
});
