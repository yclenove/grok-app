/** @vitest-environment jsdom */
import "@testing-library/jest-dom/vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import type { SettingsPageProps } from "@/components/settings/types";
import { OPEN_IMPORT_EVENT, type OpenImportDetail } from "@/lib/sessionImport/open";
import { SettingsPage } from "./SettingsPage";

vi.mock("@/lib/nativeWebviewCover", () => ({ acquireNativeWebviewCover: () => () => {} }));
vi.mock("@/lib/api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/api")>();
  return {
    ...actual,
    secretsGetMasked: () => Promise.resolve({
      hasOfficialKey: false,
      hasRelayKey: false,
      hasPexelsKey: false,
      hasSttCustomKey: false,
      sttCustomKeys: {},
      relayBaseUrl: null,
      defaultModel: null,
    }),
  };
});

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

async function chooseImportRecords() {
  const user = userEvent.setup();
  const search = screen.getByRole("textbox", { name: "Search settings…" });
  await user.type(search, "import records");
  const options = await screen.findAllByRole("option");
  const option = options.find((item) => (item.textContent ?? "").startsWith("Import records"));
  expect(option).toBeTruthy();
  await user.click(option!);
  return user;
}

afterEach(() => {
  cleanup();
  scrolled.length = 0;
  HTMLElement.prototype.scrollIntoView = originalScrollIntoView;
});

test("same-panel search reaches the real import entry", async () => {
  HTMLElement.prototype.scrollIntoView = function scrollIntoView() {
    scrolled.push((this as HTMLElement).id);
  };
  const opened: OpenImportDetail[] = [];
  const onOpen = (event: Event) => opened.push((event as CustomEvent<OpenImportDetail>).detail);
  window.addEventListener(OPEN_IMPORT_EVENT, onOpen);
  try {
    render(<SettingsPage {...pageProps({ tab: "app" })} />);
    const anchor = document.getElementById("settings-anchor-sessionImport");
    expect(anchor?.querySelector("button")?.textContent).toBe("Import sessions and workspaces");
    scrolled.length = 0;
    const user = await chooseImportRecords();
    await waitFor(() => expect(scrolled).toContain("settings-anchor-sessionImport"));
    await user.click(screen.getByRole("button", { name: "Import sessions and workspaces" }));
    expect(opened).toEqual([{ mode: "wizard" }]);
  } finally {
    window.removeEventListener(OPEN_IMPORT_EVENT, onOpen);
  }
});

test("cross-panel search paints General App and scrolls to the import anchor", async () => {
  HTMLElement.prototype.scrollIntoView = function scrollIntoView() {
    scrolled.push((this as HTMLElement).id);
  };
  render(<SettingsPage {...pageProps({ tab: "composer" })} />);
  expect(document.getElementById("settings-anchor-sessionImport")).toBeNull();
  await chooseImportRecords();
  await waitFor(() => expect(document.getElementById("settings-anchor-sessionImport")).toBeTruthy());
  await waitFor(() => expect(scrolled).toContain("settings-anchor-sessionImport"));
  expect(document.getElementById("settings-anchor-sessionImport")?.querySelector("button")?.textContent)
    .toBe("Import sessions and workspaces");
});
