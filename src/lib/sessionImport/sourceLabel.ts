import type { SourceRegistration } from "@/lib/api/sessionMigration";

export function registrationLabel(source: SourceRegistration) {
  return [source.provider, source.profile, source.rootPath, source.sourceId].filter(Boolean).join(" · ");
}

/** Keep choices readable; the selected installation's full identity is shown below. */
export function registrationShortLabel(source: SourceRegistration) {
  const name = source.profile.trim() || source.rootPath.split(/[\\/]/).filter(Boolean).at(-1) || source.sourceId;
  const chars = Array.from(name);
  const shortName = chars.length > 48 ? chars.slice(0, 47).join("") + "…" : name;
  const id = source.sourceId.length > 12 ? "…" + source.sourceId.slice(-12) : source.sourceId;
  return [source.provider, shortName, id].filter(Boolean).join(" · ");
}

export function sourceIdentityLabel(
  identity: { sourceId: string; provider: string } | null | undefined,
  registrations: SourceRegistration[],
) {
  if (!identity) return null;
  const registered = registrations.find((row) => row.sourceId === identity.sourceId && row.provider === identity.provider);
  return registered ? registrationLabel(registered) : `${identity.provider} · ${identity.sourceId}`;
}

/** Display only: Windows verbatim prefixes (`\\?\`, `\\?\UNC\`) are a Host
 * spelling, not part of the folder the user recognizes. */
export function displaySourcePath(path: string): string {
  if (path.startsWith("\\\\?\\UNC\\")) return "\\\\" + path.slice(8);
  if (path.startsWith("\\\\?\\")) return path.slice(4);
  return path;
}
