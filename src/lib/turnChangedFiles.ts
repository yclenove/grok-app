/**
 * Per-turn changed-file strip helpers (Issue #998).
 * Derive unique edit paths from an assistant timeline — no new diff engine.
 */

import {
  isEditToolKind,
  normalizePath,
  pathBaseName,
} from "./sessionChanges";
import type { MessageToolSegment } from "./session";
import type { TimelineUnit } from "./timelinePhases";

export interface TurnChangedFileItem {
  /** Full path passed to Changes open handler. */
  path: string;
  /** Basename for the chip label. */
  name: string;
}

/** Max file chips before collapsing the rest behind “+N more”. */
export const TURN_CHANGED_FILES_VISIBLE_MAX = 6;

function visitEditPath(
  tool: MessageToolSegment,
  pathSet: Set<string>,
): void {
  const raw = (tool.path || "").trim();
  if (!raw) return;
  if (!isEditToolKind(tool.toolKind)) return;
  const n = normalizePath(raw);
  if (n) pathSet.add(n);
}

/**
 * Unique edit/write paths touched in this assistant timeline (stream order).
 * Empty when the turn did not mutate files.
 */
export function collectTurnModifiedPaths(
  units: TimelineUnit[],
): string[] {
  const pathSet = new Set<string>();
  for (const u of units) {
    if (u.kind === "phase") {
      for (const t of u.tools) visitEditPath(t, pathSet);
    } else if (u.kind === "tool") {
      visitEditPath(u.tool, pathSet);
    }
  }
  return Array.from(pathSet);
}

/** Map paths → chip items (basename label). Drops empty basenames. */
export function turnChangedFileItems(
  paths: string[],
): TurnChangedFileItem[] {
  const out: TurnChangedFileItem[] = [];
  for (const path of paths) {
    const name = pathBaseName(path) || path.trim();
    if (!name) continue;
    out.push({ path, name });
  }
  return out;
}

/**
 * Split items into the visible chip row and a leftover count for “+N more”.
 */
export function splitTurnChangedFiles(
  items: TurnChangedFileItem[],
  maxVisible = TURN_CHANGED_FILES_VISIBLE_MAX,
): { visible: TurnChangedFileItem[]; hiddenCount: number } {
  if (maxVisible < 1 || items.length <= maxVisible) {
    return { visible: items, hiddenCount: 0 };
  }
  return {
    visible: items.slice(0, maxVisible),
    hiddenCount: items.length - maxVisible,
  };
}
