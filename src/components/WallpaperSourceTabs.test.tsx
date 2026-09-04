/**
 * @vitest-environment jsdom
 */
import { useState } from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import "@/test/jsdomStubs";
import type { MessageKey } from "@/i18n";
import type { WallpaperSourceKind } from "@/lib/wallpaperSource";
import { WallpaperSourceTabs } from "./WallpaperSourceTabs";

afterEach(cleanup);

function TabsHarness() {
  const [value, setValue] = useState<WallpaperSourceKind>("x");
  return (
    <WallpaperSourceTabs
      t={(key: MessageKey) => key}
      value={value}
      disabled={false}
      panelId="wallpaper-panel"
      onChange={setValue}
    />
  );
}

describe("WallpaperSourceTabs", () => {
  it("uses one roving tab stop and supports Arrow, Home, and End keys", () => {
    render(<TabsHarness />);
    const x = screen.getByRole<HTMLButtonElement>("tab", {
      name: "settings.wallpaperFromX",
    });
    const web = screen.getByRole<HTMLButtonElement>("tab", {
      name: "settings.wallpaperWeb",
    });
    const library = screen.getByRole<HTMLButtonElement>("tab", {
      name: "settings.wallpaperLibrary",
    });

    expect(x.tabIndex).toBe(0);
    expect(web.tabIndex).toBe(-1);
    x.focus();
    fireEvent.keyDown(x, { key: "ArrowRight" });
    expect(document.activeElement).toBe(web);
    expect(web.tabIndex).toBe(0);
    expect(x.tabIndex).toBe(-1);

    fireEvent.keyDown(web, { key: "End" });
    expect(document.activeElement).toBe(library);
    expect(library.tabIndex).toBe(0);

    fireEvent.keyDown(library, { key: "Home" });
    expect(document.activeElement).toBe(x);
    fireEvent.keyDown(x, { key: "ArrowLeft" });
    expect(document.activeElement).toBe(library);
  });
});
