import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import {
  HOST_ONLY_TOOLS,
  MODEL_TOOLS,
  PROTOCOL_VERSION,
  TOOLS,
  validateActionRequest,
  validateNavigateUrl,
} from "./protocol.mjs";

const golden = join(dirname(fileURLToPath(import.meta.url)), "../computer-use-protocol/golden");
const catalog = JSON.parse(readFileSync(join(golden, "catalog.json"), "utf8"));

function hasRequired(value, required = []) {
  return required.every((key) => Object.hasOwn(value, key));
}

function matchesPresenceRule(rule, value) {
  if (!hasRequired(value, rule.required)) return false;
  if (rule.not && matchesPresenceRule(rule.not, value)) return false;
  if (rule.allOf && !rule.allOf.every((entry) => matchesPresenceRule(entry, value))) return false;
  return true;
}

function acceptsObjectSchema(schema, value) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  if (!hasRequired(value, schema.required)) return false;
  if (schema.additionalProperties === false && Object.keys(value).some((key) => !(key in schema.properties))) return false;
  for (const [key, entry] of Object.entries(schema.properties)) {
    if (!Object.hasOwn(value, key)) continue;
    if (entry.type === "string" && (typeof value[key] !== "string" || value[key].length < (entry.minLength ?? 0) || value[key].length > (entry.maxLength ?? Infinity))) return false;
    if (entry.type === "integer" && (!Number.isInteger(value[key]) || value[key] < (entry.minimum ?? -Infinity) || value[key] > (entry.maximum ?? Infinity))) return false;
    if (Object.hasOwn(entry, "const") && value[key] !== entry.const) return false;
  }
  if (schema.oneOf && schema.oneOf.filter((branch) => matchesPresenceRule(branch, value)).length !== 1) return false;
  return true;
}

test("MCP tool catalog matches golden", () => {
  assert.equal(PROTOCOL_VERSION, catalog.protocolVersion);
  assert.deepEqual(MODEL_TOOLS, catalog.modelTools);
  assert.deepEqual(TOOLS.map((t) => t.name), catalog.modelTools);
  for (const name of catalog.hostOnlyTools) {
    assert.equal(HOST_ONLY_TOOLS.includes(name), true);
    assert.equal(MODEL_TOOLS.includes(name), false);
  }
  const wait = TOOLS.find((t) => t.name === "computer_wait");
  assert.equal(wait.inputSchema.properties.timeoutMs.minimum, catalog.waitTimeoutMsMin);
  assert.equal(wait.inputSchema.properties.timeoutMs.maximum, catalog.waitTimeoutMsMax);
  for (const field of ["version", "runId", "actionId", "targetId", "targetGeneration", "snapshotId", "geometryRevision", "elementRef", "nameEquals"]) {
    assert.equal(wait.inputSchema.required.includes(field), true, `wait requires ${field}`);
  }
  assert.equal(wait.inputSchema.additionalProperties, false);
  const observe = TOOLS.find((t) => t.name === "computer_observe");
  assert.match(observe.description, /observe/i);
  assert.match(observe.description, /image/i);
  const act = TOOLS.find((t) => t.name === "computer_act");
  assert.match(act.description, /unknown/i);
  assert.match(act.description, /observ/i);
  assert.ok(TOOLS.some((t) => t.name === "computer_stop"));
  assert.equal(TOOLS.some((t) => t.name === "computer_authorize"), false);
  assert.equal(TOOLS.some((t) => t.name === "computer_resume"), false);
});

test("model catalog does not expose private browser routing or locators", () => {
  for (const name of ["computer_evaluate", "computer_authorize", "computer_resume"]) {
    assert.equal(MODEL_TOOLS.includes(name), false, name);
  }
  for (const name of ["browser_list_tabs", "browser_open", "browser_observe", "browser_act"]) {
    assert.equal(MODEL_TOOLS.includes(name), true, name);
  }
  const schemas = TOOLS.map((t) => JSON.stringify(t.inputSchema)).join("\n");
  assert.equal(schemas.includes('"pageId"'), false);
  assert.equal(schemas.includes('"profile"'), false);
  assert.equal(schemas.includes('"selector"'), false);
  assert.equal(schemas.includes('"evaluate"'), false);
  const observe = TOOLS.find((t) => t.name === "browser_observe");
  assert.match(observe.description, /image/i);
  assert.match(observe.description, /png/i);
  const act = TOOLS.find((t) => t.name === "browser_act");
  assert.match(act.description, /unknown/i);
  assert.match(act.description, /cookie/i);
  assert.match(act.description, /resume/i);
});

