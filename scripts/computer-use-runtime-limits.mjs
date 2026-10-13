/** Build-time contract also compiled into Core. Bundle manifests cannot override it. */
import { readFileSync } from "node:fs";
import { URL } from "node:url";

const contract = JSON.parse(readFileSync(new URL("./computer-use-runtime-limits.json", import.meta.url), "utf8"));
const fields = ["maxEntries", "maxFiles", "maxFileBytes", "maxTotalBytes", "maxPathChars", "maxArchiveBytes", "maxCompressionRatio"];
if (contract.schemaVersion !== 1 || Object.keys(contract).sort().join() !== "chromium,node,playwright,schemaVersion") {
  throw new Error("invalid compile-owned runtime limit schema");
}
for (const name of ["chromium", "playwright", "node"]) {
  const limits = contract[name];
  if (Object.keys(limits).sort().join() !== [...fields].sort().join()
      || !fields.every(field => Number.isSafeInteger(limits[field]) && limits[field] > 0)
      || limits.maxEntries < limits.maxFiles || limits.maxTotalBytes < limits.maxFileBytes) {
    throw new Error(`invalid compile-owned ${name} resource limits`);
  }
  Object.freeze(limits);
}
export const runtimeLimits = Object.freeze(contract);

export function treeBudget(limits) {
  let entries = 0, files = 0, bytes = 0;
  const seen = new Set();
  return (path, size) => {
    if ((path && (path.split("/").some(part => !part || part === "." || part === "..") || /[\0\t\r\n:]/.test(path)))
        || Array.from(path).length > limits.maxPathChars) throw new Error("path exceeds runtime tree contract");
    const folded = path.replace(/[A-Z]/g, char => char.toLowerCase());
    if (seen.has(folded)) throw new Error("duplicate runtime tree path after case fold");
    seen.add(folded);
    entries++;
    if (entries > limits.maxEntries) throw new Error("runtime entry limit exceeded");
    if (size !== undefined) {
      if (!Number.isSafeInteger(size) || size < 0) throw new Error("invalid runtime file size");
      files++; bytes += size;
      if (files > limits.maxFiles || size > limits.maxFileBytes || !Number.isSafeInteger(bytes)
          || bytes > limits.maxTotalBytes) throw new Error("runtime file/byte limit exceeded");
    }
  };
}
