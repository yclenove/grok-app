#!/usr/bin/env node
// Build-time only. Hash the extracted installer image; never run the installer
// and never load signing keys. `tauri signer sign` signs the exact output bytes.
import { createHash } from 'node:crypto';
import { createReadStream, lstatSync, readdirSync, writeFileSync } from 'node:fs';
import { resolve, join, parse, relative, isAbsolute, sep } from 'node:path';
import { pathToFileURL } from 'node:url';

export const DOMAIN = 'grok-windows-install-inventory-v1';
export function relativePath(value) {
  if (typeof value !== 'string' || !value || value.length > 1024) throw new Error('invalid relative path length');
  for (const part of value.split('/')) {
    const stem = part.split('.')[0].toUpperCase();
    if (!part || part === '.' || part === '..' || part.length > 255 || /[. ]$/.test(part)
      || /[\x00-\x1f\\:*?"<>|~]/.test(part)
      || /^(CON|PRN|AUX|NUL|CLOCK\$|CONIN\$|CONOUT\$|COM[1-9¹²³]|LPT[1-9¹²³])$/.test(stem)) {
      throw new Error('unsafe Windows relative path');
    }
  }
}

export function validateInventory(value) {
  const fields = ['schema','domain','app_name','version','target','arch','kind','payload_sha256','executable','files'];
  if (value && Object.hasOwn(value,'completion')) fields.push('completion');
  if (!value || JSON.stringify(Object.keys(value).sort()) !== JSON.stringify(fields.sort())
    || value.schema !== 1 || value.domain !== DOMAIN
    || !['app_name','version','target','arch'].every(k=>typeof value[k] === 'string' && value[k])
    || !['Nsis','Msi'].includes(value.kind) || typeof value.payload_sha256 !== 'string' || !/^[a-f0-9]{64}$/.test(value.payload_sha256)
    || !Array.isArray(value.files) || !value.files.length || value.files.length > 4096) throw new Error('invalid inventory envelope');
  if (Object.hasOwn(value,'completion')) {
    const c=value.completion;
    const keys=c && Object.hasOwn(c,'failure_protocol') ? 'bundle_id,failure_protocol,install_scope,protocol' : 'bundle_id,install_scope,protocol';
    if (value.kind !== 'Nsis' || !c || Object.keys(c).sort().join(',') !== keys
      || (Object.hasOwn(c,'failure_protocol') && c.failure_protocol !== 'grok-nsis-install-failed-v1')
      || c.protocol !== 'grok-nsis-install-complete-v1' || c.install_scope !== 'currentUser'
      || typeof c.bundle_id !== 'string' || !/^[A-Za-z0-9][A-Za-z0-9._-]{0,199}$/.test(c.bundle_id)) throw new Error('invalid NSIS completion contract');
  }
  relativePath(value.executable);
  if (value.executable.includes('/')) throw new Error('executable must be a root file');
  const names=new Set(); let bytes=0;
  for (const file of value.files) {
    if (!file || Object.keys(file).sort().join(',') !== 'path,sha256,size') throw new Error('invalid file entry');
    relativePath(file.path);
    if (typeof file.sha256 !== 'string' || !/^[a-f0-9]{64}$/.test(file.sha256) || !Number.isSafeInteger(file.size) || file.size < 0 || file.size > 16*1024**3
      || names.has(file.path.toUpperCase())) throw new Error('invalid or duplicate file identity');
    names.add(file.path.toUpperCase()); bytes+=file.size;
    if (bytes > 64*1024**3) throw new Error('oversized installed image');
  }
  for (const file of value.files) {
    const parts=file.path.split('/');
    for(let i=1;i<parts.length;i++) if(names.has(parts.slice(0,i).join('/').toUpperCase())) throw new Error('file used as ancestor');
  }
  if (!value.files.some(f=>f.path === value.executable && f.size > 0)) throw new Error('missing main executable');
  if (Buffer.byteLength(JSON.stringify(value)) > 256*1024) throw new Error('oversized signed document');
  return value;
}

function plainAncestors(file, directory) {
  const absolute=resolve(file), base=parse(absolute).root;
  let current=base;
  const parts=absolute.slice(base.length).split(/[\\/]/).filter(Boolean);
  const count=parts.length-(directory ? 0 : 1);
  for(let i=0;i<count;i++) {
    current=join(current,parts[i]);
    const st=lstatSync(current);
    if(!st.isDirectory() || st.isSymbolicLink()) throw new Error('non-plain image ancestor');
  }
}

export async function hashFile(file) {
  plainAncestors(file,false);
  const before=lstatSync(file);
  if (!before.isFile() || before.isSymbolicLink() || before.nlink !== 1) throw new Error('non-plain or hardlinked inventory file');
  const hash=createHash('sha256'); let size=0;
  for await (const bytes of createReadStream(file)) {size+=bytes.length;hash.update(bytes);}
  const after=lstatSync(file); plainAncestors(file,false);
  if (!after.isFile() || after.isSymbolicLink() || after.nlink !== 1 || size !== before.size
    || before.size !== after.size || before.mtimeMs !== after.mtimeMs || before.ctimeMs !== after.ctimeMs
    || before.ino !== after.ino || before.dev !== after.dev) throw new Error('file changed during inventory hashing');
  return {size,sha256:hash.digest('hex')};
}

function requireOutsideImage(root, file) {
  const rel=relative(resolve(root),resolve(file));
  if (!isAbsolute(rel) && rel !== '..' && !rel.startsWith('..'+sep)) throw new Error('payload and output must remain outside installed image');
}

export async function createInventory({root,payload,appName,version,target,arch,kind,executable,nsisImage=false,completion}) {
  root=resolve(root); plainAncestors(root,true);
  requireOutsideImage(root,payload);
  if (nsisImage && kind !== 'Nsis') throw new Error('NSIS exclusions cannot apply to MSI');
  if (completion !== undefined && (!nsisImage || kind !== 'Nsis')) throw new Error('completion requires the explicit NSIS image contract');
  const files=[];
  async function walk(dir,prefix='') {
    for(const name of readdirSync(dir).sort()) {
      // NSIS extraction contains bootstrap plugins and a generated uninstaller;
      // neither is claimed as a stable installed application file. This is NOT
      // an uninstaller/registry/completion/rollback inventory.
      if (nsisImage && !prefix && (name === '$PLUGINSDIR' || name === 'uninstall.exe')) continue;
      const rel=prefix+name; relativePath(rel);
      const path=join(dir,name),stat=lstatSync(path);
      if(stat.isSymbolicLink()) throw new Error('image contains reparse link');
      if(stat.isDirectory()) await walk(path,rel+'/');
      else files.push({path:rel,...await hashFile(path)});
      if(files.length>4096) throw new Error('too many installed files');
    }
  }
  await walk(root);
  files.sort((a,b)=>Buffer.compare(Buffer.from(a.path),Buffer.from(b.path)));
  const artifact=await hashFile(payload);
  return validateInventory({schema:1,domain:DOMAIN,app_name:appName,version,target,arch,kind,
    payload_sha256:artifact.sha256,executable,files,...(completion===undefined?{}:{completion})});
}

if(process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    const required=['root','payload','app-name','version','target','arch','kind','executable','output'];
    const allowed=new Set([...required,'nsis-completion-bundle-id']);
    const options={};let nsisImage=false,nsisFailure=false;
    for(let i=2;i<process.argv.length;i++) {
      if(process.argv[i]==='--nsis-image' && !nsisImage){nsisImage=true;continue;}
      if(process.argv[i]==='--nsis-failure-receipt' && !nsisFailure){nsisFailure=true;continue;}
      const key=process.argv[i].replace(/^--/,'');
      if(!process.argv[i].startsWith('--') || !allowed.has(key) || options[key] || !process.argv[i+1] || process.argv[i+1].startsWith('--')) throw new Error('invalid or duplicate inventory option');
      options[key]=process.argv[++i];
    }
    if(required.some(k=>!options[k])) throw new Error('all explicit inventory options are required');
    requireOutsideImage(options.root,options.output);
    plainAncestors(options.output,false);
    const completion=options['nsis-completion-bundle-id']?{protocol:'grok-nsis-install-complete-v1',bundle_id:options['nsis-completion-bundle-id'],install_scope:'currentUser'}:undefined;
    if(nsisFailure) {
      if(!completion) throw new Error('failure receipt requires an explicit completion contract');
      completion.failure_protocol='grok-nsis-install-failed-v1';
    }
    const inventory=await createInventory({...options,appName:options['app-name'],nsisImage,completion});
    writeFileSync(options.output,JSON.stringify(inventory)+'\n',{flag:'wx'});
    process.stdout.write(JSON.stringify({files:inventory.files.length,output:options.output,signed:false})+'\n');
  } catch(error) {process.stderr.write(`Install inventory: ${error.message}\n`);process.exitCode=1;}
}
