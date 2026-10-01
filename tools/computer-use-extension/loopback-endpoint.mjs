export function parseEndpoint(value) {
  if (typeof value !== "string" || !/^http:\/\/127\.0\.0\.1:[1-9]\d{0,4}$/.test(value)) {
    throw new Error("invalidAddress");
  }
  const url = new URL(value);
  if (!url.port || Number(url.port) > 65535) throw new Error("invalidAddress");
  return url.origin;
}
