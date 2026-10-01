#!/usr/bin/env node
// Attach the exact signed bytes, not a parsed/reserialized document. Runtime
// independently verifies the publisher signature; this tool does not read keys.
import { readFileSync } from 'node:fs';
import { resolve, join, basename } from 'node:path';
import { pathToFileURL } from 'node:url';
import { hashFile, validateInventory } from './windows-install-inventory.mjs';

export async function attachInventories(latest,assets) {
  if(!latest.platforms || typeof latest.version !== 'string') throw new Error('invalid latest manifest');
  for(const [key,platform] of Object.entries(latest.platforms)) {
    if(!key.startsWith('windows-')) continue;
    const name=decodeURIComponent(basename(new URL(platform.url).pathname));
    if(!name || name.includes('/') || name.includes('\\') || name.includes(':') || name === '.' || name === '..') throw new Error('invalid artifact basename');
    const file=join(resolve(assets),name);
    await hashFile(`${file}.install-inventory.json`);
    await hashFile(`${file}.install-inventory.json.sig`);
    const document=readFileSync(`${file}.install-inventory.json`,'utf8');
    const signature=readFileSync(`${file}.install-inventory.json.sig`,'utf8').trim();
    if(Buffer.byteLength(document)>256*1024 || !signature || signature.length>8192) throw new Error('invalid signed inventory size');
    const inventory=validateInventory(JSON.parse(document));
    const base=`windows-${inventory.arch}`,kind=inventory.kind.toLowerCase();
    const suffixes=inventory.kind === 'Nsis' ? ['.exe','.nsis.zip'] : ['.msi','.msi.zip'];
    if(inventory.version !== latest.version || inventory.target !== 'windows' || ![base,`${base}-${kind}`].includes(key)
      || !suffixes.some(suffix=>name.toLowerCase().endsWith(suffix))
      || inventory.payload_sha256 !== (await hashFile(file)).sha256) throw new Error('inventory does not match published candidate');
    platform.windows_install_inventory={document,signature};
  }
  return latest;
}

if(process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    if(process.argv.length!==4) throw new Error('usage: attach-windows-install-inventory.mjs latest.json assets-dir');
    process.stdout.write(JSON.stringify(await attachInventories(JSON.parse(readFileSync(process.argv[2],'utf8')),process.argv[3]),null,2)+'\n');
  } catch(error) {process.stderr.write(`Install inventory: ${error.message}\n`);process.exitCode=1;}
}
