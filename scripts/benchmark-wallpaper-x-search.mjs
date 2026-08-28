#!/usr/bin/env node

/**
 * Compare the current headless Grok CLI wallpaper search with a minimal
 * Build-OAuth Responses request. The script never prints credentials or raw
 * model/search output; stdout contains sanitized JSON metrics only.
 *
 * Examples:
 *   node scripts/benchmark-wallpaper-x-search.mjs --route responses --limit 1
 *   node scripts/benchmark-wallpaper-x-search.mjs --route cli --topic misty-mountain
 *   node scripts/benchmark-wallpaper-x-search.mjs --route responses --effort medium
 *   node scripts/benchmark-wallpaper-x-search.mjs --batch-strategy staggered --stagger-ms 10000 --topic ocean-ultrawide
 */

import { spawn, spawnSync } from "node:child_process";
import { readFile } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";
import process from "node:process";
import { clearTimeout, setTimeout } from "node:timers";
import { URL } from "node:url";

const { AbortController, fetch } = globalThis;

const RESPONSES_URL = "https://cli-chat-proxy.grok.com/v1/responses";
const DEFAULT_MODEL = "grok-4.6";
const DEFAULT_EFFORT = "low";
const DEFAULT_MAX_TOOL_CALLS = 3;
const DEFAULT_RESPONSES_TIMEOUT_MS = 90_000;
const DEFAULT_CLI_TIMEOUT_MS = 155_000;
const PROBE_TIMEOUT_MS = 15_000;
const PROBE_CONCURRENCY = 8;
const DEFAULT_BATCH_TARGET_COUNT = 12;
const DEFAULT_STAGGER_MS = 10_000;
const USEFUL_GALLERY_COUNT = 6;

const TOPICS = [
  {
    id: "misty-mountain",
    query: "misty mountain landscape wide desktop wallpaper",
    sort: "top",
  },
  {
    id: "cyberpunk-vertical-zh",
    query: "赛博朋克城市夜景 竖屏手机壁纸",
    sort: "latest",
  },
  {
    id: "ocean-ultrawide",
    query: "ocean sunset photography ultrawide wallpaper",
    sort: "top",
  },
  {
    id: "ink-landscape-zh",
    query: "极简中国山水 横屏壁纸",
    sort: "top",
  },
  {
    id: "space-nebula",
    query: "space nebula 4k astrophotography wallpaper",
    sort: "latest",
  },
  {
    id: "abstract-ai-prompt",
    query: "AI art glass abstract gradient wallpaper prompt",
    sort: "top",
  },
  {
    id: "macro-flower-vertical",
    query: "macro flower photography vertical wallpaper",
    sort: "latest",
  },
  {
    id: "rainy-anime-street",
    query: "cozy anime rainy street desktop wallpaper",
    sort: "top",
  },
];

function gallerySchema(targetCount = null) {
  return {
    type: "object",
    properties: {
      items: {
        type: "array",
        ...(targetCount == null ? {} : { maxItems: targetCount }),
        items: {
          type: "object",
          properties: {
            fullUrl: { type: "string" },
            thumbUrl: { type: "string" },
            username: { type: "string" },
            postUrl: { type: "string" },
            textPreview: { type: "string" },
            likes: { type: "number" },
            kind: { type: "string" },
          },
          required: ["fullUrl"],
          additionalProperties: false,
        },
      },
    },
    required: ["items"],
    additionalProperties: false,
  };
}

// Keep this permissive shape aligned with the current Rust product path.
function cliGallerySchema(targetCount = null) {
  return {
    type: "object",
    properties: {
      items: {
        type: "array",
        ...(targetCount == null ? {} : { maxItems: targetCount }),
        items: {
          type: "object",
          properties: {
            fullUrl: { type: "string" },
            thumbUrl: { type: "string" },
            username: { type: "string" },
            postUrl: { type: "string" },
            textPreview: { type: "string" },
            likes: { type: "number" },
            kind: { type: "string" },
          },
          required: ["fullUrl"],
        },
      },
    },
    required: ["items"],
  };
}

