import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { buildExportImagePipeline } from "./sessionExportImage";

const renderDom = vi.hoisted(() => vi.fn());
vi.mock("@/lib/shareCardDomExport", () => ({ rasterizeShareCardDomPng: renderDom }));

const input = {
  title: "Synthetic export",
  smart: false,
  messages: [{ role: "user" as const, content: "Synthetic message" }],
};
const canvasProbe = () => { throw new Error("canvas fallback reached"); };

beforeEach(() => {
  renderDom.mockReset();
  renderDom.mockResolvedValue(new Blob([new Uint8Array(1024)], { type: "image/png" }));
});
afterEach(() => vi.unstubAllGlobals());

describe("full export rendering capability", () => {
  it("uses canvas directly when only a canvas document shim exists", async () => {
    vi.stubGlobal("window", undefined);
    vi.stubGlobal("document", { createElement: canvasProbe });
    await expect(buildExportImagePipeline(input)).rejects.toThrow("canvas fallback reached");
    expect(renderDom).not.toHaveBeenCalled();
  });

  it("does not attempt browser capture before a document body exists", async () => {
    vi.stubGlobal("window", {});
    vi.stubGlobal("document", { body: null, createElement: canvasProbe });
    await expect(buildExportImagePipeline(input)).rejects.toThrow("canvas fallback reached");
    expect(renderDom).not.toHaveBeenCalled();
  });

  it("continues to prefer rich DOM rendering in a browser", async () => {
    vi.stubGlobal("window", {});
    vi.stubGlobal("document", { body: {}, createElement: canvasProbe });
    const result = await buildExportImagePipeline(input);
    expect(renderDom).toHaveBeenCalledTimes(1);
    expect(result.mode).toBe("full");
    expect(result.byteLength).toBe(1024);
  });

  it("retains the canvas fallback when browser capture rejects", async () => {
    vi.stubGlobal("window", {});
    vi.stubGlobal("document", { body: {}, createElement: canvasProbe });
    renderDom.mockRejectedValueOnce(new Error("capture unavailable"));
    await expect(buildExportImagePipeline(input)).rejects.toThrow("canvas fallback reached");
    expect(renderDom).toHaveBeenCalledTimes(1);
  });

  it("still fails explicitly without any rendering document", async () => {
    vi.stubGlobal("window", undefined);
    vi.stubGlobal("document", undefined);
    await expect(buildExportImagePipeline(input)).rejects.toThrow("requires a DOM");
    expect(renderDom).not.toHaveBeenCalled();
  });
});
