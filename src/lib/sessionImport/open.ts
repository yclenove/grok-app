/** Domain bus so AppWorkbench does not grow import state. */

export const OPEN_IMPORT_EVENT = "grok:open-session-import";
export const OPEN_IMPORTED_EVENT = "grok:open-imported-history";

export type OpenImportDetail = {
  mode?: "wizard" | "file";
  projectId?: string | null;
};

export type OpenImportedDetail = {
  importId: string;
  revision?: string | null;
  leaf?: string | null;
  anchorEventId?: string | null;
  /** Open the continuation draft after the reader. */
  continue?: boolean;
};

function decodeHashPart(value: string): string | null {
  try {
    return decodeURIComponent(value);
  } catch {
    return null;
  }
}

/** `#/imported/<imp:uuid>[/rev/<revision>][/leaf/<leaf>][/event/<event-id>]` */
export function parseImportedDeepLink(hash: string): OpenImportedDetail | null {
  const raw = hash.trim().replace(/^#/, "");
  const parts = raw.split("/").filter(Boolean);
  if (parts[0] !== "imported" || !parts[1]) return null;
  const importId = decodeHashPart(parts[1]);
  if (!importId) return null;
  if (!/^imp:[0-9a-f-]{36}$/i.test(importId)) return null;
  let revision: string | null = null;
  let leaf: string | null = null;
  let anchorEventId: string | null = null;
  for (let i = 2; i < parts.length; i += 2) {
    const key = parts[i];
    const value = parts[i + 1] ? decodeHashPart(parts[i + 1]) : "";
    if (value == null) return null;
    if (!value) continue;
    if (key === "rev") revision = value;
    if (key === "leaf") leaf = value;
    if (key === "event" && value.length <= 4096) anchorEventId = value;
  }
  return anchorEventId
    ? { importId, revision, leaf, anchorEventId }
    : { importId, revision, leaf };
}

export function importedHistoryHash(detail: OpenImportedDetail): string {
  let hash = `#/imported/${encodeURIComponent(detail.importId)}`;
  if (detail.revision) hash += `/rev/${encodeURIComponent(detail.revision)}`;
  if (detail.leaf) hash += `/leaf/${encodeURIComponent(detail.leaf)}`;
  if (detail.anchorEventId) hash += `/event/${encodeURIComponent(detail.anchorEventId)}`;
  return hash;
}

function sameImportedHash(current: string, next: string): boolean {
  const a = current.trim().replace(/^#/, "");
  const b = next.trim().replace(/^#/, "");
  return a === b;
}

export function openSessionImport(detail: OpenImportDetail = {}) {
  window.dispatchEvent(new CustomEvent(OPEN_IMPORT_EVENT, { detail }));
}

function pushImportedHistoryLocation(detail: OpenImportedDetail) {
  const hash = importedHistoryHash(detail);
  if (!sameImportedHash(window.location.hash || "", hash)) {
    const url = new URL(window.location.href);
    url.hash = hash.replace(/^#/, "#");
    window.history.pushState({ imported: detail }, "", `${url.pathname}${url.search}${hash}`);
  }
}

/** Persist a reader anchor without reopening the current reader instance. */
export function rememberImportedHistoryAnchor(detail: OpenImportedDetail) {
  pushImportedHistoryLocation(detail);
}

export function openImportedHistory(detail: OpenImportedDetail) {
  pushImportedHistoryLocation(detail);
  window.dispatchEvent(new CustomEvent(OPEN_IMPORTED_EVENT, { detail }));
}
