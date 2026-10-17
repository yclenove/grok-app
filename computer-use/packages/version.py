#!/usr/bin/env python3
"""Validate tag data and version the disposable build checkout, without network IO."""
import os
from pathlib import Path
import re
import subprocess
import sys
import tomllib

workspace = Path(__file__).resolve().parents[1]
manifest = workspace / 'Cargo.toml'
current = tomllib.loads(manifest.read_text())['workspace']['package']['version']
tag = os.environ.get('RELEASE_REF', '') if os.environ.get('RELEASE_EVENT') == 'push' else os.environ.get('REQUESTED_TAG') or f'cu-v{current}'
match = re.fullmatch(r'cu-v((?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?)', tag)
if not match or any(x.isdigit() and len(x) > 1 and x.startswith('0') for x in (match[2] or '').split('.')):
    raise SystemExit('Expected cu-v<semver>')
version = match[1]
if os.environ.get('RELEASE_EVENT') == 'push':
    # A tag must version the source before release, not silently relabel an old binary.
    if current != version:
        raise SystemExit('Tag version does not match workspace.package.version')
    subprocess.run([sys.executable, str(workspace.parent / 'scripts/changelog-for-release.py'), version], check=True, stdout=subprocess.DEVNULL)
else:
    # A manual artifact may preview the next version in this disposable checkout.
    text = manifest.read_text()
    text = re.sub(r'(\[workspace\.package\][\s\S]*?\nversion = ")[^"]+("\n)', lambda m: m[1] + version + m[2], text, count=1)
    manifest.write_text(text)
    lock = workspace / 'Cargo.lock'
    sections = lock.read_text().split('[[package]]')
    for i in range(1, len(sections)):
        if not re.search(r'^source\s*=', sections[i], re.MULTILINE):
            sections[i] = re.sub(r'\nversion = "[^"]+"', f'\nversion = "{version}"', sections[i], count=1)
    lock.write_text('[[package]]'.join(sections))
(workspace / 'target').mkdir(exist_ok=True)
(workspace / 'target/release-tag').write_text(tag)
(workspace / 'target/release-version').write_text(version)
