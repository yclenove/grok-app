/** Shared task transactions. Local receipts never manufacture Host authority. */
import {
  computerTaskDecide, computerTaskPause, computerTaskResume, type ComputerTaskKey,
  type ComputerTaskExecutionAddress, type ComputerTaskExecution,
} from "@/lib/api/computerTask";
import {
  computerTaskPickerBegin, computerTaskPickerLoad, computerTaskPickerCancel, computerTaskPickerApprove,
  computerTaskPickerPreparation, computerTaskPickerPairingConfirm,
  type ComputerTaskPickerStarted, type ComputerTaskPickerView, type ComputerTaskPickerPreparation,
} from "@/lib/api/computerTaskPicker";
import { EMPTY_TASK_SNAPSHOT, type ComputerTaskSnapshot } from "./taskSource";
import { computerTaskManagedApprove } from "@/lib/api/computerTaskManaged";
import { TaskExistingPreparation } from "./taskExistingPreparation";
import { validatePickerStarted, validatePickerView } from "./taskPickerValidation";
import { computerUseErrorKey } from "./errors";

type Dependencies = {
  decide: typeof computerTaskDecide; pause: typeof computerTaskPause; resume: typeof computerTaskResume;
  begin: typeof computerTaskPickerBegin; load: typeof computerTaskPickerLoad;
  cancel: typeof computerTaskPickerCancel; approve: typeof computerTaskPickerApprove;
  managedApprove: typeof computerTaskManagedApprove;
  preparation: typeof computerTaskPickerPreparation; confirm: typeof computerTaskPickerPairingConfirm;
};
type Pending = "choose" | "approve" | "deny" | "cancel" | "pause" | "resume" | "stop";
export type ComputerTaskActionState = Readonly<{
  pending: Pending | null;
  picker: ComputerTaskPickerView | ComputerTaskPickerStarted | null;
  pickerOwner: string | null;
  choosing: boolean;
  selected: readonly string[];
  preparation: ComputerTaskPickerPreparation | null;
  preparationBusy: boolean;
  preparationConfirming: boolean;
  error: "decision" | "picker" | "preparation" | "managed" | "managed-macos" | "managed-os-version" | "pause" | "resume" | null;
  awaitingHost: boolean;
}>;
export const EMPTY_TASK_ACTION_STATE: ComputerTaskActionState = Object.freeze({
  pending: null, picker: null, pickerOwner: null, choosing: false, selected: [], preparation: null, preparationBusy: false, preparationConfirming: false, error: null, awaitingHost: false,
});

export function sameComputerTaskKey(a: ComputerTaskKey, b: ComputerTaskKey) {
  return a.connectionId === b.connectionId && a.taskId === b.taskId
    && a.requestId === b.requestId && a.revision === b.revision;
}

export function canResumeTaskExecution(host: ComputerTaskSnapshot, run: ComputerTaskExecution) {
  return host.state === "live" && !!host.view?.featureEnabled && run.resumeAvailable === true
    && run.phase === "paused" && host.view.grants.some((grant) => grant.runId === run.runId
      && sameComputerTaskKey(grant.key, run.key) && grant.suspended && !grant.executable);
}

function taskIdentity(host: ComputerTaskSnapshot) {
  const view = host.view;
  return view ? JSON.stringify([view.sessionId, host.owner?.hostInstanceId, view.connectionId,
    view.taskId ?? view.request?.key.taskId ?? view.executions[0]?.key.taskId]) : null;
}
function requestIdentity(host: ComputerTaskSnapshot) {
  return host.view?.request ? JSON.stringify(host.view.request.key) : null;
}

/** One controller is shared by the card and panel; tests inject synthetic APIs. */
export class ComputerTaskActions {
  private host = EMPTY_TASK_SNAPSHOT;
  private state = EMPTY_TASK_ACTION_STATE;
  private listeners = new Set<() => void>();
  private epoch = 0;
  private pauseAddress: ComputerTaskExecutionAddress | null = null;
  private resumeAddress: ComputerTaskExecutionAddress | null = null;
  private sawResuming = false;
  private existing: TaskExistingPreparation | null = null;

