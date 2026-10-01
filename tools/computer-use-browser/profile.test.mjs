import test from "node:test";
import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, realpathSync,
  rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { isAbsolute, join, relative } from "node:path";

import { prepareManagedPreferences, profileName, runOwner } from "./profile.mjs";

function ownedRoot(t) {
  const base = realpathSync(tmpdir());
  const root = mkdtempSync(join(base, "cu-profile-prefs-"));
  t.after(() => {
    const path = relative(base, root);
    assert(!isAbsolute(path) && path.startsWith("cu-profile-prefs-") && !path.includes(".."));
    rmSync(root, { recursive: true, force: true });
  });
  return root;
}

test("private profile names cannot traverse or alias directories", () => {
  for (const value of ["", ".", "..", "../outside", "a/b", "a\\b", ".hidden", "has space"])
    assert.throws(() => profileName(value), /invalid profile/);

  assert.equal(profileName("profile-1"), "profile-1");
  assert.equal(profileName("Work_2"), "Work_2");
});

test("run owner names cannot escape or share another run directory", () => {
  for (const value of ["", ".", "..", "../run-b", "run/a", "run\\a"])
    assert.throws(() => runOwner(value), /invalid owner/);

  assert.equal(runOwner("run-a"), "run-a");
});

test("managed launch disables dictionary downloads while preserving unrelated preferences", t => {
  const root = ownedRoot(t);
  const profile = join(root, "profile");
  prepareManagedPreferences(profile);
  const file = join(profile, "Default", "Preferences");
  const initial = JSON.parse(readFileSync(file, "utf8"));
  assert.equal(initial.browser.enable_spellchecking, false);
  assert.deepEqual(initial.spellcheck.dictionaries, []);
  assert.equal(initial.spellcheck.dictionary, "");
  assert.equal(initial.spellcheck.use_spelling_service, false);
  initial.browser.custom_option = true;
  initial.spellcheck.custom_option = "preserved";
  initial.appearance = { theme: "fixture" };
  initial.browser.enable_spellchecking = true;
  initial.spellcheck.dictionaries = ["en-US"];
  writeFileSync(file, JSON.stringify(initial));
  prepareManagedPreferences(profile);
  const prepared = JSON.parse(readFileSync(file, "utf8"));
  assert.deepEqual(prepared.appearance, initial.appearance);
  assert.equal(prepared.browser.custom_option, true);
  assert.equal(prepared.spellcheck.custom_option, "preserved");
  assert.equal(prepared.browser.enable_spellchecking, false);
  assert.deepEqual(prepared.spellcheck.dictionaries, []);
  assert.deepEqual(readdirSync(join(profile, "Default")), ["Preferences"]);
});

test("malformed, oversized or non-object preferences are rejected without overwriting them", t => {
  const root = ownedRoot(t);
  mkdirSync(join(root, "Default"));
  const file = join(root, "Default", "Preferences");
  for (const value of ["{broken", "null", "[]", '{"browser":1}', '{"spellcheck":[]}',
    " ".repeat(4 * 1024 * 1024 + 1)]) {
    writeFileSync(file, value);
    assert.throws(() => prepareManagedPreferences(root), error =>
      error.workerCode === "profile_preferences_invalid");
    assert.equal(readFileSync(file, "utf8"), value);
    assert.deepEqual(readdirSync(join(root, "Default")), ["Preferences"]);
  }
});

test("managed preferences refuse a linked Default directory", t => {
  const root = ownedRoot(t);
  const profile = join(root, "profile");
  const other = join(root, "other");
  mkdirSync(profile); mkdirSync(other);
  symlinkSync(other, join(profile, "Default"), process.platform === "win32" ? "junction" : "dir");
  assert.throws(() => prepareManagedPreferences(profile), error =>
    error.workerCode === "profile_preferences_invalid");
  assert.equal(existsSync(join(other, "Preferences")), false);
});
