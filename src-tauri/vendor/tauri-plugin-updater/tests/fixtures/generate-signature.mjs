// Generate public-only test vectors. This key is ephemeral, never written,
// never logged, and is NOT the product/release signing key.
import { generateKeyPairSync, randomBytes, createHash, sign } from 'node:crypto';
import { writeFileSync } from 'node:fs';
const { privateKey, publicKey } = generateKeyPairSync('ed25519');
const keyId = randomBytes(8);
const publicPacket = Buffer.concat([
  Buffer.from('Ed'), keyId,
  publicKey.export({ type: 'spki', format: 'der' }).subarray(-32),
]);
const payload = 'signed updater format-rejection fixture; not an executable';
const signature = sign(null, createHash('blake2b512').update(payload).digest(), privateKey);
const trustedComment = 'owned test fixture only';
const globalSignature = sign(null, Buffer.concat([signature, Buffer.from(trustedComment)]), privateKey);
const packet = Buffer.concat([Buffer.from('ED'), keyId, signature]);
const publicText = `untrusted comment: ephemeral updater fixture public key\n${publicPacket.toString('base64')}`;
const signatureText = `untrusted comment: ephemeral updater fixture signature\n${packet.toString('base64')}\ntrusted comment: ${trustedComment}\n${globalSignature.toString('base64')}`;
writeFileSync(new URL('./signed-format-rejection.json', import.meta.url), JSON.stringify({
  description: 'Public-only ephemeral-key fixture. Valid signature, deliberately invalid installer format.',
  payload,
  publicKey: Buffer.from(publicText).toString('base64'),
  signature: Buffer.from(signatureText).toString('base64'),
}, null, 2) + '\n');
console.log('Wrote public key, signature and inert payload; private key never persisted.');
