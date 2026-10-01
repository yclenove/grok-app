import { cp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(fileURLToPath(new URL("../..", import.meta.url)));
const source = path.resolve(process.argv[2] ?? path.join(root, "tools", "computer-use-extension"));
const destination = path.resolve(process.argv[3] ?? path.join(root, ".cu-installed-extension"));

await rm(destination, { recursive: true, force: true });
await mkdir(destination, { recursive: true });
await cp(source, destination, { recursive: true });

const manifestPath = path.join(destination, "manifest.json");
const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
// The production source bundle carries a public key for its stable ID. The
// installed-lifecycle probe must generate its own temporary signing key, so
// omit that field and never pretend to test the production private key.
delete manifest.key;
await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`, "utf8");
console.log(destination);
