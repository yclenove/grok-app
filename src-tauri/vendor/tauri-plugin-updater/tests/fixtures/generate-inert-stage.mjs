// Public-only signature vector. No product key or executable is read/generated.
// MZ merely passes the upstream format recognizer; payload is NOT runnable.
// The production recovery test always refuses cleanup before launch.
import { generateKeyPairSync, randomBytes, createHash, sign } from 'node:crypto';
import { writeFileSync } from 'node:fs';
const { privateKey, publicKey } = generateKeyPairSync('ed25519');
const keyId = randomBytes(8);
const publicPacket = Buffer.concat([Buffer.from('Ed'), keyId,
  publicKey.export({ type: 'spki', format: 'der' }).subarray(-32)]);
const payload = 'MZ inert owned staging fixture: deliberately not a PE executable; cleanup must always refuse launch.';
const signature = sign(null, createHash('blake2b512').update(payload).digest(), privateKey);
const trustedComment = 'owned inert staging test only';
const globalSignature = sign(null, Buffer.concat([signature, Buffer.from(trustedComment)]), privateKey);
const packet = Buffer.concat([Buffer.from('ED'), keyId, signature]);
const publicText = `untrusted comment: ephemeral fixture public key\n${publicPacket.toString('base64')}`;
const signatureText = `untrusted comment: ephemeral fixture signature\n${packet.toString('base64')}\ntrusted comment: ${trustedComment}\n${globalSignature.toString('base64')}`;
writeFileSync(new URL('./signed-inert-stage.json', import.meta.url), JSON.stringify({
  description: 'Valid test signature and MZ prefix, inert bytes. Never allow fixture cleanup/launch.',
  payload, publicKey: Buffer.from(publicText).toString('base64'),
  signature: Buffer.from(signatureText).toString('base64'),
}, null, 2) + '\n');
console.log('Wrote inert stage vector; ephemeral private key never persisted.');