  constructor(readonly sessionId: string, private readonly deps: Dependencies = {
    decide: computerTaskDecide, pause: computerTaskPause, resume: computerTaskResume, begin: computerTaskPickerBegin,
    load: computerTaskPickerLoad, cancel: computerTaskPickerCancel, approve: computerTaskPickerApprove,
    managedApprove: computerTaskManagedApprove,
    preparation: computerTaskPickerPreparation, confirm: computerTaskPickerPairingConfirm,
  }) {}

  getSnapshot = () => this.state;
  subscribe = (listener: () => void) => {
    const notify = () => listener();
    this.listeners.add(notify);
    let active = true;
    return () => {
      if (!active) return;
      active = false;
      this.listeners.delete(notify);
      if (!this.listeners.size) this.dispose();
    };
  };
  hasSubscribers = () => this.listeners.size > 0;

  private publish(patch: Partial<ComputerTaskActionState>) {
    this.state = Object.freeze({ ...this.state, ...patch });
    for (const listener of this.listeners) listener();
  }
  private cancelPicker(picker = this.state.picker) {
    if (picker) void this.deps.cancel(picker).catch(() => {});
  }
  private reset() {
    this.epoch++;
    this.existing?.dispose(); this.existing = null;
    this.cancelPicker();
    this.pauseAddress = null;
    this.resumeAddress = null;
    this.sawResuming = false;
    this.state = EMPTY_TASK_ACTION_STATE;
    for (const listener of this.listeners) listener();
  }
  dispose = () => { this.reset(); this.host = EMPTY_TASK_SNAPSHOT; };

  /** Facts come from the shared Host source. A new request/owner fences late UI
   * replies; a Stop remains task-wide across an incremental scope request. */
  update = (host: ComputerTaskSnapshot) => {
    if (host.view && host.view.sessionId !== this.sessionId) return;
    const changed = taskIdentity(host) !== taskIdentity(this.host);
    const revised = requestIdentity(host) !== requestIdentity(this.host);
    this.host = host;
    if (changed || (revised && this.state.pending !== "stop")) { this.reset(); return; }
    if (host.state !== "live") {
      if (this.state.pending || this.state.choosing || this.state.awaitingHost) this.reset();
      return;
    }
    const request = host.view?.request;
    const executions = host.view?.executions ?? [];
    const requestEnded = !!request && ["denied", "cancelled", "expired", "completed", "retired"].includes(request.phase);
    const taskEnded = !!request && ["cancelled", "denied", "expired", "completed", "retired"].includes(request.phase)
      && !host.view?.grants.some((grant) => grant.executable)
      && executions.every((run) => run.phase === "stopped");
    const stopped = executions.length > 0 && executions.every((run) => run.phase === "stopped");
    const paused = this.pauseAddress && executions.some((run) => run.runId === this.pauseAddress?.runId
      && sameComputerTaskKey(run.key, this.pauseAddress.key) && run.generation !== this.pauseAddress.generation
      && run.phase === "paused");
    const recovering = this.resumeAddress && executions.find((run) => run.runId === this.resumeAddress?.runId
      && sameComputerTaskKey(run.key, this.resumeAddress.key));
    if (recovering?.phase === "resuming") this.sawResuming = true;
    if (this.state.pending === "resume" && recovering && (!host.view?.featureEnabled
      || ["stop_requested", "stopped", "target_closed"].includes(recovering.phase))) {
      this.reset();
      return;
    }
    const resumed = recovering && recovering.generation !== this.resumeAddress?.generation
      && recovering.hasFreshObservation && ["idle", "busy", "awaiting_observation"].includes(recovering.phase)
      && host.view?.featureEnabled && host.view.grants.some((grant) => grant.runId === recovering.runId
        && sameComputerTaskKey(grant.key, recovering.key) && grant.executable && !grant.suspended);
    if (this.state.pending === "resume" && recovering?.phase === "paused"
      && (this.sawResuming || recovering.generation !== this.resumeAddress?.generation)) {
      this.reset();
      this.publish({ error: "resume" });
      return;
    }
    if ((requestEnded && this.state.pending !== "stop" && (this.state.pending || this.state.choosing))
      || (taskEnded && this.state.pending === "stop")
      || (this.state.choosing && request?.phase === "approved")
      || (this.state.pending === "stop" && stopped)
      || (this.state.pending === "pause" && paused)
      || (this.state.pending === "resume" && resumed)
      || (this.state.pending === "approve" && request?.phase === "approved")
      || (this.state.error === "managed" && request?.phase === "approved")
      || (["deny", "cancel"].includes(this.state.pending ?? "") && taskEnded)) this.reset();
  };

