import { invoke } from "@tauri-apps/api/core";
import { ensureMediaEndpoint } from "@/lib/imageSrc";

export type ImportedImageScope =
  | { kind: "preview"; previewId: string }
  | { kind: "published"; importId: string; revisionId: string };
export type ImportedImageState = "stored" | "missing" | "unresolved" | "excluded" | "oversize" | "unsupported" | "invalid";
export type ImportedImageResolution = {
  state: ImportedImageState;
  image?: { mime: string; bytes: number; width: number; height: number };
};
export type ImportedImageView = ImportedImageResolution & { thumbnail?: string; original?: string };

/** Only identity crosses IPC. The HTTP route revalidates the same scope/asset. */
export async function resolveImportedImage(scope: ImportedImageScope, assetId: string): Promise<ImportedImageView> {
  const result = await invoke<ImportedImageResolution>("session_import_image_resolve", { scope, assetId });
  if (result.state !== "stored") return result;
  const endpoint = await ensureMediaEndpoint();
  if (!endpoint || !result.image) throw new Error("import_image_unavailable");
  const query = new URLSearchParams({ t: endpoint.token, s: JSON.stringify(scope), a: assetId });
  const base = `${endpoint.baseUrl.replace(/\/$/, "")}/v1/import-image?${query}`;
  return { ...result, thumbnail: `${base}&variant=thumbnail`, original: `${base}&variant=original` };
}
