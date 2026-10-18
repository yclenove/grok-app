#!/usr/bin/env python3
"""Disposable-VM archive smoke; never substitutes fake for native doctor/MCP."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import queue
import shutil
import subprocess
import sys
import tarfile
import tempfile
import threading
import time
import zipfile


def verify(archive, sums):
    expected = {}
    for line in sums.read_text().splitlines():
        digest, name = line.split('  ', 1)
        if len(digest) != 64 or any(c not in '0123456789abcdef' for c in digest) or Path(name).name != name:
            raise ValueError('Invalid checksum record')
        if name in expected:
            raise ValueError('Duplicate checksum record')
        expected[name] = digest
    with archive.open('rb') as stream:
        actual = hashlib.file_digest(stream, 'sha256').hexdigest()
    if actual != expected.get(archive.name):
        raise ValueError('Archive digest missing or mismatched')


def extract(archive, dest):
    def safe(name):
        if '\\' in name or ':' in name or name.startswith('/') or '..' in Path(name).parts:
            raise ValueError('Unsafe archive path')
    if archive.suffix == '.zip':
        with zipfile.ZipFile(archive) as stream:
            for member in stream.infolist():
                safe(member.filename)
                if (member.external_attr >> 16) & 0o170000 == 0o120000:
                    raise ValueError('Symlink in archive')
            stream.extractall(dest)
    else:
        with tarfile.open(archive, 'r:gz') as stream:
            members = stream.getmembers()
            for member in members:
                safe(member.name)
                if not (member.isfile() or member.isdir()):
                    raise ValueError('Special file in archive')
            stream.extractall(dest, members=members, filter='data')


def mcp(exe, env, cwd):
    messages = queue.Queue()
    with tempfile.TemporaryFile() as stderr:
        child = subprocess.Popen([str(exe), 'mcp'], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=stderr, text=True, env=env, cwd=cwd)
        def read():
            for line in child.stdout:
                messages.put(line)
            messages.put(None)
        reader = threading.Thread(target=read, daemon=True)
        reader.start()
        def send(value):
            child.stdin.write(json.dumps(value) + '\n')
            child.stdin.flush()
        def response(identifier):
            deadline = time.monotonic() + 20
            for _ in range(100):
                line = messages.get(timeout=max(0, deadline - time.monotonic()))
                if line is None:
                    raise RuntimeError('Native MCP exited before replying')
                value = json.loads(line)
                if value.get('id') == identifier:
                    if 'error' in value or 'result' not in value:
                        raise RuntimeError('Native MCP request failed')
                    return value['result']
            raise RuntimeError('Native MCP response limit exceeded')
        try:
            send({'jsonrpc': '2.0', 'id': 1, 'method': 'initialize', 'params': {'protocolVersion': '2025-03-26', 'capabilities': {}, 'clientInfo': {'name': 'gcu-install-smoke', 'version': '1'}}})
            response(1)
            send({'jsonrpc': '2.0', 'method': 'notifications/initialized'})
            send({'jsonrpc': '2.0', 'id': 2, 'method': 'tools/list', 'params': {}})
            tools = response(2).get('tools', [])
            if not any(t.get('name') == 'capabilities' for t in tools):
                raise RuntimeError('Missing capabilities tool')
            send({'jsonrpc': '2.0', 'id': 3, 'method': 'tools/call', 'params': {'name': 'capabilities', 'arguments': {}}})
            if response(3).get('isError'):
                raise RuntimeError('Native capabilities failed')
        finally:
            child.stdin.close()
            try:
                child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait(timeout=5)
            child.stdout.close()
            reader.join(timeout=5)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--archive', type=Path, required=True)
    parser.add_argument('--sums', type=Path, required=True)
    parser.add_argument('--package-only', action='store_true', help='Validate package/config/uninstall only; native doctor/MCP not_run')
    parser.add_argument('--grok', type=Path, help='Optional installed Grok CLI in a clean VM; no credentials are used')
    args = parser.parse_args()
    verify(args.archive, args.sums)
    with tempfile.TemporaryDirectory(prefix='gcu-install-smoke-') as temp:
        root = Path(temp)
        dest, home, project = (root / name for name in ['install', 'home', 'project'])
        for directory in [dest, home, project]:
            directory.mkdir()
        extract(args.archive, dest)
        exe = dest / ('gcu.exe' if sys.platform == 'win32' else 'gcu')
        env = os.environ.copy()
        # Task-specific disposable environment; no changes to the user's home/config.
        env.update({'HOME': str(home), 'USERPROFILE': str(home), 'APPDATA': str(home / 'AppData'), 'XDG_CONFIG_HOME': str(home / 'config'), 'XDG_STATE_HOME': str(home / 'state'), 'XDG_RUNTIME_DIR': str(home / 'run'), 'LOCALAPPDATA': str(home / 'LocalAppData'), 'GROK_HOME': str(home / '.grok'), 'GCU_HOME': str(home), 'GCU_PROJECT': str(project)})
        (home / 'run').mkdir(mode=0o700)
        for key in list(env):
            if any(word in key.upper() for word in ['TOKEN', 'SECRET', 'PASSWORD', 'API_KEY']) or key.startswith(('GCU_APPROVAL', 'GCU_ALLOW_FAKE', 'GCU_LOG_')):
                env.pop(key)
        def run(*command):
            return subprocess.run(command, env=env, cwd=project, timeout=30, check=True, capture_output=True, text=True).stdout
        release = json.loads((dest / 'release.json').read_text())
        machine = platform.machine().lower()
        arch = {'arm64': 'aarch64', 'aarch64': 'aarch64', 'amd64': 'x86_64', 'x86_64': 'x86_64'}.get(machine)
        suffix = {'darwin': 'apple-darwin', 'linux': 'unknown-linux-gnu', 'win32': 'pc-windows-msvc'}.get(sys.platform)
        if not arch or not suffix or release.get('target') != f'{arch}-{suffix}':
            raise RuntimeError('Archive target does not match the VM; executable was not started')
        if run(str(exe), 'version').strip() != f'gcu {release["version"]}':
            raise RuntimeError('Version does not match archive metadata')
        if sys.platform == 'darwin':
            app = dest / 'Grok Computer Use.app'
            run('codesign', '--verify', '--deep', '--strict', str(app))
            if exe.read_bytes() != (app / 'Contents/MacOS/gcu').read_bytes():
                raise RuntimeError('CLI/helper signature identity differs')
            if not args.package_only:
                run('open', '-Ra', str(app))
        config_args = ['grok', '--scope', 'project', '--home', str(home), '--project', str(project), '--gcu', str(exe), '--yes']
        try:
            run(str(exe), 'install', *config_args)
            run(str(exe), 'install', *config_args)  # idempotent
            if args.grok:
                # Lists configured servers without contacting a model or an account.
                run(str(args.grok.resolve()), 'mcp', 'list')
            if args.package_only:
                print('native doctor/MCP: not_run (--package-only)')
            else:
                doctor = json.loads(run(str(exe), 'doctor', '--json'))
                if doctor.get('driver') == 'fake':
                    raise RuntimeError('Native smoke must not use fake driver')
                mcp(exe, env, project)
        finally:
            run(str(exe), 'uninstall', *config_args)
            plan = json.loads(run(str(exe), 'uninstall', *config_args, '--dry-run', '--json'))
            if not plan.get('noop'):
                raise RuntimeError('MCP registration remained after uninstall')
        shutil.rmtree(dest)
        if dest.exists():
            raise RuntimeError('Archive installation remained after removal')
    print('PASS: checksums, extraction, version, project registration, uninstall' + ('' if args.package_only else ', native doctor/MCP'))


if __name__ == '__main__':
    main()
