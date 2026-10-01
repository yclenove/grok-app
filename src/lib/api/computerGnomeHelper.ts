/** Desktop-settings transport only. Never exported as an agent or mirror tool. */
import { invoke, isDesktopHost } from "./host";

export type HelperAction = "install" | "repair" | "enable" | "disable";
export type HelperState = "unavailable" | "unsupported" | "conflict" | "missing" | "repair_required"
  | "restart_required" | "global_disabled" | "ready" | "disabled" | "blocked" | "unconfirmed";
export type HelperStatus = {
  available: boolean;
  busy: boolean;
  featureEnabled: boolean;
  state: HelperState;
  shellVersion: string | null;
  installation: string | null;
  actions: HelperAction[];
};
// Component unmount and read timeouts cannot release this mutation latch.
let mutation: Promise<HelperStatus> | null = null;
export async function computerHelperStatus(): Promise<HelperStatus> {
  if (!isDesktopHost()) return {
    available: false, busy: false, featureEnabled: false, state: "unavailable",
    shellVersion: null, installation: null, actions: [],
  };
  const result = await invoke<HelperStatus>("computer_use_helper_status");
  return mutation ? { ...result, busy: true, actions: [] } : result;
}
export function computerHelperAction(action: HelperAction): Promise<HelperStatus> {
  if (!isDesktopHost()) return Promise.reject(new Error("computer_use_helper_unavailable"));
  if (mutation) return Promise.reject(new Error("computer_use_helper_busy"));
  mutation = invoke<HelperStatus>("computer_use_helper_action", { action })
    .finally(() => { mutation = null; });
  return mutation;
}
