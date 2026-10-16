// CI static/syntax + mocked-policy checks. No GNOME Shell or KWin is started.
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {spawnSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
import {dirname, join} from 'node:path';

const root = dirname(fileURLToPath(import.meta.url));
function run(args, options = {}) {
    const result = spawnSync(process.execPath, args, {encoding: 'utf8', timeout: 10000, ...options});
    assert.equal(result.status, 0, `${args.join(' ')}\n${result.stdout}\n${result.stderr}`);
}
for (const file of ['gnome-shell/extension.js', 'gnome-shell/inventory.mjs', 'kwin/contents/code/main.js']) {
    run(['--input-type=module', '--check'], {input: readFileSync(join(root, file), 'utf8')});
}
run(['--test', join(root, 'gnome-shell/inventory.test.mjs'), join(root, 'kwin/script.test.mjs')]);
const metadata = JSON.parse(readFileSync(join(root, 'gnome-shell/metadata.json'), 'utf8'));
assert.equal(metadata['settings-schema'], 'org.grok.computer-use.windows');
assert.equal(JSON.parse(readFileSync(join(root, 'kwin/metadata.json'), 'utf8'))['X-Plasma-MainScript'], 'code/main.js');
const extension = readFileSync(join(root, 'gnome-shell/extension.js'), 'utf8');
assert.ok(extension.includes('type="a(stssub)"')); // matches Rust WireWindow
assert.ok(extension.includes('invocation.get_sender()'));
assert.ok(extension.includes('Meta.WindowClientType.WAYLAND'));
const schema = readFileSync(join(root, 'gnome-shell/schemas/org.grok.computer-use.windows.gschema.xml'), 'utf8');
assert.ok(schema.includes("<default>''</default>")); // activation disabled by default
console.log('Compositor scripts: syntax/static/mocked checks passed; native runtime not_run');
