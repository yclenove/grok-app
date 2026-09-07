/**
 * @vitest-environment jsdom
 */
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { WallpaperXSearchMode } from "@/lib/wallpaperXSearch";
import {
  WallpaperSourceControls,
  type WallpaperSourceControlsProps,
} from "./WallpaperSourceControls";

vi.mock("@/components/Select", () => ({
  Select: ({
    value,
    disabled,
    onChange,
    "aria-label": ariaLabel,
  }: {
    value: string;
    disabled?: boolean;
    onChange: (value: string) => void;
    "aria-label"?: string;
  }) => (
    <button
      type="button"
      aria-label={ariaLabel}
      disabled={disabled}
      onClick={() =>
        onChange(value === "cli" ? "responses_preview" : "top")
      }
    >
      {value}
    </button>
  ),
}));

const t = (key: string) => key;

function props(
  overrides: Partial<WallpaperSourceControlsProps> = {},
): WallpaperSourceControlsProps {
  return {
    t: t as WallpaperSourceControlsProps["t"],
    tab: "x",
    locked: false,
    busy: false,
    xSearchBusy: false,
    remoteSearchBusy: false,
    remoteSearchDisabled: false,
    hasPexelsKey: false,
    pexelsKeyInvalid: false,
    query: "mountains",
    sort: "top",
    sortOptions: [{ value: "top", label: "Top" }],
    xSearchMode: "cli",
    imagine: {
      mode: "image",
      prompt: "",
      aspect: "16:9",
      aspectOptions: [{ value: "16:9", label: "16:9" }],
      videoDuration: 6,
      videoResolution: "480p",
      videoSource: null,
      videoSourcePath: null,
      videoSourcePreview: null,
      videoSourceStatus: "idle",
      generating: false,
      cancelling: false,
      onModeChange: vi.fn(),
      onPromptChange: vi.fn(),
      onAspectChange: vi.fn(),
      onVideoDurationChange: vi.fn(),
      onVideoResolutionChange: vi.fn(),
      onClearVideoSource: vi.fn(),
      onUploadSource: vi.fn(),
      onGenerate: vi.fn(),
      onCancelGeneration: vi.fn(),
    },
    albumStatus: "closed",
    albumCachedCount: 0,
    albumVisibleCount: 0,
    albumErrorCode: null,
    albumSyncing: false,
    onQueryChange: vi.fn(),
    onSortChange: vi.fn(),
    onXSearchModeChange: vi.fn(),
    onXSearchModeSaveError: vi.fn(),
    onSearchX: vi.fn(),
    onCancelX: vi.fn(),
    onSearchRemote: vi.fn(),
    onCancelRemote: vi.fn(),
    onSavePexelsKey: vi.fn(async () => true),
    onRequestDeletePexelsKey: vi.fn(),
    onOpenAlbum: vi.fn(),
    onSyncAlbum: vi.fn(),
    onRefreshAlbum: vi.fn(),
    ...overrides,
  };
}

afterEach(cleanup);

describe("WallpaperSourceControls X route", () => {
  it.each([
    ["x", "settings.wallpaperSource.xPlaceholder"],
    ["web", "settings.wallpaperSource.web.placeholder"],
    ["openverse", "settings.wallpaperSource.openverse.placeholder"],
    ["pexels", "settings.wallpaperSource.pexels.placeholder"],
  ] as const)("gives the %s query an accessible name", (tab, label) => {
    render(<WallpaperSourceControls {...props({ tab })} />);

    expect(screen.getByRole("searchbox", { name: label })).toBeTruthy();
  });

  it("keeps search locked until the selected route is persisted", async () => {
    let finishSave: (() => void) | null = null;
    const onXSearchModeChange = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          finishSave = resolve;
        }),
    );
    render(<WallpaperSourceControls {...props({ onXSearchModeChange })} />);

    fireEvent.click(
      screen.getByRole("button", { name: "settings.wallpaperXSearchMode" }),
    );

    expect(onXSearchModeChange).toHaveBeenCalledWith("responses_preview");
    expect(
      (
        screen.getByRole("button", {
          name: "settings.wallpaperSource.search",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true);

    await act(async () => finishSave?.());

    await waitFor(() =>
      expect(
        (
          screen.getByRole("button", {
            name: "settings.wallpaperSource.search",
          }) as HTMLButtonElement
        ).disabled,
      ).toBe(false),
    );
  });

  it("reports a failed route save and restores the search controls", async () => {
    const onXSearchModeSaveError = vi.fn();
    render(
      <WallpaperSourceControls
        {...props({
          onXSearchModeChange: vi.fn(async () => {
            throw new Error("save failed");
          }),
          onXSearchModeSaveError,
        })}
      />,
    );

    fireEvent.click(
      screen.getByRole("button", { name: "settings.wallpaperXSearchMode" }),
    );

    await waitFor(() => expect(onXSearchModeSaveError).toHaveBeenCalledTimes(1));
    expect(
      (
        screen.getByRole("button", {
          name: "settings.wallpaperSource.search",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(false);
  });

  it("normalizes unknown route values before persisting them", async () => {
    const onXSearchModeChange = vi.fn(async (_mode: WallpaperXSearchMode) => {});
    const view = render(
      <WallpaperSourceControls
        {...props({ xSearchMode: "responses_preview", onXSearchModeChange })}
      />,
    );

    fireEvent.click(
      screen.getByRole("button", { name: "settings.wallpaperXSearchMode" }),
    );

    await waitFor(() => expect(onXSearchModeChange).toHaveBeenCalledWith("cli"));
    view.unmount();
  });
});
