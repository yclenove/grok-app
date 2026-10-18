// Uses the repository's existing Vite dependency. No downloaded/bundled runtime.
import { build } from "vite";
import { cp, mkdir, rm } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("./", import.meta.url));
const outDir = fileURLToPath(new URL("./dist/", import.meta.url));
await rm(outDir, { recursive: true, force: true });
await mkdir(outDir, { recursive: true });
for (const entry of ["background", "content"]) {
  await build({ configFile: false, root, publicDir: false, build: {
    target: "chrome106", outDir, emptyOutDir: false, minify: false,
    lib: { entry: `${root}${entry}.ts`, formats: [entry === "content" ? "iife" : "es"], name: "GcuDom", fileName: () => `${entry}.js` },
  } });
}
await cp(`${root}manifest.json`, `${outDir}manifest.json`);
await cp(`${root}_locales`, `${outDir}_locales`, { recursive: true });
