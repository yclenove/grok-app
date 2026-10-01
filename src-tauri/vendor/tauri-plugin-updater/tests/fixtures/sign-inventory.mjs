// PUBLIC TEST ONLY deterministic key. This script is only invoked by tests.
import { createPrivateKey, createPublicKey, createHash, sign } from 'node:crypto';
import { readFileSync } from 'node:fs';
const key = createPrivateKey({ format: 'der', type: 'pkcs8', key: Buffer.concat([
  Buffer.from('302e020100300506032b657004220420', 'hex'), Buffer.from(Array.from({length:32}, (_, i) => i)),
]) });
const id = Buffer.from('TESTONLY');
const publicKey = Buffer.from(`untrusted comment: PUBLIC TEST ONLY inventory key\n${Buffer.concat([
  Buffer.from('Ed'), id, createPublicKey(key).export({type:'spki',format:'der'}).subarray(-32),
]).toString('base64')}`).toString('base64');
const signature = sign(null, createHash('blake2b512').update(readFileSync(process.argv[2])).digest(), key);
const comment = 'PUBLIC TEST ONLY install inventory';
const packet = Buffer.concat([Buffer.from('ED'), id, signature]);
const global = sign(null, Buffer.concat([signature, Buffer.from(comment)]), key);
process.stdout.write(JSON.stringify({ publicKey, signature: Buffer.from(`untrusted comment: PUBLIC TEST ONLY\n${packet.toString('base64')}\ntrusted comment: ${comment}\n${global.toString('base64')}`).toString('base64') }));
