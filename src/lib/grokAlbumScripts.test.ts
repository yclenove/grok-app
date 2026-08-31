import { runInNewContext } from "node:vm";
import { describe, expect, it, vi } from "vitest";

import scrollRestoreScript from "../../src-tauri/src/wallpaper_grok_album/scroll_restore.js?raw";
import signedOutRecoveryScript from "../../src-tauri/src/wallpaper_grok_album/signed_out_recovery.js?raw";
import snapshotScript from "../../src-tauri/src/wallpaper_grok_album/snapshot.js?raw";

function runSnapshot(options: {
  appShell?: boolean;
  challengeAsset?: boolean;
  challengeSurface?: boolean;
}) {
  const root = {
    scrollTop: 0,
    scrollHeight: 900,
    clientHeight: 900,
  };
  const document = {
    readyState: "complete",
    scrollingElement: root,
    documentElement: root,
    body: root,
    querySelectorAll: () => [],
    querySelector: (selector: string) => {
      if (selector === "main") return options.appShell ? {} : null;
      if (selector.includes("script[src*")) {
        return options.challengeAsset ? {} : null;
      }
      if (selector.includes("#challenge-stage")) {
        return options.challengeSurface ? {} : null;
      }
      return null;
    },
  };
  const page = { innerHeight: 900 };
  const raw = runInNewContext(snapshotScript, {
    URL,
    document,
    getComputedStyle: () => ({ overflowY: "visible" }),
    location: { href: "https://grok.com/imagine/saved" },
    window: page,
  });
  return JSON.parse(String(raw)) as {
    hasAppShell: boolean;
    hasSecurityChallenge: boolean;
  };
}

function runSignedOutRecovery(options: {
  appShell?: boolean;
  challengeAsset?: boolean;
  challengeSurface?: boolean;
}) {
  const replace = vi.fn();
  const document = {
    readyState: "complete",
    querySelector: (selector: string) => {
      if (selector === "main") return options.appShell ? {} : null;
      if (selector.includes("script[src*")) {
        return options.challengeAsset ? {} : null;
      }
      if (selector.includes("#challenge-stage")) {
        return options.challengeSurface ? {} : null;
      }
      return null;
    },
  };
  const location = {
    protocol: "https:",
    hostname: "grok.com",
    pathname: "/imagine/saved",
    replace,
  };
  runInNewContext(signedOutRecoveryScript, {
    document,
    location,
    window: {
      addEventListener: vi.fn(),
      setTimeout: (callback: () => void) => callback(),
    },
  });
  return replace;
}

describe("Grok album fixed page scripts", () => {
  it("restores and clears the saved scroll offset", () => {
    const scrollTo = vi.fn();
    const root = { scrollTop: 0, scrollTo };
    const page = {
      __GROK_APP_ALBUM_SCROLL_RESTORE__: { root, top: 137 },
    };

    runInNewContext(scrollRestoreScript, { window: page });

    expect(scrollTo).toHaveBeenCalledWith({
      top: 137,
      left: 0,
      behavior: "auto",
    });
    expect("__GROK_APP_ALBUM_SCROLL_RESTORE__" in page).toBe(false);
  });

  it("falls back to scrollTop and clamps an invalid offset", () => {
    const root = { scrollTop: 99 };
    const page = {
      __GROK_APP_ALBUM_SCROLL_RESTORE__: { root, top: -10 },
    };

    runInNewContext(scrollRestoreScript, { window: page });

    expect(root.scrollTop).toBe(0);
    expect("__GROK_APP_ALBUM_SCROLL_RESTORE__" in page).toBe(false);
  });

  it("recognizes a full-page Cloudflare challenge from its bundled assets", () => {
    expect(runSnapshot({ challengeAsset: true })).toMatchObject({
      hasAppShell: false,
      hasSecurityChallenge: true,
    });
  });

  it("does not treat a normal Grok shell as a full-page challenge", () => {
    expect(
      runSnapshot({ appShell: true, challengeAsset: true }),
    ).toMatchObject({
      hasAppShell: true,
      hasSecurityChallenge: false,
    });
  });

  it("keeps recognizing explicit challenge surfaces", () => {
    expect(
      runSnapshot({ appShell: true, challengeSurface: true }),
    ).toMatchObject({
      hasAppShell: true,
      hasSecurityChallenge: true,
    });
  });

  it("redirects only the completed signed-out Saved shell", () => {
    expect(runSignedOutRecovery({})).toHaveBeenCalledWith("https://grok.com/");
    expect(runSignedOutRecovery({ appShell: true })).not.toHaveBeenCalled();
  });

  it("does not redirect Cloudflare challenge pages", () => {
    expect(
      runSignedOutRecovery({ challengeAsset: true }),
    ).not.toHaveBeenCalled();
    expect(
      runSignedOutRecovery({ challengeSurface: true }),
    ).not.toHaveBeenCalled();
  });
});
