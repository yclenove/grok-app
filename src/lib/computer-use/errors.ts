/** Map Host/Broker error text onto Computer panel i18n keys. */
export function computerUseErrorKey(
  message: string,
): "cu.panel.yoloNever" | "cu.panel.unsafeNavigate" | "cu.panel.permission" | "cu.panel.backendUnavailable" | "cu.surface.unsupported" | "cu.panel.error" {
  const m = message.toLowerCase();
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
