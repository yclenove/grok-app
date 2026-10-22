/** @vitest-environment jsdom */
import "@testing-library/jest-dom/vitest";
import "@/test/jsdomStubs";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, test, vi } from "vitest";
import type { SettingsPageProps } from "@/components/settings/types";

const gate = vi.hoisted(() => ({ delayImport: false }));

vi.mock("@/components/settings/GeneralSection", async () => {
  const React = await import("react");
  const { useSettingsModel } = await import("@/providers/SettingsModelContext");
  return {
    GeneralSection() {
      const model = useSettingsModel();
      const [importReady, setImportReady] = React.useState(!gate.delayImport);
      React.useEffect(() => {
        if (model.activeTab !== "app") return;
        if (!gate.delayImport) {
          setImportReady(true);
          return;
        }
        const timer = window.setTimeout(() => setImportReady(true), 120);
        return () => window.clearTimeout(timer);
      }, [model.activeTab]);
      if (model.activeTab !== "app") {
        return React.createElement("section", { id: "panel-composer", "data-panel": "composer" }, "composer");
      }
      return React.createElement(
        "section",
        { "data-panel": "app" },
        React.createElement("div", { id: "settings-anchor-language" }, "Language row"),
        importReady ? React.createElement("div", { id: "settings-anchor-sessionImport" }, "Import row") : null,
      );
    },
  };
});

import { useState } from "react";
import { SettingsPage } from "./SettingsPage";
import type { SettingsSectionId } from "@/components/settings/types";
import { SkinShareProvider } from "@/providers/SkinShareProvider";
import { ThemeProvider } from "@/providers/ThemeProvider";

const scrolled: string[] = [];
const originalScrollIntoView = HTMLElement.prototype.scrollIntoView;

function pageProps(over: Partial<SettingsPageProps> = {}): SettingsPageProps {
  return {
    section: "general",
    tab: "app",
    onSection: vi.fn(),
    onBack: vi.fn(),
    labels: {},
    locale: "en",
    onLocale: vi.fn(),
    theme: "dark",
    onTheme: vi.fn(),
    sessionDataMode: "independent",
    onSessionDataMode: vi.fn(),
    policy: "default",
    onPolicy: vi.fn(),
    manualCliPath: "",
    onManualCliPath: vi.fn(),
    onCliBlur: vi.fn(),
    acpServerAddr: "",
    onAcpServerAddr: vi.fn(),
    onAcpServerBlur: vi.fn(),
    cliInfo: {
      found: false,
      path: null,
      version: null,
      source: "test",
      cliAuthPresent: false,
    },
    onDoctor: vi.fn(),
    versionFooter: "test",
    account: null,
    accountLoading: false,
    accountBusy: false,
    onAccountLoginOauth: vi.fn(),
    ...over,
  } as SettingsPageProps;
}

async function jumpToLanguage() {
  const user = userEvent.setup();
  const search = screen.getByRole("textbox", { name: "Search settings…" });
  await user.type(search, "language");
  const options = await screen.findAllByRole("option");
  const language = options.find((option) => /Language/.test(option.textContent ?? "") && /App/.test(option.textContent ?? ""));
  expect(language).toBeTruthy();
  await user.click(language!);
}

function installScrollSpy() {
  HTMLElement.prototype.scrollIntoView = function scrollIntoView() {
    scrolled.push((this as HTMLElement).id);
  };
}

async function jumpToImport() {
  const user = userEvent.setup();
  const search = screen.getByRole("textbox", { name: "Search settings…" });
  await user.clear(search);
  await user.type(search, "import records");
  const option = await screen.findByRole("option", { name: /Import records/ });
  await user.click(option);
}

describe("Settings language search routes", () => {
  afterEach(() => {
    cleanup();
    scrolled.length = 0;
    gate.delayImport = false;
    HTMLElement.prototype.scrollIntoView = originalScrollIntoView;
  });

  test("same-panel search scrolls to Language without leaving General App", async () => {
    const onSection = vi.fn();
    installScrollSpy();
    render(<SettingsPage {...pageProps({ tab: "app", onSection })} />);
    expect(document.getElementById("settings-anchor-language")).toBeTruthy();
    scrolled.length = 0;
    await jumpToLanguage();
    await waitFor(() => expect(scrolled).toContain("settings-anchor-language"));
    expect(onSection).toHaveBeenCalledWith("general", "app");
    expect(document.querySelector("[data-panel='app']")).toBeTruthy();
    expect(document.getElementById("panel-composer")).toBeNull();
  });

  test("cross-panel search leaves Composer, opens App, and scrolls to Language", async () => {
    const onSection = vi.fn();
    installScrollSpy();
    render(<SettingsPage {...pageProps({ tab: "composer", onSection })} />);
    expect(document.getElementById("panel-composer")).toBeTruthy();
    expect(document.getElementById("settings-anchor-language")).toBeNull();
    await jumpToLanguage();
    await waitFor(() => expect(document.getElementById("settings-anchor-language")).toBeTruthy());
    await waitFor(() => expect(scrolled).toContain("settings-anchor-language"));
    expect(onSection).toHaveBeenCalledWith("general", "app");
    expect(document.getElementById("panel-composer")).toBeNull();
  });

  test("a late import anchor still scrolls after the first paint misses it", async () => {
    gate.delayImport = true;
    installScrollSpy();
    render(<SettingsPage {...pageProps({ tab: "composer", onSection: vi.fn() })} />);
    await jumpToImport();
    expect(document.getElementById("settings-anchor-sessionImport")).toBeNull();
    await waitFor(() => expect(scrolled).toContain("settings-anchor-sessionImport"));
    expect(document.getElementById("settings-anchor-language")).toBeTruthy();
  });

  test("a newer section wins over a stale import anchor", async () => {
    gate.delayImport = true;
    installScrollSpy();
    function Controlled() {
      const [section, setSection] = useState<SettingsSectionId>("general");
      const [tab, setTab] = useState<string | null>("composer");
      return (
        <SettingsPage
          {...pageProps({
            section,
            tab,
            onSection: (id, next) => {
              if (id === "general" && next === "app") {
                setSection("appearance");
                setTab("theme");
                return;
              }
              setSection(id);
              setTab(next ?? null);
            },
          })}
        />
      );
    }
    render(
      <ThemeProvider>
        <SkinShareProvider>
          <Controlled />
        </SkinShareProvider>
      </ThemeProvider>,
    );
    await jumpToImport();
    const stray = document.createElement("div");
    stray.id = "settings-anchor-sessionImport";
    document.body.appendChild(stray);
    await new Promise((resolve) => window.setTimeout(resolve, 450));
    expect(scrolled).not.toContain("settings-anchor-sessionImport");
    expect(document.querySelector("[data-panel='app']")).toBeNull();
    stray.remove();
  });
});
