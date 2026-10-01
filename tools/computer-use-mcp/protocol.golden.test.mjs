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

test("MCP validator accepts golden valid actions", () => {
  const dir = join(golden, "actions/valid");
  const files = readdirSync(dir).filter((f) => f.endsWith(".json"));
  assert.ok(files.length >= 7);
  for (const file of files) {
    const body = JSON.parse(readFileSync(join(dir, file), "utf8"));
    assert.equal(validateActionRequest(body), null, file);
  }
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
