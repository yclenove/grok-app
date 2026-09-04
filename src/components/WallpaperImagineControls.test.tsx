/**
 * @vitest-environment jsdom
 */
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import "@/test/jsdomStubs";
import {
  WallpaperImagineControls,
  type WallpaperImagineControlsModel,
} from "./WallpaperImagineControls";

vi.mock("@/components/Select", () => ({
  Select: ({
    value,
    options,
    disabled,
    onChange,
    "aria-label": ariaLabel,
  }: {
    value: string;
    options: Array<{ value: string }>;
    disabled?: boolean;
    onChange: (value: string) => void;
    "aria-label"?: string;
  }) => (
    <button
      type="button"
      aria-label={ariaLabel}
      disabled={disabled}
      onClick={() => {
        const next = options.find((option) => option.value !== value);
        if (next) onChange(next.value);
      }}
    >
      {value}
    </button>
  ),
}));

afterEach(cleanup);

function model(
  overrides: Partial<WallpaperImagineControlsModel> = {},
): WallpaperImagineControlsModel {
  return {
    mode: "image",
    prompt: "",
    aspect: "16:9",
    aspectOptions: [
      { value: "16:9", label: "16:9" },
      { value: "9:16", label: "9:16" },
    ],
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
    onGenerate: vi.fn(),
    onCancelGeneration: vi.fn(),
    ...overrides,
  };
}

const t = ((key: string, vars?: Record<string, unknown>) =>
  vars?.seconds ? `${key}:${vars.seconds}` : key) as never;

describe("WallpaperImagineControls", () => {
  it("switches between image and video modes", () => {
    const onModeChange = vi.fn();
    render(
      <WallpaperImagineControls
        t={t}
        locked={false}
        model={model({ onModeChange })}
      />,
    );

    expect(
      screen.getByPlaceholderText("settings.wallpaperSource.imaginePlaceholder"),
    ).toBeTruthy();
    expect(
      screen.getByRole("textbox", {
        name: "settings.wallpaperSource.imaginePlaceholder",
      }),
    ).toBeTruthy();
    fireEvent.click(
      screen.getByRole("radio", {
        name: "settings.wallpaperSource.kind.video",
      }),
    );
    expect(onModeChange).toHaveBeenCalledWith("video");
  });

  it("requires a prepared source and exposes supported video options", () => {
    const onVideoDurationChange = vi.fn();
    const onVideoResolutionChange = vi.fn();
    const view = render(
      <WallpaperImagineControls
        t={t}
        locked={false}
        model={model({
          mode: "video",
          onVideoDurationChange,
          onVideoResolutionChange,
        })}
      />,
    );

    expect(
      screen.getByText("settings.wallpaperSource.videoSourceMissing"),
    ).toBeTruthy();
    expect(
      screen.getByRole("textbox", {
        name: "settings.wallpaperSource.videoPromptPlaceholder",
      }),
    ).toBeTruthy();
    expect(
      screen.getByRole<HTMLButtonElement>("button", {
        name: "settings.wallpaperSource.generateVideo",
      }).disabled,
    ).toBe(true);

    view.rerender(
      <WallpaperImagineControls
        t={t}
        locked={false}
        model={model({
          mode: "video",
          videoSource: {
            id: "source",
            thumbUrl: "file:///wallpapers/source.jpg",
            fullUrl: "file:///wallpapers/source.jpg",
            kind: "image",
            source: "library",
            localPath: "C:\\wallpapers\\source.jpg",
          },
          videoSourcePath: "C:\\wallpapers\\source.jpg",
          videoSourceStatus: "ready",
          onVideoDurationChange,
          onVideoResolutionChange,
        })}
      />,
    );

    expect(
      screen.getByRole<HTMLButtonElement>("button", {
        name: "settings.wallpaperSource.generateVideo",
      }).disabled,
    ).toBe(false);
    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.wallpaperSource.videoDuration",
      }),
    );
    fireEvent.click(
      screen.getByRole("button", {
        name: "settings.wallpaperSource.videoResolution",
      }),
    );
    expect(onVideoDurationChange).toHaveBeenCalledWith(10);
    expect(onVideoResolutionChange).toHaveBeenCalledWith("720p");
  });

  it("turns the active generation action into cancel", () => {
    const onCancelGeneration = vi.fn();
    render(
      <WallpaperImagineControls
        t={t}
        locked
        model={model({
          mode: "video",
          generating: true,
          onCancelGeneration,
        })}
      />,
    );

    fireEvent.click(
      screen.getByRole("button", { name: "common.cancel" }),
    );
    expect(onCancelGeneration).toHaveBeenCalledTimes(1);
  });

  it("does not advertise unsupported cancellation for image generation", () => {
    const onCancelGeneration = vi.fn();
    render(
      <WallpaperImagineControls
        t={t}
        locked
        model={model({
          mode: "image",
          generating: true,
          onCancelGeneration,
        })}
      />,
    );

    const action = screen.getByRole<HTMLButtonElement>("button", {
      name: "settings.wallpaperSource.generating",
    });
    expect(action.disabled).toBe(true);
    fireEvent.click(action);
    expect(onCancelGeneration).not.toHaveBeenCalled();
  });
});
