import { describe, expect, it } from "vitest";
import { computerUseErrorKey } from "./errors";

describe("computerUseErrorKey", () => {
  it("maps yolo and unsafe navigate onto dedicated panel keys", () => {
    expect(computerUseErrorKey("yolo never grants desktop control")).toBe("cu.panel.yoloNever");
    expect(computerUseErrorKey("javascript: navigation is not allowed")).toBe(
      "cu.panel.unsafeNavigate",
    );
    expect(computerUseErrorKey("timeout")).toBe("cu.panel.error");
    expect(computerUseErrorKey("screen recording permission denied")).toBe(
      "cu.panel.permission",
    );
    expect(computerUseErrorKey("native_wayland is false")).toBe(
      "cu.panel.backendUnavailable",
    );
    expect(computerUseErrorKey("unsupported_surface: app-webview")).toBe(
      "cu.surface.unsupported",
    );
  });
});
