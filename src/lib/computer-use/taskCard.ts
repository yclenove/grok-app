import type { ComputerStatus } from "@/lib/api/computerUse";
import type { MessageKey } from "@/i18n";

/** UI freshness only: never changes Host action deadlines or retries actions. */
export const COMPUTER_STATUS_STALE_MS = 3000;

export type ComputerTaskState =
  | "running"
  | "paused"
  | "mcp_pending"
  | "mcp_cleanup_pending"
  | "stop_requested"
  | "stopped"
  | "unknown"
  | "closed"
  | "unauthorized"
  | "unavailable";

export type ComputerTaskView = {
  targetName: string;
  backend: string;
  state: ComputerTaskState;
  failure: string | null;
};

/** Map Host status onto the task card. Never invent a running label. */
export function computerTaskView(status: ComputerStatus): ComputerTaskView | null {
  if (!status.runId || (!status.featureEnabled && status.stopState === "stopped" && !status.mcpCatalog?.cleanupPending)) return null;
  const targetName = (status.targetName || status.targetId || "").trim();
  if (!targetName) return null;
  return {
    targetName,
    backend: status.backend,
    state: computerTaskState(status),
    failure: computerTaskFailure(status),
  };
}

export function computerTaskState(status: ComputerStatus, fresh = true): ComputerTaskState {
  if (!fresh) return "unknown";
  // Physical Stop has priority over asynchronous tool-catalog cleanup.
  if (status.stopState === "stop_requested") return "stop_requested";
  if (status.mcpCatalog?.cleanupPending) return "mcp_cleanup_pending";
  if (status.mcpCatalog?.pending) return "mcp_pending";
  if (status.stopState === "stopped") return "stopped";
  if (!status.featureEnabled) return "unauthorized";
  if (!status.targetAlive) return "closed";
  if (status.paused) return "paused";
  if (status.recovery === "user") return "unauthorized";
  if (!status.enabled) return "unauthorized";
  return "running";
}

export const computerStateLabel: Record<ComputerTaskState, MessageKey> = {
  running: "cu.workspace.authorized",
  paused: "cu.task.paused",
  mcp_pending: "cu.task.mcpPending",
  mcp_cleanup_pending: "cu.task.mcpCleanupPending",
  stop_requested: "cu.panel.stopping",
  stopped: "cu.task.stopped",
  unknown: "cu.workspace.unknown",
  closed: "cu.task.windowClosed",
  unauthorized: "cu.task.unauthorized",
  unavailable: "cu.task.unavailable",
};

function computerTaskFailure(status: ComputerStatus): string | null {
  const last = status.traces.at(-1);
  if (!last) return null;
  const blob = `${last.kind} ${last.detail}`.toLowerCase();
  if (/reject|error|fail|unknown|late|unauthorized|permission/.test(blob)) {
    return last.detail || last.kind;
  }
  return null;
}