function parseArgs(argv) {
  const opts = {
    route: "responses",
    effort: DEFAULT_EFFORT,
    maxToolCalls: DEFAULT_MAX_TOOL_CALLS,
    targetCount: null,
    batchStrategy: "single",
    batchCount: 2,
    staggerMs: DEFAULT_STAGGER_MS,
    limit: TOPICS.length,
    topic: null,
    probe: true,
    allowToolOverrun: false,
    maxToolCallsExplicit: false,
    batchCountExplicit: false,
    staggerMsExplicit: false,
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    const next = () => {
      const value = argv[++i];
      if (!value) throw new Error(`missing value for ${arg}`);
      return value;
    };
    if (arg === "--route") opts.route = next();
    else if (arg === "--effort") opts.effort = next();
    else if (arg === "--max-tool-calls") {
      opts.maxToolCalls = Number(next());
      opts.maxToolCallsExplicit = true;
    } else if (arg === "--target-count") opts.targetCount = Number(next());
    else if (arg === "--batch-strategy") opts.batchStrategy = next();
    else if (arg === "--batch-count") {
      opts.batchCount = Number(next());
      opts.batchCountExplicit = true;
    } else if (arg === "--stagger-ms") {
      opts.staggerMs = Number(next());
      opts.staggerMsExplicit = true;
    } else if (arg === "--limit") opts.limit = Number(next());
    else if (arg === "--topic") opts.topic = next();
    else if (arg === "--no-probe") opts.probe = false;
    else if (arg === "--allow-tool-overrun") opts.allowToolOverrun = true;
    else if (arg === "--help" || arg === "-h") opts.help = true;
    else throw new Error(`unknown argument: ${arg}`);
  }
  if (!new Set(["responses", "cli", "both"]).has(opts.route)) {
    throw new Error("--route must be responses, cli, or both");
  }
  if (!new Set(["low", "medium"]).has(opts.effort)) {
    throw new Error("--effort must be low or medium");
  }
  if (
    !new Set(["single", "serial", "parallel", "staggered"]).has(opts.batchStrategy)
  ) {
    throw new Error("--batch-strategy must be single, serial, parallel, or staggered");
  }
  if (!Number.isInteger(opts.staggerMs) || opts.staggerMs < 0 || opts.staggerMs > 60_000) {
    throw new Error("--stagger-ms must be an integer from 0 to 60000");
  }
  if (opts.staggerMsExplicit && opts.batchStrategy !== "staggered") {
    throw new Error("--stagger-ms requires --batch-strategy staggered");
  }
  if (!Number.isInteger(opts.batchCount) || opts.batchCount < 2 || opts.batchCount > 4) {
    throw new Error("--batch-count must be an integer from 2 to 4");
  }
  if (opts.batchCountExplicit && opts.batchStrategy === "single") {
    throw new Error("--batch-count requires a multi-batch strategy");
  }
  if (opts.batchStrategy !== "single") {
    if (opts.route !== "responses") {
      throw new Error("multi-batch strategies require --route responses");
    }
    if (!opts.maxToolCallsExplicit) opts.maxToolCalls = 1;
    opts.targetCount ??= DEFAULT_BATCH_TARGET_COUNT;
  }
  if (!Number.isInteger(opts.maxToolCalls) || opts.maxToolCalls < 1 || opts.maxToolCalls > 3) {
    throw new Error("--max-tool-calls must be an integer from 1 to 3");
  }
  if (
    opts.targetCount != null &&
    (!Number.isInteger(opts.targetCount) || opts.targetCount < 4 || opts.targetCount > 16)
  ) {
    throw new Error("--target-count must be an integer from 4 to 16");
  }
  if (!Number.isInteger(opts.limit) || opts.limit < 1 || opts.limit > TOPICS.length) {
    throw new Error(`--limit must be an integer from 1 to ${TOPICS.length}`);
  }
  return opts;
}

function usage() {
  return [
    "Usage: node scripts/benchmark-wallpaper-x-search.mjs [options]",
    "  --route responses|cli|both",
    "  --effort low|medium",
    "  --max-tool-calls 1..3  Responses only (single default 3; multi-batch default 1 per lane)",
    "  --target-count 4..16    Cap requested/returned items (multi-batch default 12 per lane)",
    "  --batch-strategy single|serial|parallel|staggered",
    "                           Responses scheduling strategy (default single)",
    "  --batch-count 2..4       Lane count for multi-batch strategies (default 2)",
    "  --stagger-ms 0..60000    Per-lane delay increment (staggered default 10000)",
    "  --allow-tool-overrun     Benchmark only: score outputs beyond the declared budget",
    "  --topic <id>            Run one fixed topic",
    `  --limit 1..${TOPICS.length}             Run the first N topics`,
    "  --no-probe              Skip remote image reachability probes",
  ].join("\n");
}

function selectTopics(opts) {
  if (!opts.topic) return TOPICS.slice(0, opts.limit);
  const topic = TOPICS.find((item) => item.id === opts.topic);
  if (!topic) throw new Error(`unknown topic: ${opts.topic}`);
  return [topic];
}

function usableAuthEntry(auth) {
  if (!auth || typeof auth !== "object" || Array.isArray(auth)) return null;
  return Object.values(auth).find((entry) => {
    if (!entry || typeof entry !== "object") return false;
    return Boolean(
      (typeof entry.key === "string" && entry.key.length > 0) ||
        (typeof entry.access_token === "string" && entry.access_token.length > 0),
    );
  }) ?? null;
}

async function readBuildOauth() {
  const authPath = join(homedir(), ".grok", "auth.json");
  let auth;
  try {
    auth = JSON.parse(await readFile(authPath, "utf8"));
  } catch {
    throw new Error("oauth_unavailable");
  }
  const entry = usableAuthEntry(auth);
  if (!entry) throw new Error("oauth_unavailable");
  if (typeof entry.expires_at === "string") {
    const expiresAt = Date.parse(entry.expires_at);
    if (Number.isFinite(expiresAt) && expiresAt <= Date.now() + 60_000) {
      throw new Error(expiresAt <= Date.now() ? "oauth_expired" : "oauth_near_expiry");
    }
  }
  return entry.key || entry.access_token;
}

