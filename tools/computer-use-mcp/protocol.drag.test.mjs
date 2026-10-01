import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { validateActionRequest } from "./protocol.mjs";

const cases = JSON.parse(readFileSync(new URL("../computer-use-protocol/golden/drag-destinations.json", import.meta.url), "utf8"));
for (const source of [{ elementRef: "source-ref" }, { x: 1, y: 1 }]) {
  for (const row of cases) {
    test(`drag ${JSON.stringify(source)}: ${row.name}`, () => {
      const error = validateActionRequest({
        version: 1, actionId: "drag-contract", runId: "r", targetId: "t",
        targetGeneration: 1, snapshotId: "s", geometryRevision: 1,
        action: "drag", target: source, parameters: row.parameters,
      });
      assert.equal(error === null, row.valid, `${row.name}: ${error}`);
    });
  }
}
