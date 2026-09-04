import { describe, expect, it } from "vitest";
import {
  pinReviewFocusPath,
  reviewEntryCoversPath,
  reviewFocusPathParts,
} from "./reviewFocusPaths";

describe("reviewFocusPaths", () => {
  it("pins newest path first and dedupes", () => {
    expect(pinReviewFocusPath([], ".grok-app-998-mvp-b.txt")).toEqual([
      ".grok-app-998-mvp-b.txt",
    ]);
    expect(
      pinReviewFocusPath(
        [".grok-app-998-mvp-a.txt"],
        ".grok-app-998-mvp-b.txt",
      ),
    ).toEqual([".grok-app-998-mvp-b.txt", ".grok-app-998-mvp-a.txt"]);
    expect(
      pinReviewFocusPath(
        [".grok-app-998-mvp-b.txt", ".grok-app-998-mvp-a.txt"],
        ".grok-app-998-mvp-b.txt",
      ),
    ).toEqual([".grok-app-998-mvp-b.txt", ".grok-app-998-mvp-a.txt"]);
  });

  it("builds parts for dotted relative paths", () => {
    const p = reviewFocusPathParts(
      ".grok-app-998-mvp-b.txt",
      "/Users/me/proj",
    );
    expect(p.name).toBe(".grok-app-998-mvp-b.txt");
    expect(p.relPath).toBe(".grok-app-998-mvp-b.txt");
    expect(p.key).toBe("focus:.grok-app-998-mvp-b.txt");
  });

  it("matches entries by basename", () => {
    expect(
      reviewEntryCoversPath(
        { path: "/Users/me/proj/.grok-app-998-mvp-b.txt", relPath: ".grok-app-998-mvp-b.txt" },
        ".grok-app-998-mvp-b.txt",
        "/Users/me/proj",
      ),
    ).toBe(true);
  });
});