test("model can select a grant explicitly without inventing snapshot identity", () => {
  for (const name of ["computer_status", "computer_list_targets", "computer_open_target",
    "computer_observe", "computer_request_handoff", "computer_stop", "computer_navigate",
    "computer_download", "browser_list_tabs", "browser_open", "browser_observe", "browser_act"]) {
    const schema = TOOLS.find((tool) => tool.name === name).inputSchema;
    for (const field of ["runId", "targetId"]) {
      assert.equal(schema.properties[field].type, "string", `${name}: ${field}`);
      assert.equal(schema.properties[field].maxLength, catalog.idMaxLen, name);
    }
    assert.equal(schema.required.includes("runId"), false, `${name}: single-grant compatibility`);
    assert.equal(schema.additionalProperties, false, name);
  }
  const observe = TOOLS.find((tool) => tool.name === "computer_observe").inputSchema;
  assert.equal("snapshotId" in observe.properties, false);
  assert.deepEqual(observe.required, []);
  const access = TOOLS.find((tool) => tool.name === "computer_request_access");
  for (const branch of access.inputSchema.oneOf) {
    assert.equal("runId" in branch.properties, false);
    assert.equal("targetId" in branch.properties, false);
  }
});

test("computer_navigate keeps managed and App WebView identities disjoint", () => {
  const schema = TOOLS.find((tool) => tool.name === "computer_navigate").inputSchema;
  assert.equal(schema.additionalProperties, false);
  assert.deepEqual(schema.required, ["actionId"]);
  assert.deepEqual(Object.keys(schema.properties).sort(), [
    "actionId", "kind", "pageGeneration", "runId", "tabId", "targetGeneration",
    "targetId", "url",
  ]);
  assert.equal(schema.oneOf.length, 3);

  const [managed, webviewUrl, webviewHistory] = schema.oneOf;
  assert.deepEqual(managed.required, ["tabId", "url", "pageGeneration"]);
  assert.deepEqual(managed.properties.kind, { const: "url" });
  assert.deepEqual(managed.not, { required: ["targetGeneration"] });

  assert.deepEqual(webviewUrl.required, ["targetGeneration", "url"]);
  assert.deepEqual(webviewUrl.properties.kind, { const: "url" });
  assert.deepEqual(
    webviewUrl.allOf.map((rule) => rule.not.required[0]),
    ["tabId", "pageGeneration"],
  );

  assert.deepEqual(webviewHistory.required, ["targetGeneration", "kind"]);
  assert.deepEqual(webviewHistory.properties.kind.enum, ["reload", "back", "forward"]);
  assert.deepEqual(
    webviewHistory.allOf.map((rule) => rule.not.required[0]),
    ["url", "tabId", "pageGeneration"],
  );
});

test("computer_download keeps managed and App WebView identities disjoint", () => {
  const schema = TOOLS.find((tool) => tool.name === "computer_download").inputSchema;
  assert.equal(schema.additionalProperties, false);
  assert.deepEqual(schema.required, ["actionId", "snapshotId", "elementRef"]);
  assert.deepEqual(Object.keys(schema.properties).sort(), [
    "actionId", "elementRef", "filename", "pageGeneration", "runId", "snapshotId",
    "tabId", "targetGeneration", "targetId",
  ]);
  assert.equal(schema.oneOf.length, 2);
  const [managed, webview] = schema.oneOf;
  assert.deepEqual(managed.required, ["tabId", "pageGeneration"]);
  assert.deepEqual(managed.not, { required: ["targetGeneration"] });
  assert.deepEqual(webview.required, ["runId", "targetId", "targetGeneration"]);
  assert.deepEqual(
    webview.allOf.map((rule) => rule.not.required[0]),
    ["tabId", "pageGeneration"],
  );

  const common = { actionId: "download-1", snapshotId: "snapshot-1", elementRef: "node-1" };
  assert.equal(acceptsObjectSchema(schema, { ...common, tabId: "tab-1", pageGeneration: 2 }), true);
  assert.equal(acceptsObjectSchema(schema, {
    ...common,
    runId: "run-1",
    targetId: "wv|resource|native|1|session|run",
    targetGeneration: 3,
  }), true);
  assert.equal(acceptsObjectSchema(schema, {
    ...common,
    runId: "run-1",
    targetId: "wv|resource|native|1|session|run",
    targetGeneration: 3,
    tabId: "tab-1",
    pageGeneration: 2,
  }), false, "mixed managed/WebView fields must be rejected");
  assert.equal(acceptsObjectSchema(schema, {
    ...common,
    targetId: "wv|resource|native|1|session|run",
    targetGeneration: 3,
  }), false, "App WebView download requires an explicit runId");
  assert.equal(acceptsObjectSchema(schema, {
    ...common,
    runId: "r".repeat(catalog.idMaxLen + 1),
    targetId: "wv|resource|native|1|session|run",
    targetGeneration: 3,
  }), false, "identity length remains bounded");
});

