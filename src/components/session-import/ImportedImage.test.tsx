/** @vitest-environment jsdom */
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, test, vi } from "vitest";
import { createT } from "@/i18n";
import { ImageViewerContext, type ImageViewerApi } from "@/components/ImageViewerContext";
import { ImportedEventList } from "./ImportedEventList";
import { ImportedImage } from "./ImportedImage";
import { SessionImportContentDetail } from "./SessionImportContentDetail";
import { emptyContentSelection } from "@/lib/sessionImport/contentSelection";

const resolveImage = vi.hoisted(() => vi.fn());
vi.mock("@/lib/api/sessionMigrationImages", () => ({ resolveImportedImage: resolveImage }));
afterEach(() => { cleanup(); vi.clearAllMocks(); });
const tr = createT("en");
const image = { state: "stored" as const, thumbnail: "http://127.0.0.1:50000/thumb", original: "http://127.0.0.1:50000/original",
  image: { mime: "image/png", width: 600, height: 400, bytes: 1000 } };
const event = { eventId: "image-message", kind: "message", role: "user", text: "wrong flattened prose", assetIds: ["photo"],
  displayParts: [{ kind: "text", text: "before" }, { kind: "asset", asset_id: "photo" }, { kind: "text", text: "after" }] };
const viewer = (): ImageViewerApi => ({ open: vi.fn(), close: vi.fn(), copyImage: vi.fn(), isOpen: () => false });

test("ordered image activation supports keyboard and releases only its own viewer", async () => {
  const api = viewer(); const retry = vi.fn();
  const view = render(<ImageViewerContext.Provider value={api}><ImportedEventList events={[event]} tr={tr}
    images={{ states: { photo: image }, retry }} /></ImageViewerContext.Provider>);
  expect([...view.container.querySelectorAll("pre,img")].map((node) => node.tagName === "IMG" ? "image" : node.textContent))
    .toEqual(["before", "image", "after"]);
  const open = screen.getByRole("button", { name: "View image" });
  open.focus(); await userEvent.keyboard("{Enter}");
  expect(api.open).toHaveBeenCalledTimes(1);
  const [slides, , owner] = vi.mocked(api.open).mock.calls[0];
  expect(typeof owner).toBe("symbol");
  expect((slides[0] as { src: string }).src).toBe(image.thumbnail);
  view.unmount();
  expect(api.close).toHaveBeenCalledWith(owner);
  expect(retry).not.toHaveBeenCalled();
});

test("previewing an unselected image never changes event or image authorization", async () => {
  resolveImage.mockResolvedValue(image);
  const api = viewer(); const onAsset = vi.fn(); const onToggleEvent = vi.fn();
  const state = { ...emptyContentSelection(), previewId: "preview", leaf: "leaf", events: [event] };
  render(<ImageViewerContext.Provider value={api}><SessionImportContentDetail tr={tr} state={state} busy={false} empty={false}
    onAsset={onAsset} onToggleEvent={onToggleEvent} onSelectPage={vi.fn()} onInvertPage={vi.fn()} onField={vi.fn()}
    onSubagents={vi.fn()} onRequestLeaf={vi.fn()} onConfirmLeaf={vi.fn()} onCancelLeaf={vi.fn()} onLoadMore={vi.fn()} /></ImageViewerContext.Provider>);
  await waitFor(() => expect(screen.getByRole("button", { name: "View image" })).toBeTruthy());
  const checkbox = screen.getByRole("checkbox", { name: "Attachments 1" }) as HTMLInputElement;
  expect(checkbox.checked).toBe(false);
  fireEvent.click(screen.getByRole("button", { name: "View image" }));
  expect(checkbox.checked).toBe(false);
  expect(onAsset).not.toHaveBeenCalled(); expect(onToggleEvent).not.toHaveBeenCalled();
  fireEvent.click(checkbox);
  expect(onAsset).toHaveBeenCalledWith("photo");
  expect(api.open).toHaveBeenCalledTimes(1);
});

test("thumbnail failures require retry and never reveal source paths or data URLs", () => {
  const retry = vi.fn();
  render(<ImportedImage assetId="data:SECRET" alt="data:image/png;base64,PRIVATE_BYTES" tr={tr} images={{ states: { "data:SECRET": image }, retry }} />);
  expect(screen.getByRole("img").getAttribute("alt")).toBe("Attachments");
  fireEvent.error(screen.getByRole("img"));
  expect(screen.getByText("Could not read this image.")).toBeTruthy();
  expect(screen.queryByRole("img")).toBeNull();
  expect(document.body.textContent).not.toContain("SECRET");
  expect(retry).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: tr("sessionImport.comparison.retryRead") }));
  expect(retry).toHaveBeenCalledWith("data:SECRET");
});

test("reusing the same image row on another branch closes the previous gallery", () => {
  const api = viewer(); const retry = vi.fn();
  const row = (identity: string) => <ImageViewerContext.Provider value={api}>
    <ImportedImage assetId="photo" tr={tr} images={{ identity, states: { photo: image }, retry }} />
  </ImageViewerContext.Provider>;
  const view = render(row("preview:first-branch"));
  fireEvent.click(screen.getByRole("button", { name: "View image" }));
  const owner = vi.mocked(api.open).mock.calls[0][2];
  view.rerender(row("preview:second-branch"));
  expect(api.close).toHaveBeenCalledWith(owner);
  expect(api.open).toHaveBeenCalledTimes(1);
  expect(retry).not.toHaveBeenCalled();
});
