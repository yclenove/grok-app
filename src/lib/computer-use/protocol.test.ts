import { describe, expect, it } from "vitest";
import {
  COMPUTER_USE_PROTOCOL_VERSION,
  coordActionBlockedWithoutImage,
  staleSnapshotCannotClick,
  validateActionRequest,
  type ActionRequest,
} from "./protocol";

function base(over: Partial<ActionRequest> = {}): ActionRequest {
  return {
    version: COMPUTER_USE_PROTOCOL_VERSION,
    actionId: "a1",
    runId: "r1",
    targetId: "t1",
    targetGeneration: 1,
    snapshotId: "s1",
    geometryRevision: 1,
    action: "click",
    target: { elementRef: "n1" },
    parameters: {},
    ...over,
  };
}

describe("computer-use protocol helpers", () => {
  it("rejects wrong version and empty ids via the shipped validator", () => {
    expect(validateActionRequest(base({ version: 9 }))).toMatch(/version/);
    expect(validateActionRequest(base({ actionId: " " }))).toMatch(/actionId/);
  });

  it("rejects YOLO parameters and accepts a right-click", () => {
    expect(
      validateActionRequest(base({ parameters: { yolo: true, acceptEdits: true } })),
    ).toMatch(/never grants desktop control/);
    expect(
      validateActionRequest(base({ parameters: { button: "right", count: 2 } })),
    ).toBeNull();
    expect(
      validateActionRequest(base({ parameters: { button: "double" } })),
    ).toMatch(/button/);
  });

  it("rejects alt+f4 and accepts arrow keys via the shipped validator", () => {
    expect(
      validateActionRequest(base({ action: "key", parameters: { key: "alt+f4" } })),
    ).toMatch(/allowed set/);
    expect(
      validateActionRequest(base({ action: "key", parameters: { key: "down" } })),
    ).toBeNull();
  });

  it("does not let set_value use coordinates", () => {
    expect(
      validateActionRequest(
        base({ action: "set_value", target: { x: 1, y: 2 }, parameters: { text: "x" } }),
      ),
    ).toMatch(/elementRef/);
  });

  it("accepts wait on an element and rejects coord or zero timeout", () => {
    expect(
      validateActionRequest(
        base({ action: "wait", parameters: { nameEquals: "Count", timeoutMs: 2000 } }),
      ),
    ).toBeNull();
    expect(
      validateActionRequest(
        base({
          action: "wait",
          target: { x: 4, y: 4 },
          parameters: { nameEquals: "Count" },
        }),
      ),
    ).toMatch(/elementRef/);
    expect(
      validateActionRequest(
        base({ action: "wait", parameters: { nameEquals: "Count", timeoutMs: 0 } }),
      ),
    ).toMatch(/timeoutMs/);
  });

  it("blocks clicks on a stale or hidden preview snapshot", () => {
    expect(
      staleSnapshotCannotClick({
        previewHidden: true,
        snapshotId: "s1",
        clickSnapshotId: "s0",
      }),
    ).toBe(true);
    expect(
      staleSnapshotCannotClick({
        previewHidden: false,
        snapshotId: "s1",
        clickSnapshotId: "s1",
      }),
    ).toBe(false);
  });

  it("blocks coordinate actions when the observation has no image", () => {
    expect(
      coordActionBlockedWithoutImage({
        target: { x: 8, y: 12 },
        hasImage: false,
      }),
    ).toBe(true);
    expect(
      coordActionBlockedWithoutImage({
        target: { x: 8, y: 12 },
        hasImage: true,
      }),
    ).toBe(false);
    expect(
      coordActionBlockedWithoutImage({
        target: { elementRef: "n1" },
        hasImage: false,
      }),
    ).toBe(false);
  });
});
