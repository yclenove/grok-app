/** Minimal OS + explicit GROK_CU_* env for the production browser worker. */

const WINDOWS_KEYS = [
  "SystemRoot",
  "WINDIR",
  "TEMP",
  "TMP",
  "LOCALAPPDATA",
  "APPDATA",
  "USERPROFILE",
  "HOMEDRIVE",
  "HOMEPATH",
  "ProgramData",
  "ProgramFiles",
  "ProgramFiles(x86)",
  "CommonProgramFiles",
  "ComSpec",
  "PATHEXT",
  "PATH",
  "Path",
  "NUMBER_OF_PROCESSORS",
  "PROCESSOR_ARCHITECTURE",
];

const UNIX_KEYS = ["HOME", "TMPDIR", "TMP", "TEMP", "LANG", "LC_ALL", "USER", "LOGNAME", "PATH"];

export const FORBIDDEN_CHILD_ENV = [
  "OPENAI_API_KEY",
  "GROK_API_KEY",
  "HTTP_PROXY",
  "HTTPS_PROXY",
  "ALL_PROXY",
  "http_proxy",
  "https_proxy",
  "NODE_OPTIONS",
];

export function keepChildEnvKey(key) {
  if (!key) return false;
  if (FORBIDDEN_CHILD_ENV.includes(key)) return false;
  if (key.startsWith("GROK_CU_")) return true;
  const allow = process.platform === "win32" ? WINDOWS_KEYS : UNIX_KEYS;
  return allow.some((name) => name.toLowerCase() === key.toLowerCase());
}

export function childEnv(overrides = {}) {
  const env = {};
  for (const [key, value] of Object.entries(process.env)) {
    if (value != null && keepChildEnvKey(key)) env[key] = value;
  }
  for (const [key, value] of Object.entries(overrides)) {
    if (value != null && keepChildEnvKey(key)) env[key] = value;
  }
  return env;
}
