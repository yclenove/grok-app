export type ScanUiStatus =
  | "idle"
  | "queued"
  | "scanning"
  | "cancelling"
  | "paused"
  | "failed"
  | "empty"
  | "ready";

export function scanUiStatus(input: {
  state?: string | null;
  candidateCount: number;
}): ScanUiStatus {
  const state = input.state ?? "";
  if (state === "cancelling") return "cancelling";
  if (state === "failed") return "failed";
  if (state === "queued") return "queued";
  if (state === "scanning") return "scanning";
  if (state === "paused") return "paused";
  if (input.candidateCount > 0) return "ready";
  if (!state) return "idle";
  return "empty";
}