function responsesPromptFor(
  topic,
  maxToolCalls,
  targetCount,
  batchIndex = null,
  batchCount = null,
) {
  const sort = topic.sort === "latest" ? "Latest" : "Top";
  const resultTarget =
    targetCount == null
      ? "Return 8-16 distinct items when possible."
      : `Return ${targetCount} distinct items when possible. Return fewer only when you cannot confirm enough candidates; never pad, duplicate, or guess metadata.`;
  const batchRoles = [
    null,
    "Primary batch: search the strongest direct interpretation of the topic and prioritize immediately recognizable wallpaper candidates.",
    "Visual-variation batch: avoid repeating the primary lane; emphasize alternate composition, lighting, season, or medium.",
    "Discovery batch: use different viewpoint, palette, time or weather, cultural framing, or bilingual keywords; avoid the obvious direct and visual-variation queries.",
    "Long-tail batch: explore less obvious related scenes, synonyms, and niche high-quality interpretations; avoid the direct, visual-variation, and discovery lanes.",
  ];
  const batchGuidance = batchIndex
    ? `Concurrent batch ${batchIndex} of ${batchCount}: ${batchRoles[batchIndex]}`
    : null;
  return `You collect high-quality still images from X (Twitter) for a wallpaper picker.

User topic: ${topic.query}
Sort preference: ${sort}

${batchGuidance ? `${batchGuidance}\n\n` : ""}Use X search only. Use no more than ${maxToolCalls} x_search ${maxToolCalls === 1 ? "call" : "calls"}. Search useful query variants with image/media filters. Prefer real photography or polished AI art suitable as wallpaper. Prefer posts that include both a prompt and attached images when relevant. Skip memes, screenshots, text cards, avatars, emoji packs, ads, blurry thumbnails, and placeholder links.

${resultTarget} fullUrl must be a direct full-size media CDN URL, preferably https://pbs.twimg.com/media/... with name=orig. postUrl must be a confirmed canonical https://x.com/<user>/status/<id>; omit it rather than guessing. Return metadata only and do not download files.`;
}

function cliPromptFor(topic, targetCount) {
  const sort = topic.sort === "latest" ? "Latest" : "Top";
  const resultTarget =
    targetCount == null
      ? "items array, 12-28 when possible"
      : `items array, ${targetCount} when possible; return fewer only when you cannot confirm enough candidates, and never pad, duplicate, or guess metadata`;
  return `You collect high-quality still images from X (Twitter) for a desktop wallpaper picker.

User topic (raw): ${topic.query}

Search strategy (use X tools; sort = ${sort}):
1. Expand the user topic into 2-4 effective queries before searching. Prefer posts that:
   - Share AI image-generation prompts (prompt share / Midjourney / Flux / SD / Grok Imagine / "prompt" / 提示词 / 咒语)
   - Attach real photos or AI art suitable as wallpaper (landscape, scenery, aesthetic stills)
2. Always require media: use filter:images (or media). Prefer higher engagement (likes/reposts) when sort is Top.
3. Prefer posts that include BOTH the prompt text and attached images; if none, fall back to high-quality image posts about the topic.
4. Skip low quality: memes with heavy text overlays, screenshots of chat UI, profile avatars, emoji packs, ads, pure text cards, blurry thumbs, broken/placeholder links.
5. Collect distinct direct IMAGE CDN URLs only for fullUrl - prefer https://pbs.twimg.com/media/... (name=orig or full size). Never put status page URLs in fullUrl.
6. Always set postUrl to the real canonical status link https://x.com/<user>/status/<id> when the post is known. Never invent or guess a status id. If you cannot confirm the status URL, omit postUrl (client will mark the tile Unverified).
7. Return exactly ONE JSON object matching the schema (${resultTarget}). No prose, no second JSON object, no placeholder.jpg.

Do not download files - metadata only.`;
}

function normalizeMediaUrl(raw) {
  if (typeof raw !== "string") return "";
  const trimmed = raw.trim();
  if (!trimmed) return "";
  for (const suffix of [":thumb", ":small", ":medium", ":large"]) {
    if (trimmed.endsWith(suffix)) return `${trimmed.slice(0, -suffix.length)}:orig`;
  }
  try {
    const url = new URL(trimmed);
    if (url.hostname.toLowerCase() === "pbs.twimg.com" && url.pathname.startsWith("/media/")) {
      url.searchParams.set("name", "orig");
    }
    return url.href;
  } catch {
    return trimmed;
  }
}

function allowedMediaUrl(raw) {
  let url;
  try {
    url = new URL(raw);
  } catch {
    return false;
  }
  if (url.protocol !== "https:" && url.protocol !== "http:") return false;
  const host = url.hostname.toLowerCase();
  const exact = new Set([
    "pbs.twimg.com",
    "video.twimg.com",
    "ton.twimg.com",
    "abs.twimg.com",
    "cdn.grok.com",
    "assets.grok.com",
    "imagine-public.x.ai",
    "imgen.x.ai",
    "filesystem.site",
  ]);
  return exact.has(host) || host.endsWith(".x.ai") || host.endsWith(".twimg.com") || host === "x.ai";
}

function canonicalPostUrl(raw) {
  if (typeof raw !== "string") return false;
  try {
    const url = new URL(raw);
    const host = url.hostname.toLowerCase().replace(/^www\./, "");
    return (
      (host === "x.com" || host === "twitter.com" || host === "mobile.twitter.com") &&
      /\/(?:status|statuses)\/\d{8,}(?:\/|$)/i.test(url.pathname)
    );
  } catch {
    return false;
  }
}

