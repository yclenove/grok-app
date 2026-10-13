/** Map Host/Broker error text onto Computer panel i18n keys. */
export function computerUseErrorKey(
  message: string,
): "cu.panel.yoloNever" | "cu.panel.unsafeNavigate" | "cu.panel.permission" | "cu.panel.backendUnavailable" | "cu.surface.unsupported" | "cu.panel.error" | "cu.consent.managedMacosMinimum" | "cu.consent.managedOsVersionUnavailable" {
  const m = message.toLowerCase();
  if (m.includes("managed_browser_macos_requires_14:")) return "cu.consent.managedMacosMinimum";
  if (m.includes("managed_browser_os_version_unavailable:")) return "cu.consent.managedOsVersionUnavailable";
  if (
    m.includes("never grants desktop") ||
    m.includes("yolo") ||
    m.includes("acceptedits") ||
    m.includes("alwaysapprove")
  ) {
    return "cu.panel.yoloNever";
  }
  if (
    m.includes("navigation is not allowed") ||
    m.includes("navigate url") ||
    m.includes("javascript:") ||
    m.includes("file:")
  ) {
    return "cu.panel.unsafeNavigate";
  }
  if (
    m.includes("tcc") ||
    m.includes("accessibility") ||
    m.includes("screen recording") ||
    m.includes("permission_denied") ||
    m.includes("permission denied")
  ) {
    return "cu.panel.permission";
  }
  if (
    m.includes("native_wayland") ||
    m.includes("backend unavailable") ||
    m.includes("adapter unavailable")
  ) {
    return "cu.panel.backendUnavailable";
  }
  if (m.includes("unsupported_surface")) {
    return "cu.surface.unsupported";
  }
  return "cu.panel.error";
}

/** Only explicit staging maintenance can report retained native-download buffers. */
export function computerUseStagingCleanupErrorKey(
  message: string,
): ReturnType<typeof computerUseErrorKey> | "cu.settings.nativeCleanupRetained" | "cu.settings.nativeCleanupLive" {
  const m = message.toLowerCase();
  if (m.includes("native download")) {
    return m.includes("still live") ? "cu.settings.nativeCleanupLive" : "cu.settings.nativeCleanupRetained";
  }
  return computerUseErrorKey(message);
}
