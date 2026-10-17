/** Desktop-only redacted screenshot for one Computer Use tool card. */

import type { AdmittedShot, ShotFault } from "@/lib/computer-use/events";
import { invoke, isDesktopHost } from "./host";

type ShotBody = { thumbPath?: string | null; fullPath?: string | null };

export async function loadComputerUseShot(
  sessionId: string,
  id: string,
): Promise<AdmittedShot | ShotFault> {
  if (!isDesktopHost()) return "missing";
  try {
    const body = await invoke<ShotBody>("cu_tool_shot", {
      body: { sessionId, id },
    });
    return {
      sessionId,
      id,
      thumbPath: body?.thumbPath ?? null,
      fullPath: body?.fullPath ?? null,
      source: "redacted",
    };
  } catch (error) {
    return faultOf(error);
  }
}

function faultOf(error: unknown): ShotFault {
  const text = errorText(error);
  if (text === "foreign") return "foreign";
  if (text === "rejected") return "rejected";
  return "missing";
}

function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "";
}
