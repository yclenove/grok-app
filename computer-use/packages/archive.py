#!/usr/bin/env python3
"""Archive signed staging payload and resolved SBOM without following symlinks."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import zipfile

workspace = Path(__file__).resolve().parents[1]
os.chdir(workspace)
version = Path('target/release-version').read_text()
target = os.environ['GCU_TARGET']
npm = os.environ['GCU_NPM']
stage = Path('target/cu-stage')
assets = Path('target/cu-assets')
assets.mkdir(parents=True, exist_ok=False)
payload = stage / 'payload'
exe = payload / ('gcu.exe' if npm.startswith('win32') else 'gcu')
subprocess.run([str(exe.resolve()), 'version'], check=True)
if npm.startswith('darwin'):
    if exe.read_bytes() != (payload / 'Grok Computer Use.app/Contents/MacOS/gcu').read_bytes():
        raise SystemExit('CLI/helper executable identity differs')
    subprocess.run(['codesign', '--verify', '--deep', '--strict', str(payload / 'Grok Computer Use.app')], check=True)
if npm.startswith('win32'):
    archive = assets / f'gcu-{version}-{target}.zip'
    with zipfile.ZipFile(archive, 'w', compression=zipfile.ZIP_DEFLATED) as stream:
        for file in sorted(payload.rglob('*')):
            if file.is_file():
                stream.write(file, file.relative_to(payload))
else:
    archive = assets / f'gcu-{version}-{target}.tar.gz'
    # Native modes and stapled .app tickets survive tar and npm pack.
    with tarfile.open(archive, 'w:gz') as stream:
        for file in sorted(payload.iterdir()):
            stream.add(file, arcname=file.name, recursive=True)
shutil.copyfile(stage / 'sbom.cdx.json', assets / f'gcu-{version}-{target}.lock.cdx.json')
# Accept cargo-cyclonedx's standard or crate-prefixed JSON filename.
graphs = []
for candidate in Path('crates/gcu-cli').glob('*.json'):
    if json.loads(candidate.read_text()).get('bomFormat') == 'CycloneDX':
        graphs.append(candidate)
if len(graphs) != 1:
    raise SystemExit(f'Expected one generated cargo-cyclonedx JSON, got {len(graphs)}')
graph = json.loads(graphs[0].read_text())
if graph.get('bomFormat') != 'CycloneDX' or not graph.get('components'):
    raise SystemExit('Invalid or empty resolved CycloneDX graph')
shutil.copyfile(graphs[0], assets / f'gcu-{version}-{target}.cdx.json')
package = stage / 'npm' / npm
shutil.copytree(payload, package / 'bin')
subprocess.run(['npm.cmd' if os.name == 'nt' else 'npm', 'pack', str(package.resolve()), '--ignore-scripts', '--offline', '--pack-destination', str(assets)], check=True)
# Keep only reviewable metadata unpacked: Actions artifacts strip executable modes.
metadata = assets / 'npm' / npm
metadata.mkdir(parents=True)
shutil.copyfile(package / 'package.json', metadata / 'package.json')
