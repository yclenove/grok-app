import { describe, expect, it } from "vitest";
import { computerUseErrorKey, computerUseStagingCleanupErrorKey } from "./errors";

describe("computerUseErrorKey", () => {
  it("distinguishes managed macOS support from repair, permissions and other surfaces", () => {
    expect(computerUseErrorKey("managed_browser_macos_requires_14: older native macOS")).toBe("cu.consent.managedMacosMinimum");
    expect(computerUseErrorKey("managed_browser_os_version_unavailable: read failed")).toBe("cu.consent.managedOsVersionUnavailable");
    expect(computerUseErrorKey("macOS browser failed")).toBe("cu.panel.error");
    expect(computerUseErrorKey("permission denied on macOS")).toBe("cu.panel.permission");
  });
  it("explains retained native-download files only during explicit staging maintenance", () => {
    expect(computerUseStagingCleanupErrorKey("native download original App or browser is still live; retained")).toBe("cu.settings.nativeCleanupLive");
    for (const message of ["native download buffer directory replaced", "native download journal filesystem unavailable or changed", "native download retirement receipt invalid"]) {
      expect(computerUseStagingCleanupErrorKey(message)).toBe("cu.settings.nativeCleanupRetained");
      expect(computerUseErrorKey(message)).toBe("cu.panel.error");
    }
  });
  it("keeps ordinary native-download failures and non-native maintenance errors distinct", () => {
    expect(computerUseErrorKey("native download original App or browser is still live; retained")).toBe("cu.panel.error");
    expect(computerUseErrorKey("native download journal publication failed")).toBe("cu.panel.error");
    expect(computerUseStagingCleanupErrorKey("permission denied")).toBe("cu.panel.permission");
    expect(computerUseStagingCleanupErrorKey("timeout")).toBe("cu.panel.error");
  });
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
