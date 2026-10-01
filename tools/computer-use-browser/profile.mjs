const SEGMENT = /^[A-Za-z0-9][A-Za-z0-9_-]*$/;

export function privateSegment(value, label = "identifier", maxLength = 64) {
  const raw = String(value ?? "");
  if (!raw || raw.length > maxLength || !SEGMENT.test(raw)) {
    throw new Error(`invalid ${label}`);
  }
  return raw;
}

export function profileName(value) {
  return privateSegment(value, "profile", 64);
}

export function runOwner(value) {
  return privateSegment(value, "owner", 128);
}

function invalidPreferences() {
  return workerException(409, "profile_preferences_invalid",
    "managed browser preferences cannot be safely prepared");
}

function record(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

// Call only before launching an App-owned profile. Never touch an existing-tab
// browser. Windows Chromium 130 otherwise downloads Hunspell dictionaries into
// its binary directory, violating the pinned runtime's immutable tree.
export function prepareManagedPreferences(profileDir) {
  if (!isAbsolute(profileDir)) throw invalidPreferences();
  const directory = join(profileDir, "Default");
  for (const path of [profileDir, directory]) {
    if (existsSync(path)) {
      const info = lstatSync(path);
      if (!info.isDirectory() || info.isSymbolicLink()) throw invalidPreferences();
    }
  }
  mkdirSync(directory, { recursive: true });
  const file = join(directory, "Preferences");
  let preferences = {};
  if (existsSync(file)) {
    const info = lstatSync(file);
    if (!info.isFile() || info.isSymbolicLink() || info.size > 4 * 1024 * 1024) {
      throw invalidPreferences();
    }
    try { preferences = JSON.parse(readFileSync(file, "utf8")); }
    catch { throw invalidPreferences(); }
  }
  if (!record(preferences) || [preferences.browser, preferences.spellcheck]
    .some(value => value !== undefined && !record(value))) throw invalidPreferences();
  const prepared = {
    ...preferences,
    browser: { ...preferences.browser, enable_spellchecking: false },
    spellcheck: { ...preferences.spellcheck, dictionary: "", dictionaries: [],
      use_spelling_service: false },
  };
  const temporary = join(directory, `.cu-preferences-${randomUUID()}.tmp`);
  const fd = openSync(temporary, "wx", 0o600);
  try {
    try { writeFileSync(fd, JSON.stringify(prepared)); }
    finally { closeSync(fd); }
    renameSync(temporary, file);
  } finally {
    if (existsSync(temporary)) unlinkSync(temporary);
  }
}
import { randomUUID } from "node:crypto";
import { closeSync, existsSync, lstatSync, mkdirSync, openSync, readFileSync,
  renameSync, unlinkSync, writeFileSync } from "node:fs";
import { isAbsolute, join } from "node:path";
import { workerException } from "./worker-errors.mjs";
