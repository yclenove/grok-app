/** App-owned bundled files only. No paths, browser profiles or grant fields in invokes. */
import { invoke, isDesktopHost } from "./host";
export type ComputerExtensionInfo = { version: string; extensionId: string; minimumBrowserVersion: string; sha256: string; fileCount: number };
export type ComputerExtensionExport = ComputerExtensionInfo & { path: string };
function desktop() { if (!isDesktopHost()) throw new Error("Computer Use extension requires the local desktop host"); }
function validate(info: ComputerExtensionInfo) {
  if (!info || !/^\d+\.\d+\.\d+$/.test(info.version) || !/^[a-p]{32}$/.test(info.extensionId)
    || !/^\d+$/.test(info.minimumBrowserVersion) || !/^[a-f0-9]{64}$/.test(info.sha256)
    || !Number.isSafeInteger(info.fileCount) || info.fileCount < 1) throw new Error("Invalid bundled extension metadata");
  return info;
}
export async function computerExtensionInfo() {
  desktop(); return validate(await invoke<ComputerExtensionInfo>("computer_use_extension_info"));
}
export async function computerExtensionExport() {
  desktop(); const result = await invoke<ComputerExtensionExport>("computer_use_extension_export"); validate(result);
  if (typeof result.path !== "string" || !/^(?:[a-zA-Z]:[\\/]|\/)/.test(result.path) || /[\r\n\0]/.test(result.path)) throw new Error("Invalid extension export path");
  return result;
}
export async function computerExtensionOpen() { desktop(); await invoke<void>("computer_use_extension_open"); }
