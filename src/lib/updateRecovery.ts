/** Native Windows ownership or an explicit verified terminal receipt. */
export type NativePendingInstall = {
  candidateId: string;
  version: string;
  state: "running" | "retryable" | "blocked" | "failed" | "completed";
  phase: string;
  message: string | null;
};

/** Invalid/missing IPC data is not an authoritative empty recovery catalog. */
export function decodePendingInstall(value: unknown): NativePendingInstall | null {
  if (value === null) return null;
  if (typeof value !== "object" || value === null) throw new Error("Invalid native update recovery metadata");
  const data = value as Record<string, unknown>;
  if (typeof data.candidateId !== "string" || !data.candidateId.trim()
    || typeof data.version !== "string" || !data.version.trim()
    || typeof data.state !== "string" || !["running", "retryable", "blocked", "failed", "completed"].includes(data.state)
    || typeof data.phase !== "string" || !data.phase.trim()
    || !(data.message === null || typeof data.message === "string")
    || (data.state === "retryable" && !["cleanup", "launch", "exit"].includes(data.phase))
    || (data.state === "failed" && data.phase !== "staging")
    || (data.state === "completed" && (data.phase !== "completed" || data.message !== null))) {
    throw new Error("Invalid native update recovery metadata");
  }
  return data as NativePendingInstall;
}
