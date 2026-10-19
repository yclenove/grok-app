// @vitest-environment node
import { describe, expect, it } from "vitest";
import { adversaries, explore, type Adversary } from "./fixtures/t04-model";

// Independently declared expected counterexample suffixes, not derived from
// protocol return codes or oracle results. Any new boundary changes this audit.
const residualStart: Partial<Record<Adversary, string>> = {
  switch_tab: "dispatch.tabs.get:after",
  move_window: "dispatch.tabs.get:after",
  move_geometry: "revalidate.final.native:after",
  minimize: "dispatch.windows.get:after",
  worker_restart: "dispatch.message:after",
};
const harmless = new Set<Adversary>(["focus_window", "late_response"]);

describe("ADR 0014 M0b deterministic T04 model (not browser evidence)", () => {
  it("binding_revalidation_without_interference_matches_independent_oracle", async () => {
    const run = await explore();
    expect(run.outcome).toBe("done");
    expect(run.oracle.effects).toHaveLength(1);
    expect(run.oracle.violations()).toEqual([]);
    // 39 asynchronous API/transport calls x 2, 17 helper-await resumptions,
    // and two explicit cross-thread gaps. No helper-return await is hidden.
    expect(run.schedule.trace).toHaveLength(97);
    expect(new Set(run.schedule.trace).size).toBe(97);
    expect(run.native.acceptedResponses).toBe(1);
    expect(run.desktop.titles.get(1)).toBe("baseline");
  });

  it.each(adversaries)("every_await_boundary_%s_classifies_blocked_or_residual", async adversary => {
    const baseline = await explore();
    const trace = baseline.schedule.trace;
    for (const [index, point] of trace.entries()) {
      const run = await explore({ point, adversary });
      expect(run.schedule.injected, point).toBe(true);
      const start = residualStart[adversary];
      const residual = start !== undefined && index >= trace.indexOf(start);
      if (residual) {
        // Known UNSAFE execution: passing this test keeps D2 CLOSED.
        expect(run.outcome, point).toBe("done");
        expect(run.oracle.effects, point).toHaveLength(1);
        expect(run.oracle.violations(), point).toHaveLength(1);
      } else if (harmless.has(adversary)) {
        expect(run.outcome, point).toBe("done");
        expect(run.oracle.effects, point).toHaveLength(1);
        expect(run.oracle.violations(), point).toEqual([]);
      } else {
        expect(run.outcome, point).toBe("refused");
        expect(run.oracle.effects, point).toEqual([]);
        expect(run.oracle.violations(), point).toEqual([]);
        expect(run.native.admission, point).toBe(false);
        expect(run.native.connectionBound || run.native.hubBound, point).toBe(false);
      }
      expect(run.oracle.effects.length, point).toBeLessThanOrEqual(1);
      const effectsBeforeNotification = run.oracle.effects.length;
      run.desktop.flushEvents();
      expect(run.oracle.effects.length, point).toBe(effectsBeforeNotification);
      // Receiving an old success cannot publish a new binding or replay input.
      const admission = run.native.admission;
      const acceptedResponses = run.native.acceptedResponses;
      run.native.receive(0, "retired-request");
      expect(run.native.admission, point).toBe(admission);
      expect(run.native.acceptedResponses, point).toBe(acceptedResponses);
      expect(run.oracle.effects.length, point).toBe(effectsBeforeNotification);
    }
  });

  it.each(adversaries)("immediate_invalidation_%s_refuses_before_effect_or_preserves_identity", async adversary => {
    const { schedule } = await explore();
    for (const point of schedule.trace) {
      const run = await explore({ point, adversary, notification: "immediate" });
      expect(run.schedule.injected, point).toBe(true);
      expect(run.oracle.violations(), point).toEqual([]);
      expect(run.outcome, point).toBe(harmless.has(adversary) ? "done" : "refused");
      expect(run.oracle.effects.length, point).toBe(harmless.has(adversary) ? 1 : 0);
    }
  });

  it.each(["switch_tab", "move_window"] as const)("check_then_dispatch_%s_explicitly_retains_wrong_delivery_window", async adversary => {
    const run = await explore({ point: "dispatch.beforePost", adversary });
    expect(run.outcome).toBe("done");
    expect(run.oracle.effects).toHaveLength(1);
    expect(run.oracle.violations()).toHaveLength(1);
    const [effect] = run.oracle.effects;
    expect(effect.actualDocument).toBe(run.desktop.original); // Exact port does not fix ownership.
    if (adversary === "move_window") expect(effect.actualWindow).not.toBe(effect.policyWindow);
    else expect(effect.selectedDocument).not.toBe(effect.actualDocument);
    run.desktop.flushEvents();
    expect(run.native.admission).toBe(false);
    expect(run.oracle.violations()).toHaveLength(1); // Post-hoc revoke cannot erase the effect.
  });

  it("oracle_deliberate_wrong_document_same_window_detects_false_success", async () => {
    const run = await explore();
    const other = run.desktop.tabs.get(2)!.document;
    const originalEffect = run.oracle.effects[0];
    run.oracle.commit({ window: originalEffect.policyWindow, document: run.desktop.original,
      geometry: 0, epoch: 1 }, other);
    expect(run.oracle.effects).toHaveLength(2);
    expect(run.oracle.violations()).toHaveLength(1);
    expect(run.oracle.violations()[0].actualDocument).toBe(other);
  });

  it("worker_death_after_marker_install_leaves_observable_ui_without_capability", async () => {
    const run = await explore({ point: "bind.install.action.setTitle:after", adversary: "worker_restart" });
    expect(run.outcome).toBe("refused");
    expect(run.desktop.titles.get(1)).toBe("t04-model-nonce-1");
    expect(run.native.admission || run.native.connectionBound || run.native.hubBound).toBe(false);
    expect(run.oracle.effects).toEqual([]);
    run.native.receive(1, "one-shot");
    expect(run.native.acceptedResponses).toBe(0);
    expect(run.oracle.effects).toEqual([]);
  });

  it("d2_closed_gate_at_every_injection_point_has_zero_effects", async () => {
    const baseline = await explore();
    for (const adversary of adversaries) for (const point of baseline.schedule.trace) {
      const run = await explore({ point, adversary }, "d2-closed");
      expect(run.outcome).toBe("refused");
      expect(run.schedule.trace).toEqual([]); // Stops before challenge/probes/dispatch; consent port pre-exists.
      expect(run.desktop.titles.size).toBe(0);
      expect(run.native.connectionBound || run.native.hubBound || run.native.admission).toBe(false);
      expect(run.oracle.effects).toEqual([]);
    }
  });
});