  closePicker = () => {
    if (!this.state.choosing && !this.state.picker) return;
    this.reset();
  };

  setPickerVisible = (owner: string, visible: boolean) => {
    if (owner === this.state.pickerOwner) this.existing?.setVisible(visible);
  };
  refreshPreparation = () => this.existing?.refresh();
  confirmPairing = () => this.existing?.confirm();

  choose = async (surface: string, owner: string | null = null) => {
    const request = this.host.view?.request;
    if (this.host.state !== "live" || request?.phase !== "pending" || this.state.pending || this.state.choosing
      || !request.surfaces.includes(surface) || !this.host.view?.pickerSurfaces.includes(surface)) return;
    const key = request.key;
    const epoch = ++this.epoch;
    if (surface === "managed-browser") {
      this.publish({ pending: "approve", error: null, awaitingHost: false });
      try {
        const receipt = await this.deps.managedApprove(this.sessionId, key);
        if (epoch !== this.epoch) return;
        if (receipt.requestId !== key.requestId || receipt.revision !== key.revision || !receipt.runId) throw new Error("approval changed");
        // The receipt is not a grant. Only the original Host stream can settle
        // approval; hiding this UI does not revoke an explicitly approved task.
        this.publish({ awaitingHost: true });
      } catch (error) {
        const key = computerUseErrorKey(error instanceof Error ? error.message : String(error));
        const state = key === "cu.consent.managedMacosMinimum" ? "managed-macos"
          : key === "cu.consent.managedOsVersionUnavailable" ? "managed-os-version" : "managed";
        if (epoch === this.epoch) this.publish({ pending: null, error: state, awaitingHost: false });
      }
      return;
    }
    if (surface !== "desktop" && surface !== "existing-tabs" && surface !== "app-webview") return;
    this.publish({ pending: "choose", choosing: true, pickerOwner: owner, error: null, selected: [], awaitingHost: false });
    try {
      const picker = await this.deps.begin(this.sessionId, key, surface);
      validatePickerStarted(picker, this.sessionId, key, surface);
      if (epoch !== this.epoch) { this.cancelPicker(picker); return; }
      this.publish({ picker });
      if (surface === "existing-tabs") {
        this.existing = new TaskExistingPreparation(picker, this.deps,
          () => epoch === this.epoch && this.state.choosing, patch => {
            const { failed, ...state } = patch;
            this.publish({ ...state, pending: null, ...(failed !== undefined ? { error: failed ? "preparation" : null } : {}) });
          });
        await this.existing.refresh();
        return;
      }
      const view = await this.deps.load(picker);
      if (epoch !== this.epoch) return;
      validatePickerView(view, picker);
      this.publish({ pending: null, picker: view });
    } catch {
      if (epoch === this.epoch) {
        this.cancelPicker();
        this.publish({ pending: null, picker: null, error: "picker" });
      }
    }
  };

  toggleCandidate = (id: string) => {
    const picker = this.state.picker;
    if (!picker || !("candidates" in picker) || this.state.pending || this.state.preparationBusy
      || !picker.candidates.some((row) => row.candidateId === id)) return;
    const selected = this.state.selected.includes(id) ? this.state.selected.filter((value) => value !== id)
      : this.state.selected.length < picker.maxSelection ? [...this.state.selected, id] : this.state.selected;
    if (selected !== this.state.selected) this.publish({ selected });
  };

