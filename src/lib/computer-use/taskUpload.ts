/** One original native picker call shared by every view of this App session. */
import {
  computerTaskChooseUpload, computerTaskCancelUpload,
  type ComputerTaskExecution, type ComputerTaskExecutionAddress, type ComputerTaskUploadCompletion,
} from "@/lib/api/computerTask";
import { sameComputerTaskKey } from "./taskActions";
import { EMPTY_TASK_SNAPSHOT, type ComputerTaskSnapshot } from "./taskSource";

type Dependencies = { choose: typeof computerTaskChooseUpload; cancel: typeof computerTaskCancelUpload };
type Choice = { address: ComputerTaskExecutionAddress; requestId: string; actionId: string; hostId: string | undefined };
export type TaskUploadState = Readonly<{
  choice: Choice | null;
  pending: boolean;
  cancelling: boolean;
  result: "assigned" | "cancelled" | "unknown" | "failed" | null;
}>;
export const EMPTY_UPLOAD_STATE: TaskUploadState = Object.freeze({ choice: null, pending: false, cancelling: false, result: null });

function sameExecution(run: ComputerTaskExecution, address: ComputerTaskExecutionAddress) {
  return run.runId === address.runId && run.generation === address.generation && sameComputerTaskKey(run.key, address.key);
}

export class ComputerTaskUploadChoice {
  private host = EMPTY_TASK_SNAPSHOT;
  private state = EMPTY_UPLOAD_STATE;
  private listeners = new Set<() => void>();
  // A stale stream or unmounted renderer cannot release the original dialog.
  private nativeCall: Choice | null = null;
  private cancelCalls = new Set<Choice>();
  constructor(readonly sessionId: string, private readonly deps: Dependencies = {
    choose: computerTaskChooseUpload, cancel: computerTaskCancelUpload,
  }) {}
  getSnapshot = () => this.state;
  hasPendingCall = () => !!this.nativeCall || !!this.cancelCalls.size;
  hasSubscribers = () => !!this.listeners.size;
  subscribe = (listener: () => void) => {
    const notify = () => listener();
    this.listeners.add(notify);
    return () => { this.listeners.delete(notify); };
  };
  private publish(patch: Partial<TaskUploadState>) {
    this.state = Object.freeze({ ...this.state, ...patch });
    for (const listener of this.listeners) listener();
  }
  private current(choice: Choice) {
    return this.host.state === "live" && this.host.view?.featureEnabled
      && choice.hostId === this.host.owner?.hostInstanceId
      && !!this.host.view.executions.some(run => sameExecution(run, choice.address)
        && ["idle", "busy", "awaiting_observation"].includes(run.phase)
        && (!run.upload || run.upload.requestId === choice.requestId));
  }
  update = (host: ComputerTaskSnapshot) => {
    if (host.view && host.view.sessionId !== this.sessionId) return;
    this.host = host;
    const choice = this.state.choice;
    if (!choice) return;
    if (!this.current(choice)) {
      this.publish({ choice: null, result: null });
      // The authoritative Broker also fences retirement; this explicit Host
      // cancellation only addresses the original request and never a new run.
      if (this.nativeCall === choice) void this.cancelChoice(choice);
    }
  };
  choose = async (run: ComputerTaskExecution, dialogTitle: string) => {
    const upload = run.upload;
    if (!upload || upload.phase !== "awaiting_file" || this.hasPendingCall()) return;
    const choice: Choice = {
      address: { key: { ...run.key }, runId: run.runId, generation: run.generation },
      requestId: upload.requestId, actionId: upload.actionId, hostId: this.host.owner?.hostInstanceId,
    };
    if (!this.current(choice) || !this.host.view?.executions.some(row => sameExecution(row, run)
      && row.upload?.requestId === choice.requestId && row.upload.actionId === choice.actionId && row.upload.phase === "awaiting_file")
      || !this.host.view.grants.some(grant => grant.runId === run.runId && sameComputerTaskKey(grant.key, run.key)
        && grant.executable && !grant.suspended)) return;
    this.nativeCall = choice;
    this.publish({ choice, pending: true, result: null });
    try {
      const completion = await this.deps.choose(this.sessionId, choice.address, choice.requestId, dialogTitle);
      if (this.state.choice === choice && this.current(choice)) this.publish({ result: this.result(choice, completion) });
    } catch {
      if (this.state.choice === choice && this.current(choice)) this.publish({ result: "failed" });
    } finally {
      if (this.nativeCall === choice) this.nativeCall = null;
      this.publish({ pending: false });
    }
  };
  private result(choice: Choice, completion: ComputerTaskUploadCompletion): TaskUploadState["result"] {
    const outcome = completion.outcome;
    if (outcome.runId !== choice.address.runId || outcome.actionId !== choice.actionId) return "failed";
    if (outcome.kind === "verified" && outcome.executed && Number.isSafeInteger(completion.bytes)
      && completion.bytes !== null && completion.bytes >= 0 && completion.bytes <= 8 * 1024 * 1024) return "assigned";
    if (outcome.kind === "unknown") return "unknown";
    return outcome.kind === "rejected" && outcome.reason === "user cancelled file selection" ? "cancelled" : "failed";
  }
  cancel = async (run: ComputerTaskExecution) => {
    if (!run.upload || !["awaiting_file", "selecting"].includes(run.upload.phase)) return;
    const choice = this.state.choice ?? {
      address: { key: { ...run.key }, runId: run.runId, generation: run.generation },
      requestId: run.upload.requestId, actionId: run.upload.actionId, hostId: this.host.owner?.hostInstanceId,
    };
    if (!this.current(choice) || !sameExecution(run, choice.address) || run.upload.requestId !== choice.requestId
      || !this.host.view?.executions.some(row => sameExecution(row, run) && row.upload?.requestId === choice.requestId
        && ["awaiting_file", "selecting"].includes(row.upload.phase))
      || this.cancelCalls.size || (this.nativeCall && this.nativeCall !== choice)) return;
    if (this.state.choice !== choice) this.publish({ choice, result: null });
    await this.cancelChoice(choice);
  };
  private cancelChoice = async (choice: Choice) => {
    if (this.cancelCalls.has(choice)) return;
    this.cancelCalls.add(choice);
    this.publish({ cancelling: true });
    try {
      const completion = await this.deps.cancel(this.sessionId, choice.address, choice.requestId);
      if (this.state.choice === choice && this.current(choice)) this.publish({ result: this.result(choice, completion) });
    } catch {
      if (this.state.choice === choice && this.current(choice)) this.publish({ result: "failed" });
    } finally {
      this.cancelCalls.delete(choice);
      this.publish({ cancelling: !!this.cancelCalls.size });
    }
  };
  release = () => {
    const choice = this.nativeCall;
    this.publish({ choice: null, result: null });
    if (choice) void this.cancelChoice(choice);
  };
}