function parsePossibleJson(text) {
  if (typeof text !== "string" || !text.trim()) return null;
  const trimmed = text.trim();
  const candidates = [trimmed];
  const fence = trimmed.match(/```(?:json)?\s*([\s\S]*?)```/i);
  if (fence) candidates.push(fence[1].trim());
  for (const candidate of candidates) {
    try {
      return JSON.parse(candidate);
    } catch {
      // Try the next safe parse candidate. Raw text is never printed.
    }
  }
  return null;
}

function iterJsonObjects(text) {
  const values = [];
  for (let start = 0; start < text.length; start += 1) {
    if (text[start] !== "{") continue;
    let depth = 0;
    let inString = false;
    let escaped = false;
    let end = -1;
    for (let cursor = start; cursor < text.length; cursor += 1) {
      const char = text[cursor];
      if (inString) {
        if (escaped) escaped = false;
        else if (char === "\\") escaped = true;
        else if (char === '"') inString = false;
      } else if (char === '"') {
        inString = true;
      } else if (char === "{") {
        depth += 1;
      } else if (char === "}") {
        depth -= 1;
        if (depth === 0) {
          end = cursor;
          break;
        }
      }
    }
    if (end < 0) break;
    try {
      values.push(JSON.parse(text.slice(start, end + 1)));
    } catch {
      // Keep scanning without exposing model output.
    }
    start = end;
  }
  return values;
}

function harvestMediaItems(text) {
  const matches = text.match(/https?:\/\/[^\s"')\]}>`]+/gi) ?? [];
  const urls = matches
    .map((url) => normalizeMediaUrl(url.replace(/[.,;:\\]+$/g, "")))
    .filter((url) => url && allowedMediaUrl(url));
  return [...new Set(urls)].map((fullUrl) => ({ fullUrl, kind: "image" }));
}

function galleryFromModelText(text, depth = 0) {
  if (typeof text !== "string" || !text.trim() || depth > 5) return null;
  const trimmed = text.trim();
  const candidates = [];
  const fence = trimmed.match(/```(?:json)?\s*([\s\S]*?)```/i);
  if (fence) candidates.push(fence[1].trim());
  candidates.push(trimmed);

  for (const candidate of candidates) {
    try {
      const parsed = JSON.parse(candidate);
      const found =
        typeof parsed === "string"
          ? galleryFromModelText(parsed, depth + 1)
          : findGallery(parsed, depth + 1);
      if (found) return found;
    } catch {
      // Try brace-balanced objects below.
    }

    const merged = [];
    for (const value of iterJsonObjects(candidate)) {
      const found = findGallery(value, depth + 1);
      if (found) merged.push(...found);
      else if (value && typeof value === "object" && (value.fullUrl || value.full_url || value.url)) {
        merged.push(value);
      }
    }
    if (merged.length) return merged;
  }

  const harvested = harvestMediaItems(trimmed);
  return harvested.length ? harvested : null;
}

function findGallery(value, depth = 0) {
  if (!value || depth > 5) return null;
  if (typeof value === "string") return galleryFromModelText(value, depth + 1);
  if (typeof value !== "object") return null;
  if (Array.isArray(value.items)) return value.items;
  if (Array.isArray(value)) {
    if (value.some((item) => item && typeof item === "object" && (item.fullUrl || item.full_url || item.url))) {
      return value;
    }
    for (const item of value) {
      const found = findGallery(item, depth + 1);
      if (found) return found;
    }
    return null;
  }
  if (value.structuredOutput) {
    const found = findGallery(value.structuredOutput, depth + 1);
    if (found) return found;
  }
  if (typeof value.text === "string") {
    const found = galleryFromModelText(value.text, depth + 1);
    if (found) return found;
  }
  for (const [key, child] of Object.entries(value)) {
    if (key === "text" || key === "structuredOutput") continue;
    const found = findGallery(child, depth + 1);
    if (found) return found;
  }
  return null;
}

function galleryFromResponses(response) {
  for (const output of response?.output ?? []) {
    for (const content of output?.content ?? []) {
      if (content?.type !== "output_text") continue;
      const parsed = parsePossibleJson(content.text);
      const found = parsed ? findGallery(parsed) : null;
      if (found) return found;
    }
  }
  return [];
}

function safeErrorCategory(error) {
  const message = error instanceof Error ? error.message : String(error ?? "");
  const causeCode = typeof error?.cause?.code === "string" ? error.cause.code : "";
  if (/^oauth_(?:unavailable|expired|near_expiry)$/.test(message)) return message;
  if (error?.name === "AbortError" || /timeout|timed out/i.test(message)) return "timeout";
  if (causeCode === "UND_ERR_CONNECT_TIMEOUT" || causeCode === "ETIMEDOUT") {
    return "connect_timeout";
  }
  if (causeCode === "ECONNREFUSED") return "connection_refused";
  if (/^(?:CERT_|ERR_TLS_|UNABLE_TO_VERIFY_)/.test(causeCode)) return "tls_error";
  if (/^http_\d{3}$/.test(message)) return message;
  if (error instanceof SyntaxError) return "response_json_invalid";
  if (/parse|json|structured/i.test(message)) return "parse_failed";
  if (error instanceof TypeError) return "network_error";
  return "transport_or_process_error";
}

async function withAbortTimeout(timeoutMs, fn) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    return await fn(controller.signal);
  } finally {
    clearTimeout(timer);
  }
}

