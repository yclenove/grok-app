import { cp, mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";

const args = process.argv.slice(2);
const valueAfter = (name, fallback) => {
  const index = args.indexOf(name);
  return index >= 0 ? args[index + 1] : fallback;
};
const profile = path.resolve(valueAfter("--profile", ".cu-probe-pref-installed"));
const source = path.resolve(valueAfter("--source", "tools/computer-use-extension"));
const extensionId = valueAfter("--id", "gldlkoglmaheognbfbhpjicmgeolknon");
const extensionKey = valueAfter(
  "--key",
  "MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEA2zJ0PlrXttuobFjd+ikcHdiZLEPLPWCrxzcMmLfCR656x8S5TYG6Fdtn2ZMFkwkF7aL3iVXcnzPDKQmKo1L4oP1eC72Xk48AVh4O5RPXXkdSPGsXlmKwh0IIJF/3Q2iumFXBqReFuGDaED2SVgJvZIWsOWEgYzsvuumNlmblEyGUBHifAQjsh7VtShd2eukLHRhn1kcyiZAXE/b7+Ljy0Azm9/h/Mi+NUHVVVKdFLp6zyuBC8I9MqySULyiQocvSkKM94qfBTfob2O6k3ig+WMKR4vcVCH7rFWLeg8OcIxJOV6kPFSYxTiNJ5CUEMLNMMcSx7HyiCS+jDPPGNcjPUQIDAQAB",
);
const version = "1.1.0";
const defaultDir = path.join(profile, "Default");
const installedDir = path.join(defaultDir, "Extensions", extensionId, version);
await mkdir(installedDir, { recursive: true });
await cp(source, installedDir, { recursive: true });
const manifestPath = path.join(installedDir, "manifest.json");
const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
manifest.key = extensionKey;
await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`, "utf8");

const preferencesPath = path.join(defaultDir, "Preferences");
const preferences = JSON.parse(await readFile(preferencesPath, "utf8"));
preferences.extensions ??= {};
preferences.extensions.settings ??= {};
const now = String((Date.now() + 11644473600000) * 1000);
preferences.extensions.settings[extensionId] = {
  active_permissions: {
    api: manifest.permissions ?? [],
    explicit_host: manifest.host_permissions ?? [],
    manifest_permissions: [],
    scriptable_host: [],
  },
  commands: {},
  content_settings: [],
  creation_flags: 1,
  events: [],
  first_install_time: now,
  from_webstore: false,
  incognito_content_settings: [],
  incognito_preferences: {},
  last_update_time: now,
  location: 4,
  manifest,
  path: installedDir,
  preferences: {},
  regular_only_preferences: {},
  state: 1,
  was_installed_by_default: false,
  was_installed_by_oem: false,
};
await writeFile(preferencesPath, `${JSON.stringify(preferences)}\n`, "utf8");
console.log(JSON.stringify({ profile, extensionId, installedDir }));
