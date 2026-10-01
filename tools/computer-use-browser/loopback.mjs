/** Loopback-only clients. Used by the managed Playwright worker. */
export function clientAllowed(ra) {
  return ra === "127.0.0.1" || ra === "::1" || ra === "::ffff:127.0.0.1";
}
