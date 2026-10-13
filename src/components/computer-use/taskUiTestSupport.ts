import { act } from "@testing-library/react";
import { vi } from "vitest";
import type { ComputerTaskExecution, ComputerTaskKey, ComputerTaskView } from "@/lib/api/computerTask";
import type { ComputerTaskEvent, ComputerTaskSubscription } from "@/lib/api/computerTaskEvents";
import * as events from "@/lib/api/computerTaskEvents";
import * as taskApi from "@/lib/api/computerTask";
import * as pickerApi from "@/lib/api/computerTaskPicker";
import * as managedApi from "@/lib/api/computerTaskManaged";

vi.mock("@/lib/api/host", async (original) => ({ ...await original<typeof import("@/lib/api/host")>(), isDesktopHost: () => true }));
vi.mock("@/lib/api/computerTaskEvents", () => ({ computerTaskSubscribe: vi.fn(), computerTaskUnsubscribe: vi.fn() }));
vi.mock("@/lib/api/computerTask", () => ({ computerTaskDecide: vi.fn(), computerTaskPause: vi.fn(), computerTaskResume: vi.fn(), computerTaskPreview: vi.fn(), computerTaskChooseUpload: vi.fn(), computerTaskCancelUpload: vi.fn() }));
vi.mock("@/lib/api/computerTaskPicker", () => ({ computerTaskPickerBegin: vi.fn(), computerTaskPickerLoad: vi.fn(), computerTaskPickerCancel: vi.fn(), computerTaskPickerApprove: vi.fn(), computerTaskPickerPreparation: vi.fn(), computerTaskPickerPairingConfirm: vi.fn() }));
vi.mock("@/lib/api/computerTaskManaged", () => ({ computerTaskManagedApprove: vi.fn() }));

export const key: ComputerTaskKey = { connectionId: "synthetic-connection", taskId: "synthetic-task", requestId: "synthetic-request", revision: 1 };
export const address = { key, runId: "synthetic-run", generation: "18446744073709551614" };
export const pendingView = (sessionId = "chat"): ComputerTaskView => ({
  sessionId, connectionId: key.connectionId, taskId: key.taskId, featureEnabled: false,
  request: { key, phase: "pending", purpose: "Arrange my owned editor", scopeIntent: "Only selected fixture windows", surfaces: ["desktop"], capabilities: ["observe", "input"], remainingMs: 60_000 },
  grants: [], executions: [], pickerSurfaces: ["desktop"],
});
export const execution = (patch: Partial<ComputerTaskExecution> = {}): ComputerTaskExecution => ({
  ...address, phase: "idle", target: { targetId: "fixture-window", surface: "desktop" },
  targetName: "Owned editor", backend: "synthetic", cleanupPending: false, resumeAvailable: false, hasFreshObservation: true, lastFailure: null, ...patch,
});
export const runningView = (sessionId = "chat"): ComputerTaskView => ({
  ...pendingView(sessionId), featureEnabled: true, request: { ...pendingView(sessionId).request!, phase: "approved" },
  executions: [execution()], grants: [{ key, runId: address.runId, members: [execution().target!], capabilities: ["observe", "input"], suspended: false, executable: true }],
});
export function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

type Stream = { owner: ComputerTaskSubscription; callback: (event: ComputerTaskEvent) => void; sequence: number };
export function setupTaskUi() {
  vi.resetAllMocks();
  const streams: Stream[] = [];
  vi.mocked(events.computerTaskSubscribe).mockImplementation(async (sessionId, callback) => {
    const owner = { sessionId, connectionId: key.connectionId, subscriptionId: `synthetic-${streams.length}`, hostInstanceId: "synthetic-host" };
    streams.push({ owner, callback, sequence: 0 });
    return owner;
  });
  vi.mocked(events.computerTaskUnsubscribe).mockResolvedValue(true);
  vi.mocked(taskApi.computerTaskDecide).mockImplementation(async (sessionId, key, decision) => ({ sessionId, key, decision }));
  vi.mocked(taskApi.computerTaskPause).mockResolvedValue();
  vi.mocked(taskApi.computerTaskResume).mockImplementation(async (_sessionId, address) => ({
    phase: "resumed", address: { ...address, generation: (BigInt(address.generation) + 1n).toString() },
  }));
  vi.mocked(taskApi.computerTaskPreview).mockImplementation(async (_sessionId, address) => ({ address, previewDataUrl: "data:image/png;base64,AAAA", truncated: false }));
  vi.mocked(pickerApi.computerTaskPickerBegin).mockImplementation(async (sessionId, key, surface) => ({ sessionId, key, pickerId: "synthetic-picker", maxSelection: 2, surface }));
  vi.mocked(pickerApi.computerTaskPickerLoad).mockImplementation(async (picker) => ({ ...picker, candidates: [
    { candidateId: "fixture-A", title: "Fixture editor", appName: "Owned app", kind: "window", surface: "desktop" },
    { candidateId: "fixture-B", title: "Fixture viewer", appName: "Owned app", kind: "window", surface: "desktop" },
  ], truncated: false }));
  vi.mocked(pickerApi.computerTaskPickerCancel).mockResolvedValue(true);
  vi.mocked(pickerApi.computerTaskPickerApprove).mockImplementation(async (picker) => ({ requestId: picker.key.requestId, revision: picker.key.revision, runId: address.runId, decisionClass: "synthetic" }));
  vi.mocked(managedApi.computerTaskManagedApprove).mockImplementation(async (_sessionId, key) => ({ requestId: key.requestId, revision: key.revision, runId: address.runId, decisionClass: "synthetic" }));
  Object.defineProperty(document, "hidden", { configurable: true, value: false });
  const send = (view: ComputerTaskView, stream?: Stream) => {
    const original = stream ?? [...streams].reverse().find((value) => value.owner.sessionId === view.sessionId);
    if (!original) throw new Error("UI has no original subscription");
    original.callback({ ...original.owner, sequence: String(++original.sequence), kind: "view", view });
  };
  return { streams, send, emit: (view: ComputerTaskView) => act(async () => { send(view); }) };
}