async function probeOne(raw) {
  const normalized = normalizeMediaUrl(raw);
  if (!allowedMediaUrl(normalized)) return false;
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), PROBE_TIMEOUT_MS);
  let responseBody = null;
  let reader = null;
  try {
    const response = await fetch(normalized, {
      signal: controller.signal,
      redirect: "follow",
      headers: {
        Accept: "image/avif,image/webp,image/*,*/*;q=0.8",
        Range: "bytes=0-2047",
      },
    });
    responseBody = response.body;
    if (response.status !== 200 && response.status !== 206) return false;
    if (!allowedMediaUrl(response.url)) return false;
    const mime = (response.headers.get("content-type") ?? "").toLowerCase();
    if (mime.startsWith("text/") || mime.includes("html") || mime.includes("json")) return false;
    if (!responseBody) return false;

    reader = responseBody.getReader();
    let prefixBytes = 0;
    while (prefixBytes < 2_048) {
      const { done, value } = await reader.read();
      if (done) break;
      prefixBytes += Math.min(value?.byteLength ?? 0, 2_048 - prefixBytes);
    }
    return prefixBytes >= 32;
  } catch {
    return false;
  } finally {
    clearTimeout(timer);
    if (reader) {
      await reader.cancel().catch(() => {});
    } else if (responseBody) {
      await responseBody.cancel().catch(() => {});
    }
  }
}

async function mapConcurrent(values, concurrency, fn) {
  const output = new Array(values.length);
  let index = 0;
  async function worker() {
    while (index < values.length) {
      const current = index++;
      output[current] = await fn(values[current]);
    }
  }
  await Promise.all(Array.from({ length: Math.min(concurrency, values.length) }, worker));
  return output;
}

async function qualityMetrics(items, shouldProbe) {
  const rawItems = Array.isArray(items) ? items : [];
  const media = rawItems
    .map((item) => normalizeMediaUrl(item?.fullUrl ?? item?.full_url ?? item?.url ?? ""))
    .filter(Boolean);
  const uniqueMedia = [...new Set(media)];
  const allowed = uniqueMedia.filter(allowedMediaUrl);
  const probeStarted = Date.now();
  const probeResults = shouldProbe
    ? await mapConcurrent(allowed, PROBE_CONCURRENCY, probeOne)
    : null;
  const validMedia = shouldProbe
    ? allowed.filter((_, index) => probeResults[index])
    : allowed;
  return {
    metrics: {
      candidateCount: rawItems.length,
      uniqueMediaCount: uniqueMedia.length,
      duplicateCount: Math.max(0, media.length - uniqueMedia.length),
      allowedMediaCount: allowed.length,
      reachableCount: shouldProbe ? validMedia.length : null,
      canonicalPostCount: rawItems.filter((item) =>
        canonicalPostUrl(item?.postUrl ?? item?.post_url ?? item?.statusUrl ?? ""),
      ).length,
      probeMs: shouldProbe ? Date.now() - probeStarted : 0,
    },
    // Kept in memory only for cross-batch dedupe; callers never serialize it.
    validMedia,
  };
}

async function runResponses(topic, opts, token, batchIndex = null) {
  const started = Date.now();
  try {
    const body = await withAbortTimeout(DEFAULT_RESPONSES_TIMEOUT_MS, async (signal) => {
      const response = await fetch(RESPONSES_URL, {
        method: "POST",
        signal,
        // The OAuth credential is valid only for the fixed Build proxy host.
        // A redirect is a protocol error, not permission to replay it elsewhere.
        redirect: "error",
        headers: {
          Authorization: `Bearer ${token}`,
          "Content-Type": "application/json",
          "x-grok-client-mode": "cli",
          "x-grok-client-identifier": "grok-shell",
          "x-grok-client-version": "1.0.5",
        },
        body: JSON.stringify({
          model: DEFAULT_MODEL,
          input: responsesPromptFor(
            topic,
            opts.maxToolCalls,
            opts.targetCount,
            batchIndex,
            opts.batchCount,
          ),
          tools: [{ type: "x_search" }],
          tool_choice: "auto",
          max_tool_calls: opts.maxToolCalls,
          reasoning: { effort: opts.effort, summary: "concise" },
          text: {
            format: {
              type: "json_schema",
              name: "wallpaper_gallery",
              strict: true,
              schema: gallerySchema(opts.targetCount),
            },
          },
          store: false,
        }),
      });
      if (!response.ok) {
        await response.body?.cancel().catch(() => {});
        throw new Error(`http_${response.status}`);
      }
      return response.json();
    });
    const searchMs = Date.now() - started;
    const items = galleryFromResponses(body);
    if (!items.length) throw new Error("parse_failed");
    const xSearchCalls = (body.output ?? []).filter((item) =>
      /(?:x_?search|custom_tool)/i.test(String(item?.type ?? "")),
    ).length;
    const baseResult = {
      topic: topic.id,
      sort: topic.sort,
      route: "responses",
      model: typeof body.model === "string" ? body.model : DEFAULT_MODEL,
      effort: opts.effort,
      maxToolCalls: opts.maxToolCalls,
      targetCount: opts.targetCount,
      xSearchCalls,
      toolBudgetExceeded: xSearchCalls > opts.maxToolCalls,
      searchMs,
      inputTokens: Number.isFinite(body?.usage?.input_tokens) ? body.usage.input_tokens : null,
      outputTokens: Number.isFinite(body?.usage?.output_tokens) ? body.usage.output_tokens : null,
    };
    if (xSearchCalls > opts.maxToolCalls && !opts.allowToolOverrun) {
      return {
        row: {
          ...baseResult,
          ok: false,
          candidateCount: items.length,
          durationMs: searchMs,
          errorCode: "tool_budget_exceeded",
        },
        validMedia: [],
      };
    }
    const quality = await qualityMetrics(items, opts.probe);
    const result = {
      ...baseResult,
      ok: true,
      totalMs: searchMs + quality.metrics.probeMs,
      ...quality.metrics,
    };
    const validCount = opts.probe
      ? quality.metrics.reachableCount
      : quality.metrics.allowedMediaCount;
    return {
      row:
        validCount === 0
          ? { ...result, ok: false, durationMs: result.totalMs, errorCode: "no_valid_images" }
          : result,
      validMedia: quality.validMedia,
    };
  } catch (error) {
    return {
      row: {
        topic: topic.id,
        sort: topic.sort,
        route: "responses",
        ok: false,
        effort: opts.effort,
        maxToolCalls: opts.maxToolCalls,
        targetCount: opts.targetCount,
        durationMs: Date.now() - started,
        errorCode: safeErrorCategory(error),
      },
      validMedia: [],
    };
  }
}