  approve = async () => {
    const picker = this.state.picker;
    const request = this.host.view?.request;
    if (this.host.state !== "live" || !picker || !("candidates" in picker)
      || !request || !["pending", "preparing"].includes(request.phase) || !sameComputerTaskKey(request.key, picker.key)
      || this.state.pending || this.state.preparationBusy || !this.state.selected.length) return;
    const epoch = ++this.epoch;
    this.existing?.dispose(); this.existing = null;
    const ids = [...this.state.selected];
    this.publish({ pending: "approve", error: null });
    try {
      const receipt = await this.deps.approve(picker, ids);
      if (epoch !== this.epoch) return;
      if (receipt.requestId !== picker.key.requestId || receipt.revision !== picker.key.revision || !receipt.runId) throw new Error("approval changed");
      // Wait for authoritative events instead of adopting receipt.runId.
      this.publish({ awaitingHost: true });
    } catch {
      if (epoch === this.epoch) this.publish({ pending: null, error: "decision" });
    }
  };

  decide = async (decision: "deny" | "cancel" | "stop") => {
    const view = this.host.view;
    const key = view?.request?.key ?? view?.executions[0]?.key ?? view?.grants[0]?.key;
    if (!key || this.state.pending === "stop" || this.state.pending === decision
      || (decision !== "stop" && this.host.state !== "live")) return;
    const epoch = ++this.epoch;
    this.existing?.dispose(); this.existing = null;
    this.cancelPicker();
    this.publish({ pending: decision, picker: null, pickerOwner: null, choosing: false, selected: [], preparation: null, preparationBusy: false, error: null, awaitingHost: false });
    try {
      const receipt = await this.deps.decide(this.sessionId, key, decision);
      if (epoch !== this.epoch) return;
      if (receipt.sessionId !== this.sessionId || !sameComputerTaskKey(receipt.key, key) || receipt.decision !== decision) throw new Error("decision changed");
      this.publish({ awaitingHost: true });
    } catch {
      if (epoch === this.epoch) this.publish({ pending: null, awaitingHost: false, error: "decision" });
    }
  };

  pause = async (address: ComputerTaskExecutionAddress) => {
    if (this.host.state !== "live" || this.state.pending || !this.host.view?.featureEnabled
      || !this.host.view.grants.some((grant) => grant.runId === address.runId && sameComputerTaskKey(grant.key, address.key) && grant.executable)
      || !this.host.view.executions.some((run) =>
      run.runId === address.runId && run.generation === address.generation && sameComputerTaskKey(run.key, address.key)
      && ["idle", "busy", "awaiting_observation"].includes(run.phase))) return;
    const epoch = ++this.epoch;
    const exact = { key: address.key, runId: address.runId, generation: address.generation };
    this.pauseAddress = exact;
    this.publish({ pending: "pause", error: null });
    try {
      await this.deps.pause(this.sessionId, exact);
      if (epoch === this.epoch) this.publish({ awaitingHost: true });
    } catch {
      if (epoch === this.epoch) this.publish({ pending: null, awaitingHost: false, error: "pause" });
    }
  };

  resume = async (address: ComputerTaskExecutionAddress) => {
    const run = this.host.view?.executions.find((execution) => execution.runId === address.runId
      && execution.generation === address.generation && sameComputerTaskKey(execution.key, address.key));
    if (this.state.pending || !run || !canResumeTaskExecution(this.host, run)) return;
    const epoch = ++this.epoch;
    const exact = { key: { ...address.key }, runId: address.runId, generation: address.generation };
    this.resumeAddress = exact;
    this.sawResuming = false;
    this.publish({ pending: "resume", error: null, awaitingHost: false });
    try {
      const receipt = await this.deps.resume(this.sessionId, exact);
      if (epoch !== this.epoch) return;
      if (receipt.phase === "resumed") {
        const next = receipt.address;
        if (next.runId !== exact.runId || !sameComputerTaskKey(next.key, exact.key)
          || !/^(0|[1-9]\d*)$/.test(next.generation) || BigInt(next.generation) <= BigInt(exact.generation)
          || BigInt(next.generation) > 18446744073709551615n) throw new Error("resume identity changed");
      } else if (receipt.phase !== "in_progress") throw new Error("resume reply changed");
      // A receipt is not a grant. The shared Host stream settles the operation.
      this.publish({ awaitingHost: true });
    } catch {
      if (epoch === this.epoch) this.publish({ pending: null, awaitingHost: false, error: "resume" });
    }
  };
}
