/** App-owned settings projections. Doctor retains the existing engine wire shape. */
export type Confirmation = "off" | "standard" | "strict";
export type Foreground = "never" | "ask" | "always";
export type PermissionKind = "accessibility" | "screen_recording" | "input_monitoring";
export type Settings = { enabled: boolean; confirmation: Confirmation; foreground: Foreground; hotkey: string };
export type PngIcon = { mime: string; width: number; height: number; bytes: number[] };
export type Approval = { id: string; name: string; imagePath: string; grantedMs: number | null; icon: PngIcon | null };
export type Doctor = {
  ready: boolean;
  issues: string[];
  capabilities: {
    platform: "macos" | "windows" | "linux";
    version: string;
    observe: { tree: boolean; screenshot: boolean; events: boolean };
    delivery: { accessibility: boolean; targeted_pointer: boolean; targeted_keys: boolean; foreground: "policy" | "unavailable" };
    background_operation: "full" | "partial" | "none";
    user_activity_guard: boolean;
  };
};
export type PermissionStatus = "granted" | "missing" | "unknown";

/** Ready/missing metadata is evidence; an unrelated probe failure is not a grant. */
export function permissionStatus(report: Doctor, kind: PermissionKind): PermissionStatus {
  if (kind === "input_monitoring") return report.capabilities.user_activity_guard ? "granted" : "unknown";
  if (report.issues.includes(`permission missing: ${kind}`)) return "missing";
  return report.ready ? "granted" : "unknown";
}

/** Icons are display-only, bounded inline PNGs; no filesystem or network URL. */
export function iconSrc(icon: PngIcon | null): string | undefined {
  const magic = [137, 80, 78, 71, 13, 10, 26, 10];
  if (!icon || icon.mime !== "image/png" || icon.width < 1 || icon.width > 256 || icon.height < 1 || icon.height > 256 || icon.bytes.length > 65536 || !magic.every((b, i) => icon.bytes[i] === b)) return undefined;
  let binary = "";
  for (const byte of icon.bytes) binary += String.fromCharCode(byte);
  return `data:image/png;base64,${btoa(binary)}`;
}
