#!/usr/bin/env node
'use strict';
// Launcher only: select an already-installed optional package and forward stdio.
const path = require('node:path');
const { spawn } = require('node:child_process');
const supported = new Set(['darwin-arm64', 'darwin-x64', 'win32-x64', 'linux-x64', 'linux-arm64']);
const platform = `${process.platform}-${process.arch}`;
if (!supported.has(platform)) {
  console.error(`gcu: unsupported platform ${platform}`);
  process.exit(1);
}
let binary;
try {
  const manifest = require.resolve(`@grok-computer-use/${platform}/package.json`);
  binary = path.join(path.dirname(manifest), 'bin', process.platform === 'win32' ? 'gcu.exe' : 'gcu');
} catch {
  console.error(`gcu: missing @grok-computer-use/${platform}; reinstall with optional dependencies enabled`);
  process.exit(1);
}
const child = spawn(binary, process.argv.slice(2), { stdio: 'inherit', windowsHide: false, shell: false });
for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) {
  process.on(signal, () => child.kill(signal));
}
child.on('error', () => { console.error('gcu: cannot execute the installed native binary'); process.exit(1); });
child.on('exit', (code, signal) => {
  if (signal && process.platform !== 'win32') {
    process.removeAllListeners(signal);
    process.kill(process.pid, signal);
  } else process.exit(code ?? 1);
});
