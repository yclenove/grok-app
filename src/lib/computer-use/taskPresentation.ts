import type { MessageKey } from "@/i18n";
import type { ComputerTaskExecution } from "@/lib/api/computerTask";
import { canResumeTaskExecution, sameComputerTaskKey, type ComputerTaskActionState } from "./taskActions";
import type { ComputerTaskSnapshot } from "./taskSource";

export function taskCanPause(host: ComputerTaskSnapshot, run: ComputerTaskExecution) {
  return host.state === "live" && !!host.view?.featureEnabled
    && ["idle", "busy", "awaiting_observation"].includes(run.phase)
    && host.view.grants.some((grant) => grant.executable && grant.runId === run.runId && sameComputerTaskKey(grant.key, run.key));
}

export function taskIsStopped(host: ComputerTaskSnapshot) {
  return host.state === "live" && !!host.view?.executions.length
    && host.view.executions.every((run) => run.phase === "stopped" && !run.cleanupPending);
}

export const taskCanResume = canResumeTaskExecution;

export function taskActionErrorKey(error: ComputerTaskActionState["error"]): MessageKey {
  switch (error) {
    case "managed-macos": return "cu.consent.managedMacosMinimum";
    case "managed-os-version": return "cu.consent.managedOsVersionUnavailable";
    case "managed": return "cu.consent.managedFailed";
    case "picker": return "cu.consent.pickerFailed";
    case "resume": return "cu.consent.resumeFailed";
    default: return "cu.panel.error";
  }
}

export function taskIsCompleted(host: ComputerTaskSnapshot) {
  return host.state === "live" && host.view?.request?.phase === "completed"
    && !host.view.grants.some((grant) => grant.executable)
    && host.view.executions.every((run) => run.phase === "stopped" && !run.cleanupPending);
}

export function taskStatusKey(host: ComputerTaskSnapshot, state: ComputerTaskActionState): MessageKey {
  if (host.state !== "live") return "cu.workspace.unknown";
  const view = host.view;
  if (!view) return "cu.task.unavailable";
  if (state.pending === "stop" || view.executions.some((run) => run.phase === "stop_requested")) return "cu.panel.stopping";
  if (taskIsCompleted(host)) return "cu.consent.completed";
  if (view.request?.phase === "completed" && !view.grants.some((grant) => grant.executable)) return "cu.panel.stopping";
  const phases = view.executions.map((run) => run.phase);
  if (phases.length && phases.every((phase) => phase === "stopped")) {
    return view.executions.some((run) => run.cleanupPending) ? "cu.panel.stopping" : "cu.task.stopped";
  }
  if (phases.includes("unknown")) return "cu.workspace.unknown";
  if (phases.includes("target_closed")) return "cu.task.windowClosed";
  if (phases.includes("resuming") || state.pending === "resume") return "cu.consent.resuming";
  if (phases.includes("paused")) {
    return view.executions.some((run) => run.phase === "paused" && run.cleanupPending)
      ? "cu.consent.pausedCleanup" : "cu.task.paused";
  }
  if (view.request?.phase === "preparing" || phases.includes("preparing") || state.pending === "approve") return "cu.consent.preparing";
  if (view.request?.phase === "pending") return "cu.consent.pending";
  if (!view.featureEnabled && phases.length) return "cu.task.unauthorized";
  if (phases.includes("busy")) return "cu.panel.busy";
  if (phases.includes("switching")) return "cu.consent.preparing";
  if (phases.includes("awaiting_observation") && view.executions.some((run) => taskCanPause(host, run))) return "cu.workspace.authorized";
  if (phases.includes("idle") && view.executions.some((run) => taskCanPause(host, run))) return "cu.workspace.authorized";
  const terminal: Partial<Record<NonNullable<typeof view.request>["phase"], MessageKey>> = {
    approved: "cu.consent.allowed", denied: "cu.consent.denied", cancelled: "cu.consent.cancelled",
    expired: "cu.consent.expired", retired: "cu.consent.retired", completed: "cu.consent.completed",
  };
  return view.request ? terminal[view.request.phase] ?? "cu.task.unauthorized" : "cu.consent.idle";
}

export function taskDataState(key: MessageKey) {
  if (key === "cu.workspace.authorized" || key === "cu.panel.busy") return "running";
  if (key === "cu.consent.completed") return "completed";
  if (key === "cu.workspace.unknown") return "unknown";
  if (key === "cu.task.paused" || key === "cu.consent.pausedCleanup") return "paused";
  if (key === "cu.consent.resuming") return "resuming";
  if (key === "cu.consent.preparing") return "authorizing";
  if (key === "cu.panel.stopping") return "stop_requested";
  if (key === "cu.task.stopped") return "stopped";
  if (key === "cu.task.windowClosed") return "closed";
  return "pending";
}
