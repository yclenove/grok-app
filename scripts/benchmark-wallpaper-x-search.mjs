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

const GALLERY_SCHEMA = {
  type: "object",
  properties: {
    items: {
      type: "array",
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

// Keep this permissive shape aligned with the current Rust product path.
const CLI_GALLERY_SCHEMA = {
  type: "object",
  properties: {
    items: {
      type: "array",
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

function parseArgs(argv) {
  const opts = {
    route: "responses",
    effort: DEFAULT_EFFORT,
    maxToolCalls: DEFAULT_MAX_TOOL_CALLS,
    limit: TOPICS.length,
    topic: null,
    probe: true,
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
    else if (arg === "--max-tool-calls") opts.maxToolCalls = Number(next());
    else if (arg === "--limit") opts.limit = Number(next());
    else if (arg === "--topic") opts.topic = next();
    else if (arg === "--no-probe") opts.probe = false;
    else if (arg === "--help" || arg === "-h") opts.help = true;
    else throw new Error(`unknown argument: ${arg}`);
  }
  if (!new Set(["responses", "cli", "both"]).has(opts.route)) {
    throw new Error("--route must be responses, cli, or both");
  }
  if (!new Set(["low", "medium"]).has(opts.effort)) {
    throw new Error("--effort must be low or medium");
  }
  if (!Number.isInteger(opts.maxToolCalls) || opts.maxToolCalls < 1 || opts.maxToolCalls > 3) {
    throw new Error("--max-tool-calls must be an integer from 1 to 3");
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
    "  --max-tool-calls 1..3  Responses only (default 3)",
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

function responsesPromptFor(topic, maxToolCalls) {
  const sort = topic.sort === "latest" ? "Latest" : "Top";
  return `You collect high-quality still images from X (Twitter) for a wallpaper picker.

User topic: ${topic.query}
Sort preference: ${sort}

Use X search only. Use no more than ${maxToolCalls} x_search ${maxToolCalls === 1 ? "call" : "calls"}. Search useful query variants with image/media filters. Prefer real photography or polished AI art suitable as wallpaper. Prefer posts that include both a prompt and attached images when relevant. Skip memes, screenshots, text cards, avatars, emoji packs, ads, blurry thumbnails, and placeholder links.

Return 8-16 distinct items when possible. fullUrl must be a direct full-size media CDN URL, preferably https://pbs.twimg.com/media/... with name=orig. postUrl must be a confirmed canonical https://x.com/<user>/status/<id>; omit it rather than guessing. Return metadata only and do not download files.`;
}

function cliPromptFor(topic) {
  const sort = topic.sort === "latest" ? "Latest" : "Top";
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
7. Return exactly ONE JSON object matching the schema (items array, 12-28 when possible). No prose, no second JSON object, no placeholder.jpg.

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
  const reachable = shouldProbe
    ? (await mapConcurrent(allowed, PROBE_CONCURRENCY, probeOne)).filter(Boolean).length
    : null;
  return {
    candidateCount: rawItems.length,
    uniqueMediaCount: uniqueMedia.length,
    duplicateCount: Math.max(0, media.length - uniqueMedia.length),
    allowedMediaCount: allowed.length,
    reachableCount: reachable,
    canonicalPostCount: rawItems.filter((item) =>
      canonicalPostUrl(item?.postUrl ?? item?.post_url ?? item?.statusUrl ?? ""),
    ).length,
    probeMs: shouldProbe ? Date.now() - probeStarted : 0,
  };
}

async function runResponses(topic, opts, token) {
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
          input: responsesPromptFor(topic, opts.maxToolCalls),
          tools: [{ type: "x_search" }],
          tool_choice: "auto",
          max_tool_calls: opts.maxToolCalls,
          reasoning: { effort: opts.effort, summary: "concise" },
          text: {
            format: {
              type: "json_schema",
              name: "wallpaper_gallery",
              strict: true,
              schema: GALLERY_SCHEMA,
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
    const quality = await qualityMetrics(items, opts.probe);
    const xSearchCalls = (body.output ?? []).filter((item) =>
      /(?:x_?search|custom_tool)/i.test(String(item?.type ?? "")),
    ).length;
    const result = {
      topic: topic.id,
      sort: topic.sort,
      route: "responses",
      ok: true,
      model: typeof body.model === "string" ? body.model : DEFAULT_MODEL,
      effort: opts.effort,
      maxToolCalls: opts.maxToolCalls,
      xSearchCalls,
      searchMs,
      totalMs: searchMs + quality.probeMs,
      inputTokens: Number.isFinite(body?.usage?.input_tokens) ? body.usage.input_tokens : null,
      outputTokens: Number.isFinite(body?.usage?.output_tokens) ? body.usage.output_tokens : null,
      ...quality,
    };
    const validCount = opts.probe ? quality.reachableCount : quality.allowedMediaCount;
    return validCount === 0
      ? { ...result, ok: false, durationMs: result.totalMs, errorCode: "no_valid_images" }
      : result;
  } catch (error) {
    return {
      topic: topic.id,
      sort: topic.sort,
      route: "responses",
      ok: false,
      effort: opts.effort,
      maxToolCalls: opts.maxToolCalls,
      durationMs: Date.now() - started,
      errorCode: safeErrorCategory(error),
    };
  }
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
      cliPromptFor(topic),
      "--always-approve",
      "--max-turns",
      "14",
      "--effort",
      "low",
      "--json-schema",
      JSON.stringify(CLI_GALLERY_SCHEMA),
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
    searchMs,
    totalMs: searchMs + quality.probeMs,
    inputTokens: Number.isFinite(envelope?.usage?.input_tokens)
      ? envelope.usage.input_tokens
      : null,
    outputTokens: Number.isFinite(envelope?.usage?.output_tokens)
      ? envelope.usage.output_tokens
      : null,
    ...quality,
  };
  const validCount = opts.probe ? quality.reachableCount : quality.allowedMediaCount;
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
  const totals = success.map((row) => row.totalMs);
  const validCounts = success.map((row) => row.reachableCount ?? row.allowedMediaCount);
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
    latencyPopulation: "successful_samples",
    p50Ms: percentile(totals, 0.5),
    p95Ms: percentile(totals, 0.95),
    medianValidImages: percentile(validCounts, 0.5),
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
          ? await runResponses(topic, opts, token)
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
