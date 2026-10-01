// PUBLIC deterministic TEST key. Never use this key for a product release.
// Only test binaries include its public key; no product secrets are accessed.
import { createPrivateKey, createPublicKey, createHash, sign } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
const root = process.argv[2];
const exe = path.join(root, 'owned-dispatch-payload.exe');
if (process.argv[3] === 'inert') {
  writeFileSync(exe, 'MZ owned refusal fixture; deliberately not a PE executable');
} else {
  execFileSync('rustc', ['--edition', '2021', '--crate-name', 'owned_dispatch_payload',
    fileURLToPath(new URL('./dispatch-payload.rs', import.meta.url)), '-o', exe], { stdio: 'pipe' });
}
const privateKey = createPrivateKey({ format: 'der', type: 'pkcs8', key: Buffer.concat([
  Buffer.from('302e020100300506032b657004220420', 'hex'), Buffer.from(Array.from({length:32}, (_, i) => i)),
]) });
const keyId = Buffer.from('TESTONLY');
const pub = Buffer.concat([Buffer.from('Ed'), keyId, createPublicKey(privateKey).export({ type:'spki', format:'der' }).subarray(-32)]);
const publicKey = Buffer.from(`untrusted comment: PUBLIC TEST ONLY dispatch key\n${pub.toString('base64')}`).toString('base64');
const signature = sign(null, createHash('blake2b512').update(readFileSync(exe)).digest(), privateKey);
const comment = 'owned native process; not an installer';
const packet = Buffer.concat([Buffer.from('ED'), keyId, signature]);
const globalSignature = sign(null, Buffer.concat([signature, Buffer.from(comment)]), privateKey);
writeFileSync(path.join(root, 'signature.txt'), Buffer.from(`untrusted comment: PUBLIC TEST signature\n${packet.toString('base64')}\ntrusted comment: ${comment}\n${globalSignature.toString('base64')}`).toString('base64'));
process.stdout.write(publicKey);
