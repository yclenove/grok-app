import test from "node:test";
import assert from "node:assert/strict";
import { runtimeLimits, treeBudget } from "./computer-use-runtime-limits.mjs";

const tiny = { maxEntries: 4, maxFiles: 2, maxFileBytes: 4, maxTotalBytes: 8, maxPathChars: 8 };

test("build-time ceilings are frozen and versioned", () => {
  assert.equal(runtimeLimits.schemaVersion, 1);
  assert(Object.isFrozen(runtimeLimits));
  for (const key of ["chromium", "playwright", "node"]) assert(Object.isFrozen(runtimeLimits[key]));
  assert.equal(runtimeLimits.chromium.maxTotalBytes, 1024 ** 3);
  assert.equal(runtimeLimits.chromium.maxFiles, 1024);
});

test("root directories and payloads accept exact independent boundaries", () => {
  const add = treeBudget(tiny);
  add(""); add("dir"); add("12345678", 4); add("b", 4);
});

test("one over any entry, file, single-file, total-byte or path bound fails", () => {
  assert.throws(() => treeBudget(tiny)("a", 5), /limit/);
  assert.throws(() => treeBudget(tiny)("123456789", 1), /path/);
  const entries = treeBudget(tiny);
  for (const path of ["", "a", "b", "c"]) entries(path);
  assert.throws(() => entries("d"), /entry/);
  const files = treeBudget(tiny);
  files("a", 0); files("b", 0);
  assert.throws(() => files("c", 0), /file/);
  const total = treeBudget({ ...tiny, maxTotalBytes: 7 });
  total("a", 4);
  assert.throws(() => total("b", 4), /byte/);
});

test("unsafe numbers, ambiguous paths and case aliases cannot enter inventory", () => {
  for (const size of [-1, NaN, Infinity, 1.5, Number.MAX_SAFE_INTEGER + 1]) {
    assert.throws(() => treeBudget(tiny)("a", size), /size/);
  }
  for (const path of ["../a", "a//b", "a\tb", "a\nb", "a\rb", "/a", "C:/a"]) {
    assert.throws(() => treeBudget(tiny)(path, 0), /path/);
  }
  const add = treeBudget(tiny); add("A", 0);
  assert.throws(() => add("a", 0), /duplicate/);
});