function delay(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function thresholdCompletionMs(completions, threshold) {
  const seen = new Set();
  const chronological = [...completions].sort((a, b) => a.completedAtMs - b.completedAtMs);
  for (const completion of chronological) {
    for (const media of completion.execution.validMedia) seen.add(media);
    if (seen.size >= threshold) return completion.completedAtMs;
  }
  return null;
}

function sanitizedBatchResult(completion, shouldProbe) {
  const { row } = completion.execution;
  return {
    batch: completion.batchIndex,
    completedAtMs: completion.completedAtMs,
    ok: row.ok,
    errorCode: row.errorCode ?? null,
    searchMs: row.searchMs ?? null,
    totalMs: row.totalMs ?? row.durationMs ?? null,
    candidateCount: row.candidateCount ?? 0,
    validCount: shouldProbe
      ? (row.reachableCount ?? 0)
      : (row.allowedMediaCount ?? 0),
    xSearchCalls: row.xSearchCalls ?? null,
    toolBudgetExceeded: row.toolBudgetExceeded ?? false,
  };
}

async function runResponsesBatchStrategy(topic, opts, token) {
  const started = Date.now();
  const completions = [];
  const executeBatch = async (batchIndex) => {
    const execution = await runResponses(topic, opts, token, batchIndex);
    const completion = {
      batchIndex,
      completedAtMs: Date.now() - started,
      execution,
    };
    completions.push(completion);
    return completion;
  };
  const batchIndexes = Array.from({ length: opts.batchCount }, (_, index) => index + 1);

  let completed;
  if (opts.batchStrategy === "serial") {
    completed = [];
    for (const batchIndex of batchIndexes) {
      completed.push(await executeBatch(batchIndex));
    }
  } else if (opts.batchStrategy === "parallel") {
    completed = await Promise.all(batchIndexes.map(executeBatch));
  } else {
    completed = await Promise.all(
      batchIndexes.map(async (batchIndex) => {
        if (batchIndex > 1) await delay(opts.staggerMs * (batchIndex - 1));
        return executeBatch(batchIndex);
      }),
    );
  }

  const rows = completed.map((completion) => completion.execution.row);
  const allValidMedia = completed.flatMap((completion) => completion.execution.validMedia);
  const uniqueValidMedia = new Set(allValidMedia);
  const validCount = uniqueValidMedia.size;
  const totalMs = Date.now() - started;
  const candidateCount = rows.reduce((sum, row) => sum + (row.candidateCount ?? 0), 0);
  const withinBatchDuplicates = rows.reduce((sum, row) => sum + (row.duplicateCount ?? 0), 0);
  const crossBatchDuplicates = Math.max(0, allValidMedia.length - validCount);
  const errorCodes = [...new Set(rows.map((row) => row.errorCode).filter(Boolean))];
  const ok = validCount > 0;

  return {
    topic: topic.id,
    sort: topic.sort,
    route: "responses",
    ok,
    ...(ok
      ? {}
      : {
          durationMs: totalMs,
          errorCode: errorCodes.length === 1 ? errorCodes[0] : "all_batches_failed",
        }),
    model: DEFAULT_MODEL,
    effort: opts.effort,
    maxToolCalls: opts.maxToolCalls,
    targetCount: opts.targetCount,
    batchStrategy: opts.batchStrategy,
    batchCount: opts.batchCount,
    staggerMs: opts.batchStrategy === "staggered" ? opts.staggerMs : 0,
    targetTotal: opts.targetCount * opts.batchCount,
    firstDisplayMs: thresholdCompletionMs(completions, 1),
    firstUsefulMs: thresholdCompletionMs(completions, USEFUL_GALLERY_COUNT),
    first18Ms: thresholdCompletionMs(completions, 18),
    first24Ms: thresholdCompletionMs(completions, 24),
    first30Ms: thresholdCompletionMs(completions, 30),
    totalMs,
    successfulBatches: rows.filter((row) => row.ok).length,
    candidateCount,
    uniqueMediaCount: validCount,
    duplicateCount: withinBatchDuplicates + crossBatchDuplicates,
    crossBatchDuplicateCount: crossBatchDuplicates,
    allowedMediaCount: opts.probe
      ? rows.reduce((sum, row) => sum + (row.allowedMediaCount ?? 0), 0)
      : validCount,
    reachableCount: opts.probe ? validCount : null,
    canonicalPostCount: rows.reduce((sum, row) => sum + (row.canonicalPostCount ?? 0), 0),
    probeMs: rows.reduce((sum, row) => sum + (row.probeMs ?? 0), 0),
    xSearchCalls: rows.reduce((sum, row) => sum + (row.xSearchCalls ?? 0), 0),
    toolOverrunBatches: rows.filter((row) => row.toolBudgetExceeded).length,
    inputTokens: rows.reduce((sum, row) => sum + (row.inputTokens ?? 0), 0),
    outputTokens: rows.reduce((sum, row) => sum + (row.outputTokens ?? 0), 0),
    batches: completions
      .sort((a, b) => a.batchIndex - b.batchIndex)
      .map((completion) => sanitizedBatchResult(completion, opts.probe)),
  };
}

function resolveGrokPath() {
  if (process.env.GROK_CLI_PATH) return process.env.GROK_CLI_PATH;
  if (process.platform === "win32") return join(homedir(), ".grok", "bin", "grok.exe");
  return "grok";
}

function runChild(cliPath, args, timeoutMs) {
  return new Promise((resolve) => {
    const child = spawn(cliPath, args, {
      windowsHide: true,
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    let settled = false;
    child.stdout.setEncoding("utf8");
    child.stderr.setEncoding("utf8");
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
    });
    const finish = (value) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolve(value);
    };
    const timer = setTimeout(() => {
      if (process.platform === "win32" && child.pid) {
        spawnSync("taskkill", ["/PID", String(child.pid), "/T", "/F"], {
          windowsHide: true,
          stdio: "ignore",
        });
      } else {
        child.kill("SIGKILL");
      }
      finish({ timedOut: true, code: null, stdout: "", stderr: "" });
    }, timeoutMs);
    child.on("error", () => finish({ timedOut: false, code: null, stdout: "", stderr: "" }));
    child.on("close", (code) => finish({ timedOut: false, code, stdout, stderr }));
  });
}

