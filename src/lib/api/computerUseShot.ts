/** Desktop-only redacted screenshot for one Computer Use tool card. */

import { sharedShotBook, type AdmittedShot, type ShotFault } from "@/lib/computer-use/events";
import { invoke, isDesktopHost, listen } from "./host";

type ShotBody = { thumbPath?: string | null; fullPath?: string | null };
const closedHandlers = new Set<(sessionId: string) => void>();
let closedListener: Promise<() => void> | null = null;

export async function listenComputerUseSessionClosed(
  handler: (sessionId: string) => void,
): Promise<() => void> {
  if (!isDesktopHost()) return () => {};
  closedHandlers.add(handler);
  // Keep the shared ledger subscribed when virtualized/background cards unmount.
  closedListener ??= listen<{ sessionId: string }>("cu://session-closed", (event) => {
    if (typeof event.sessionId !== "string") return;
    sharedShotBook.dropSession(event.sessionId);
    for (const callback of closedHandlers) callback(event.sessionId);
  }).catch((error: unknown) => {
    closedListener = null;
    throw error;
  });
  try {
    await closedListener;
  } catch (error) {
    closedHandlers.delete(handler);
    throw error;
  }
  return () => { closedHandlers.delete(handler); };
}

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
