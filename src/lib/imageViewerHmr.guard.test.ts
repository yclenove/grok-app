import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const sourceRoot = resolve(__dirname, "..");
const provider = readFileSync(
  resolve(sourceRoot, "components/ImageViewer.tsx"),
  "utf8",
);
const context = readFileSync(
  resolve(sourceRoot, "components/ImageViewerContext.ts"),
  "utf8",
);
const consumers = [
  "components/AttachmentCard.tsx",
  "components/ImageUi.tsx",
  "components/WallpaperSourceModal.tsx",
] as const;

describe("ImageViewerProvider HMR boundary", () => {
  it("keeps the context identity outside the component module", () => {
    expect(provider).toContain(
      'from "@/components/ImageViewerContext"',
    );
    expect(provider).not.toContain("createContext(");
    expect(provider).not.toContain("export function useImageViewer");
    expect(context).toContain(
      "export const ImageViewerContext = createContext",
    );
    expect(context).toContain("export function useImageViewer");
    expect(context).toContain("export function useImageViewerOptional");
  });

  it.each(consumers)("imports the stable context from %s", (relativePath) => {
    const consumer = readFileSync(resolve(sourceRoot, relativePath), "utf8");
    expect(consumer).toContain(
      'from "@/components/ImageViewerContext"',
    );
    expect(consumer).not.toContain(
      'from "@/components/ImageViewer"',
    );
  });
});