async function runCli(topic, opts) {
  const started = Date.now();
  const cliResult = await runChild(
    resolveGrokPath(),
    [
      "-p",
      cliPromptFor(topic, opts.targetCount),
      "--always-approve",
      "--max-turns",
      "14",
      "--effort",
      "low",
      "--json-schema",
      JSON.stringify(cliGallerySchema(opts.targetCount)),
      "--output-format",
      "json",
    ],
    DEFAULT_CLI_TIMEOUT_MS,
  );
  const searchMs = Date.now() - started;
  if (cliResult.timedOut) {
    return {
      topic: topic.id,
      sort: topic.sort,
      route: "cli",
      ok: false,
      targetCount: opts.targetCount,
      durationMs: searchMs,
      errorCode: "timeout",
    };
  }
  if (cliResult.code !== 0 || !cliResult.stdout.trim()) {
    return {
      topic: topic.id,
      sort: topic.sort,
      route: "cli",
      ok: false,
      targetCount: opts.targetCount,
      durationMs: searchMs,
      errorCode: "process_failed",
    };
  }
  let envelope = null;
  try {
    envelope = JSON.parse(cliResult.stdout);
  } catch {
    // Product code also accepts bare/fenced/concatenated model output.
  }
  const items = findGallery(envelope) ?? galleryFromModelText(cliResult.stdout) ?? [];
  if (!items.length) {
    return {
      topic: topic.id,
      sort: topic.sort,
      route: "cli",
      ok: false,
      targetCount: opts.targetCount,
      durationMs: searchMs,
      errorCode: "empty",
    };
  }
  const quality = await qualityMetrics(items, opts.probe);
  const result = {
    topic: topic.id,
    sort: topic.sort,
    route: "cli",
    ok: true,
    effort: "low",
    targetCount: opts.targetCount,
    searchMs,
    totalMs: searchMs + quality.metrics.probeMs,
    inputTokens: Number.isFinite(envelope?.usage?.input_tokens)
      ? envelope.usage.input_tokens
      : null,
    outputTokens: Number.isFinite(envelope?.usage?.output_tokens)
      ? envelope.usage.output_tokens
      : null,
    ...quality.metrics,
  };
  const validCount = opts.probe
    ? quality.metrics.reachableCount
    : quality.metrics.allowedMediaCount;
  return validCount === 0
    ? { ...result, ok: false, durationMs: result.totalMs, errorCode: "no_valid_images" }
    : result;
}