test("MCP validator accepts golden valid actions", () => {
  const dir = join(golden, "actions/valid");
  const files = readdirSync(dir).filter((f) => f.endsWith(".json"));
  assert.ok(files.length >= 7);
  for (const file of files) {
    const body = JSON.parse(readFileSync(join(dir, file), "utf8"));
    assert.equal(validateActionRequest(body), null, file);
  }
});

test("upload is a strict versioned original file-field intent, not a local-file API", () => {
  const tool = TOOLS.find(tool => tool.name === "computer_upload");
  const schema = tool.inputSchema;
  const value = { version: 1, actionId: "upload", runId: "run", targetId: "wv|page", targetGeneration: 1,
    snapshotId: "snapshot", geometryRevision: 0, elementRef: "file" };
  assert.equal(acceptsObjectSchema(schema, value), true);
  assert.deepEqual(Object.keys(schema.properties).sort(), Object.keys(value).sort());
  assert.deepEqual([...schema.required].sort(), Object.keys(value).sort());
  for (const key of Object.keys(value)) {
    const missing = { ...value }; delete missing[key];
    assert.equal(acceptsObjectSchema(schema, missing), false, key);
  }
  for (const key of ["path", "filename", "bytes", "mime", "url", "selector", "script", "tabId", "pageGeneration", "approve"]) {
    assert.equal(acceptsObjectSchema(schema, { ...value, [key]: "forged" }), false, key);
  }
  assert.equal(acceptsObjectSchema(schema, { ...value, version: 2 }), false);
  assert.equal(acceptsObjectSchema(schema, { ...value, geometryRevision: Number.MAX_SAFE_INTEGER + 1 }), false);
  assert.match(tool.description, /original call waits/i);
  assert.match(tool.description, /assignment only/i);
  assert.match(tool.description, /other surfaces are unavailable/i);
});

test("explicit task completion requires only the original Host taskId", () => {
  const tool = TOOLS.find((tool) => tool.name === "computer_complete_task");
  assert.deepEqual(tool.inputSchema.required, ["taskId"]);
  assert.deepEqual(Object.keys(tool.inputSchema.properties), ["taskId"]);
  assert.equal(tool.inputSchema.properties.taskId.maxLength, catalog.idMaxLen);
  assert.equal(tool.inputSchema.additionalProperties, false);
  assert.match(tool.description, /verif/i);
  assert.match(tool.description, /physical cleanup/i);
  assert.match(tool.description, /waiting for approval/i);
  assert.match(tool.description, /successor task/i);
});

test("consent readback never instructs the model to poll or execute while pending", () => {
  const status = TOOLS.find((tool) => tool.name === "computer_status");
  const request = TOOLS.find((tool) => tool.name === "computer_request_access");
  assert.equal(acceptsObjectSchema(status.inputSchema, {}), true);
  assert.equal(acceptsObjectSchema(status.inputSchema, { screenshot: true }), false);
  assert.equal(acceptsObjectSchema(status.inputSchema, { runId: null }), false);
  assert.match(status.description, /authority:none/i);
  assert.match(status.description, /explicit runId\/targetId always require the approved scope/i);
  assert.match(status.description, /end this turn and wait/i);
  assert.match(request.description, /do not poll computer_status/i);
  assert.match(request.description, /call execution tools while awaiting approval/i);
});

test("MCP validator rejects golden invalid actions", () => {
  const dir = join(golden, "actions/invalid");
  const files = readdirSync(dir).filter((f) => f.endsWith(".json"));
  assert.ok(files.length >= 9);
  for (const file of files) {
    const fixture = JSON.parse(readFileSync(join(dir, file), "utf8"));
    const err = validateActionRequest(fixture.body);
    assert.ok(err, file);
    assert.ok(String(err).toLowerCase().includes(String(fixture.errorContains).toLowerCase()), `${file}: ${err}`);
  }
});

test("MCP validator rejects illegal URLs", () => {
  const rows = JSON.parse(readFileSync(join(golden, "urls/invalid.json"), "utf8"));
  for (const row of rows) {
    const err = validateNavigateUrl(row.url);
    assert.ok(err, row.url);
    assert.ok(String(err).toLowerCase().includes(String(row.errorContains).toLowerCase()), `${row.url}: ${err}`);
  }
  assert.equal(validateNavigateUrl("https://example.com/a"), null);
});
