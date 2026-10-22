/** Pull inventory / issues out of a preview adapter payload. */

type Rec = Record<string, unknown>;

function asRec(v: unknown): Rec | null {
  return v !== null && typeof v === "object" ? (v as Rec) : null;
}

function num(v: unknown): number {
  return typeof v === "number" && Number.isFinite(v) ? v : 0;
}

export type PeekIssue = { code: string; message: string };

export type PeekSummary = {
  outcome: string | null;
  eventCount: number;
  messages: number;
  tools: number;
  attachments: number;
  missing: number;
  unsupported: number;
  sourceTokens: number;
  issues: PeekIssue[];
  cwd: string | null;
  leaf: string | null;
  leaves: string[];
  defaultSelection: unknown | null;
};

export function peekSummaryFromAdapter(adapter: unknown): PeekSummary {
  const empty: PeekSummary = {
    outcome: null,
    eventCount: 0,
    messages: 0,
    tools: 0,
    attachments: 0,
    missing: 0,
    unsupported: 0,
    sourceTokens: 0,
    issues: [],
    cwd: null,
    leaf: null,
    leaves: [],
    defaultSelection: null,
  };
  const o = asRec(adapter);
  if (!o) return empty;
  const inv = asRec(o.inventory);
  const imported = asRec(inv?.imported);
  const missing = asRec(inv?.missing);
  const unsupported = asRec(inv?.unsupported);
  const graph = asRec(o.graph);
  const events = Array.isArray(graph?.events) ? graph.events : [];
  const issues: PeekIssue[] = [];
  if (Array.isArray(o.issues)) {
    for (const raw of o.issues) {
      const issue = asRec(raw);
      if (!issue) continue;
      issues.push({
        code: typeof issue.code === "string" ? issue.code : "",
        message: typeof issue.message === "string" ? issue.message : "",
      });
    }
  }
  return {
    outcome: typeof o.outcome === "string" ? o.outcome : null,
    eventCount: num(o.eventCount) || events.length,
    messages: num(imported?.messages),
    tools: num(imported?.toolCalls),
    attachments: num(imported?.attachments),
    missing: num(missing?.attachments),
    unsupported:
      num(unsupported?.messages) + num(unsupported?.toolCalls) + num(unsupported?.toolResults),
    sourceTokens: num(inv?.sourceTokens),
    issues,
    cwd: typeof graph?.cwd === "string" ? graph.cwd : typeof o.cwd === "string" ? o.cwd : null,
    leaf:
      typeof o.activeLeaf === "string"
        ? o.activeLeaf
        : typeof graph?.activeLeaf === "string"
          ? graph.activeLeaf
          : null,
    leaves: Array.isArray(o.leaves)
      ? o.leaves.filter((item): item is string => typeof item === "string")
      : [],
    defaultSelection: o.defaultSelection ?? null,
  };
}
