import { readFileSync, writeFileSync, readdirSync, existsSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../../src/i18n/messages/", import.meta.url));
for (const loc of readdirSync(root)) {
  const f = join(root, loc, "computer-use.ts");
  if (!existsSync(f)) continue;
  let t = readFileSync(f, "utf8");
  if (t.includes("side.tab.computer")) continue;
  const cu = (t.match(/"cu.side.tab": ("[^"]+")/) || [])[1] || '"Computer"';
  const pk = (t.match(/"cu.side.picker": ("[^"]+")/) || [])[1] || cu;
  const insert = `  "side.tab.computer": ${cu},\n  "side.picker.computer": ${pk},\n`;
  if (t.includes("} as const;")) {
    t = t.replace("} as const;", insert + "} as const;");
  } else {
    t = t.replace(/\n\};\n?$/, "\n" + insert + "};\n");
  }
  writeFileSync(f, t);
  console.log("patched", loc);
}