function percentile(values, fraction) {
  if (!values.length) return null;
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.max(0, Math.ceil(sorted.length * fraction) - 1)];
}

function aggregate(route, rows) {
  const matching = rows.filter((row) => row.route === route);
  const success = matching.filter((row) => row.ok);
  const batches = matching.flatMap((row) => row.batches ?? []);
  const totals = success.map((row) => row.totalMs);
  const validCounts = success.map((row) => row.reachableCount ?? row.allowedMediaCount);
  const firstDisplayTimes = matching
    .map((row) => row.firstDisplayMs)
    .filter(Number.isFinite);
  const firstUsefulTimes = matching
    .map((row) => row.firstUsefulMs)
    .filter(Number.isFinite);
  const citations = matching.reduce((sum, row) => sum + (row.canonicalPostCount ?? 0), 0);
  const candidates = matching.reduce((sum, row) => sum + (row.candidateCount ?? 0), 0);
  const validImages = matching.reduce(
    (sum, row) => sum + (row.reachableCount ?? row.allowedMediaCount ?? 0),
    0,
  );
  const inputTokens = matching.reduce((sum, row) => sum + (row.inputTokens ?? 0), 0);
  const outputTokens = matching.reduce((sum, row) => sum + (row.outputTokens ?? 0), 0);
  return {
    route,
    samples: matching.length,
    successes: success.length,
    successRate: matching.length ? success.length / matching.length : 0,
    batchSamples: batches.length,
    batchSuccessRate: batches.length
      ? batches.filter((batch) => batch.ok).length / batches.length
      : null,
    latencyPopulation: "successful_samples",
    p50Ms: percentile(totals, 0.5),
    p95Ms: percentile(totals, 0.95),
    p50FirstDisplayMs: percentile(firstDisplayTimes, 0.5),
    p95FirstDisplayMs: percentile(firstDisplayTimes, 0.95),
    p50FirstUsefulMs: percentile(firstUsefulTimes, 0.5),
    p95FirstUsefulMs: percentile(firstUsefulTimes, 0.95),
    medianValidImages: percentile(validCounts, 0.5),
    atLeast6Rate: matching.length
      ? matching.filter((row) => (row.reachableCount ?? row.allowedMediaCount ?? 0) >= 6).length /
        matching.length
      : 0,
    atLeast18Rate: matching.length
      ? matching.filter((row) => (row.reachableCount ?? row.allowedMediaCount ?? 0) >= 18).length /
        matching.length
      : 0,
    atLeast24Rate: matching.length
      ? matching.filter((row) => (row.reachableCount ?? row.allowedMediaCount ?? 0) >= 24).length /
        matching.length
      : 0,
    atLeast30Rate: matching.length
      ? matching.filter((row) => (row.reachableCount ?? row.allowedMediaCount ?? 0) >= 30).length /
        matching.length
      : 0,
    totalCandidateCount: candidates,
    totalValidImages: validImages,
    validImageRate: candidates ? validImages / candidates : null,
    canonicalCitationRate: candidates ? citations / candidates : null,
    totalInputTokens: inputTokens,
    totalOutputTokens: outputTokens,
    errorCounts: Object.fromEntries(
      [...new Set(matching.filter((row) => !row.ok).map((row) => row.errorCode))].map((code) => [
        code,
        matching.filter((row) => !row.ok && row.errorCode === code).length,
      ]),
    ),
    batchErrorCounts: Object.fromEntries(
      [...new Set(batches.filter((batch) => !batch.ok).map((batch) => batch.errorCode))].map(
        (code) => [
          code,
          batches.filter((batch) => !batch.ok && batch.errorCode === code).length,
        ],
      ),
    ),
  };
}

async function main() {
  const opts = parseArgs(process.argv.slice(2));
  if (opts.help) {
    process.stdout.write(`${usage()}\n`);
    return;
  }
  const topics = selectTopics(opts);
  const routes = opts.route === "both" ? ["responses", "cli"] : [opts.route];
  const token = routes.includes("responses") ? await readBuildOauth() : null;
  const results = [];
  for (const topic of topics) {
    for (const route of routes) {
      const row =
        route === "responses"
          ? opts.batchStrategy === "single"
            ? (await runResponses(topic, opts, token)).row
            : await runResponsesBatchStrategy(topic, opts, token)
          : await runCli(topic, opts);
      results.push(row);
      process.stdout.write(`${JSON.stringify({ type: "sample", ...row })}\n`);
    }
  }
  const summary = routes.map((route) => aggregate(route, results));
  process.stdout.write(
    `${JSON.stringify({
      type: "summary",
      generatedAt: new Date().toISOString(),
      topics: topics.map((topic) => topic.id),
      probeEnabled: opts.probe,
      batchStrategy: opts.batchStrategy,
      batchCount: opts.batchStrategy === "single" ? 1 : opts.batchCount,
      staggerMs: opts.batchStrategy === "staggered" ? opts.staggerMs : null,
      allowToolOverrun: opts.allowToolOverrun,
      routes: summary,
    })}\n`,
  );
}

main().catch((error) => {
  process.stdout.write(
    `${JSON.stringify({ type: "fatal", errorCode: safeErrorCategory(error) })}\n`,
  );
  process.exitCode = 1;
});
