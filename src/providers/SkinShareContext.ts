import { createContext, useContext } from "react";
import type { SkinPackErrorCode } from "@/lib/skinPack";

export type SkinShareNotice = {
  kind: "err" | "warn";
  code: SkinPackErrorCode | "unknown_skin" | "will_clear_wallpaper";
};

export type SkinShareValue = {
  appearanceBusy: boolean;
  notice: SkinShareNotice | null;
  clearNotice: () => void;
  openFilePreview: (path: string) => Promise<void>;
  openPresetPreview: (id: string, undoMode?: boolean) => Promise<void>;
  openCatalogPreview: (sourceId: string, packId: string) => Promise<void>;
  refreshPending: () => Promise<void>;
};

export const SkinShareContext = createContext<SkinShareValue | null>(null);

export function useSkinShare(): SkinShareValue {
  const value = useContext(SkinShareContext);
  if (!value) {
    throw new Error("useSkinShare requires SkinShareProvider");
  }
  return value;
}

export function useSkinShareOptional(): SkinShareValue | null {
  return useContext(SkinShareContext);
}
