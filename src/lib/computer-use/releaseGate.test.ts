import { describe, expect, it } from "vitest";
import {
  absentReport,
  channelFor,
  judgeRelease,
  shippedReleaseEvidence,
  type EvidenceSource,
  type LayerStatus,
  type ReleaseEvidence,
} from "./releaseGate";

const APP_IDS = [
  "app-approval-01",
  "app-image-session-01",
  "app-permission-01",
  "app-ssh-inject-01",
  "app-stop-01",
  "app-switch-01",
];

function layers(status: LayerStatus): ReleaseEvidence["layers"] {
  const all: ReleaseEvidence["layers"] = {};
  for (const platform of ["macos", "windows", "linux"]) {
    all[platform] = {};
    for (let index = 1; index <= 7; index += 1) all[platform][`L${index}`] = status;
  }
  return all;
}

function formalEvidence(source: EvidenceSource): ReleaseEvidence {
  const l6: ReleaseEvidence["l6"] = {};
  for (const platform of ["macos", "windows", "linux"]) {
    l6[platform] = { native_bp: 9000, text_bp: 9000, overall_bp: 9000 };
  }
  return {
    source,
    layers: layers("pass"),
    l6,
    security_signed: Array(10).fill(true),
    screenshot_chain_redacted: true,
    unreliable_screenshot_refused: true,
    ide_unknown_stop_regression: true,
    performance_budget_met: true,
    compatibility_apps: 30,
    signed_and_notarized: true,
    red_team_violations: 0,
  };
}

describe("computer use release gate", () => {
  it("ships experimental when no runner report exists", () => {
    expect(shippedReleaseEvidence()).toBeNull();
    expect(channelFor(null)).toBe("experimental");
    const missing = absentReport();
    expect(missing.promote).toBe(false);
    expect(missing.defaultFormal).toBe(false);
    expect(missing.tier).toBe("experimental");
    expect(APP_IDS).toHaveLength(6);
  });

  it("rejects bench data, partial, not_run, and a performance-only report", () => {
    const bench = judgeRelease(formalEvidence("bench"));
    expect(bench.channel).toBe("experimental");
    expect(bench.promote).toBe(false);
    expect(bench.defaultFormal).toBe(false);
    expect(bench.reasons).toEqual(["performance data cannot replace the eval runner"]);
    const smuggled = judgeRelease({ ...formalEvidence("bench"), promote: true } as ReleaseEvidence);
    expect(smuggled.promote).toBe(false);

    const partial = formalEvidence("runner");
    partial.layers.macos.L4 = "partial";
    expect(judgeRelease(partial).tier).toBe("experimental");

    const skipped = formalEvidence("runner");
    skipped.layers.linux.L7 = "not_run";
    const beta = judgeRelease(skipped);
    expect(beta.tier).toBe("beta");
    expect(beta.channel).toBe("experimental");
    expect(beta.defaultFormal).toBe(false);

    const budget = formalEvidence("runner");
    budget.layers = {};
    budget.l6 = {};
    budget.security_signed = [];
    budget.screenshot_chain_redacted = false;
    budget.unreliable_screenshot_refused = false;
    budget.ide_unknown_stop_regression = false;
    budget.compatibility_apps = 0;
    budget.signed_and_notarized = false;
    budget.red_team_violations = 1;
    const only = judgeRelease(budget);
    expect(only.promote).toBe(false);
    expect(only.tier).toBe("experimental");
    expect(only.reasons.some((item) => item.includes("L1-L4"))).toBe(true);
  });

  it("keeps alpha and beta experimental and promotes only a full runner report", () => {
    const preview = formalEvidence("runner");
    preview.l6.macos = { native_bp: 7000, text_bp: 1000, overall_bp: 9000 };
    preview.security_signed = Array(6).fill(true);
    preview.performance_budget_met = false;
    preview.compatibility_apps = 0;
    preview.signed_and_notarized = false;
    const alpha = judgeRelease(preview);
    expect(alpha.tier).toBe("alpha");
    expect(alpha.defaultFormal).toBe(false);
    expect(channelFor(preview)).toBe("experimental");

    const low = formalEvidence("runner");
    low.l6.macos.native_bp = 6999;
    expect(judgeRelease(low).tier).toBe("experimental");

    const overall = formalEvidence("runner");
    overall.l6.macos.overall_bp = 8499;
    expect(judgeRelease(overall).tier).toBe("beta");
    overall.l6.macos.overall_bp = 8500;
    overall.l6.windows.overall_bp = 7499;
    expect(judgeRelease(overall).promote).toBe(false);
    overall.l6.windows.overall_bp = 7500;
    overall.l6.linux.overall_bp = 7500;
    overall.compatibility_apps = 29;
    expect(judgeRelease(overall).promote).toBe(false);
    overall.compatibility_apps = 30;
    const formal = judgeRelease(overall);
    expect(formal.tier).toBe("formal");
    expect(formal.promote).toBe(true);
    expect(formal.defaultFormal).toBe(true);
    expect(formal.channel).toBe("formal");
    expect(formal.reasons).toEqual([]);

    overall.l6.macos.native_bp = 10001;
    expect(judgeRelease(overall).reasons).toEqual(["release evidence is invalid"]);
  });
});
