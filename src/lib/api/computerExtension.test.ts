import { beforeEach, expect, it, vi } from "vitest";
const { invoke, isDesktopHost } = vi.hoisted(() => ({ invoke: vi.fn(), isDesktopHost: vi.fn() }));
vi.mock("./host", () => ({ invoke, isDesktopHost }));
import { computerExtensionInfo, computerExtensionExport, computerExtensionOpen } from "./computerExtension";
const info = { version: "1.1.0", extensionId: "bgegbabkegkanjbmjbeaockdijnbkjgi", minimumBrowserVersion: "112", sha256: "a".repeat(64), fileCount: 44 };
beforeEach(() => { invoke.mockReset(); isDesktopHost.mockReturnValue(true); });
it("gets embedded metadata and exports/opens only the Host-derived directory, without any arguments", async () => {
  invoke.mockResolvedValueOnce(info).mockResolvedValueOnce({ ...info, path: "H:\\App data\\browser-extension" });
  expect(await computerExtensionInfo()).toEqual(info);
  expect((await computerExtensionExport()).path).toBe("H:\\App data\\browser-extension"); await computerExtensionOpen();
  expect(invoke.mock.calls).toEqual([["computer_use_extension_info"], ["computer_use_extension_export"], ["computer_use_extension_open"]]);
});
it("refuses browser/mirror clients before any host call", async () => {
  isDesktopHost.mockReturnValue(false);
  for (const call of [computerExtensionInfo, computerExtensionExport, computerExtensionOpen]) await expect(call()).rejects.toThrow("local desktop host");
  expect(invoke).not.toHaveBeenCalled();
});
it.each([{ version: "../../escape" }, { extensionId: "invalid" }, { sha256: "invalid" }, { fileCount: 0 }, { minimumBrowserVersion: "remote" }])("rejects malformed embedded metadata: %j", async patch => {
  invoke.mockResolvedValue({ ...info, ...patch }); await expect(computerExtensionInfo()).rejects.toThrow("metadata");
});
it.each(["relative", "https://remote.example/extension", "H:/bad\npath", "H:/bad\0path"])("rejects malformed path %j", async path => {
  invoke.mockResolvedValue({ ...info, path }); await expect(computerExtensionExport()).rejects.toThrow("export path");
});
