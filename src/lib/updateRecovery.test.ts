import { describe, expect, it } from "vitest";
import { decodePendingInstall } from "./updateRecovery";

const candidate = { candidateId: "original-id", version: "1.2.3", state: "retryable", phase: "cleanup", message: "refused" };
describe("native update recovery protocol", () => {
  it("requires an authoritative null for an empty catalog", () => {
    expect(decodePendingInstall(null)).toBeNull();
    for (const value of [undefined, false, "", {}, [], { ...candidate, candidateId: " " },
      { ...candidate, state: "complete" }, { ...candidate, version: "" },
      { ...candidate, state: ["running"] },
      { ...candidate, state: "failed", phase: "cleanup" },
      { ...candidate, state: "completed", phase: "installer_exited", message: null },
      { ...candidate, state: "completed", phase: "completed", message: "exit 0" },
      { ...candidate, message: {} }, { ...candidate, message: undefined },
      { ...candidate, phase: "launch_unknown" }, { ...candidate, phase: "" }]) {
      expect(() => decodePendingInstall(value)).toThrow("Invalid native update recovery metadata");
    }
  });
  it("preserves opaque original identity and does not infer installed state", () => {
    expect(decodePendingInstall(candidate)).toEqual(candidate);
    expect(decodePendingInstall({ ...candidate, state: "blocked", phase: "launch_unknown", message: null }))
      .toMatchObject({ state: "blocked", phase: "launch_unknown" });
    expect(decodePendingInstall({ ...candidate, state: "running", phase: "staging" }))
      .toMatchObject({ state: "running" });
    expect(decodePendingInstall({ ...candidate, state: "failed", phase: "staging" }))
      .toMatchObject({ state: "failed" });
  });
  it("keeps process observations blocked regardless of a successful-looking exit code", () => {
    for (const phase of ["installer_running", "installer_exited", "installer_unobserved", "installer_failed", "installer_cancelled"]) {
      const observation = { ...candidate, state: "blocked", phase, message: "process exit code 0 is not installation success" };
      expect(decodePendingInstall(observation)).toEqual(observation);
      expect(() => decodePendingInstall({ ...observation, state: "retryable" })).toThrow();
      expect(() => decodePendingInstall({ ...observation, state: "failed" })).toThrow();
      expect(() => decodePendingInstall({ ...observation, state: "completed" })).toThrow();
    }
  });
  it("accepts only an explicit verified native completion receipt", () => {
    const completed = { ...candidate, state: "completed", phase: "completed", message: null };
    expect(decodePendingInstall(completed)).toEqual(completed);
  });
});
