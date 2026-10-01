// Source UI strings are owned by src/i18n; Chrome consumes generated catalogs.
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { createHash } from "node:crypto";
import path from "node:path";
const root = fileURLToPath(new URL("../", import.meta.url));
const extension = path.join(root, "tools/computer-use-extension");
const manifest = JSON.parse(await readFile(path.join(extension, "manifest.json"), "utf8"));
const id = [...createHash("sha256").update(Buffer.from(manifest.key, "base64")).digest().subarray(0, 16)]
  .map(byte => String.fromCharCode(97 + (byte >> 4), 97 + (byte & 15))).join("");
if (id !== "bgegbabkegkanjbmjbeaockdijnbkjgi") throw new Error("Extension identity changed");
const locales = ["de", "en", "es", "fil", "fr", "id", "it", "ja", "ko", "pt-BR", "ru", "ta", "uk", "zh", "zh-TW"];
const chromeLocales = { fil: "tl", "pt-BR": "pt_BR", zh: "zh_CN", "zh-TW": "zh_TW" };
const aliases = {
  extensionDescription: "cu.extension.description", pairingHint: "cu.panel.pairingHint",
  appAddress: "cu.panel.pairingEndpoint", pairingCode: "cu.extension.code", pair: "cu.extension.pair",
  paired: "cu.extension.paired", unpaired: "cu.extension.unpaired", forget: "cu.panel.revokePairing",
  shareCurrentTab: "cu.extension.shareCurrentTab", unshareCurrentTab: "cu.extension.unshareCurrentTab",
  sharedCurrentTab: "cu.extension.sharedCurrentTab", unsharedCurrentTab: "cu.extension.unsharedCurrentTab",
  shareUnavailable: "cu.extension.shareUnavailable",
};
const errorKeys = ["invalidAddress", "invalidCode", "challengeChanged", "pairingRejected", "busy", "cancelled", "connectionFailed"];
for (const locale of locales) {
  const source = await readFile(path.join(root, "src/i18n/messages", locale, "computer-use.ts"), "utf8");
  const strings = Object.fromEntries([...source.matchAll(/^\s*"([^"]+)":\s*("(?:\\.|[^"\\])*"),?\s*$/gm)]
    .map(([, key, value]) => [key, JSON.parse(value)]));
  const catalog = { extensionName: { message: "Grok Computer Use" } };
  for (const [key, from] of Object.entries(aliases)) {
    if (!strings[from]) throw new Error(`Missing ${locale}/${from}`);
    catalog[key] = { message: strings[from] };
  }
  for (const key of errorKeys) catalog[key] = { message: strings["cu.extension.retry"] };
  const destination = path.join(extension, "_locales", chromeLocales[locale] || locale, "messages.json");
  const content = JSON.stringify(catalog, null, 2) + "\n";
  if (process.argv.includes("--check")) {
    if (await readFile(destination, "utf8") !== content) throw new Error(`Stale extension locale: ${locale}`);
  } else {
    await mkdir(path.dirname(destination), { recursive: true });
    await writeFile(destination, content);
  }
}
console.log(`Computer Use extension: ${locales.length} locales; identity ${id}`);
