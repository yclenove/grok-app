#!/usr/bin/env python3
"""Append package metadata to a disposable CI source checkout before packaging."""
from pathlib import Path
import json
import os

workspace = Path(__file__).resolve().parents[2]
manifest = workspace / 'crates/gcu-cli/Cargo.toml'
text = manifest.read_text()
if '[package.metadata.deb]' in text or '[package.metadata.generate-rpm]' in text:
    raise SystemExit('Linux package metadata already present')
metadata = (workspace / 'packages/linux/metadata.toml').read_text()
for name, path in {
    'BINARY': workspace / 'target' / os.environ['GCU_TARGET'] / 'release/gcu',
    'README': workspace / 'packages/README.md',
    'LICENSE': workspace.parent / 'LICENSE',
}.items():
    if not path.is_file():
        raise SystemExit(f'Missing packaging asset: {name}')
    metadata = metadata.replace(f'"@{name}@"', json.dumps(str(path)))
text = text.replace('name = "gcu-cli"', 'name = "gcu-cli"\ndescription = "Native computer use MCP server"', 1)
manifest.write_text(text + '\n' + metadata)
