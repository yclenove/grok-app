#!/usr/bin/env python3
"""Normalize the generated RPM output filename without rebuilding or stripping."""
import os
from pathlib import Path
import shutil

workspace = Path(__file__).resolve().parents[2]
output = workspace / 'target/cu-rpm'
candidates = [output] if output.is_file() else list(output.glob('*.rpm'))
if len(candidates) != 1 or candidates[0].stat().st_size == 0:
    raise SystemExit('Expected exactly one generated RPM')
version = (workspace / 'target/release-version').read_text()
dest = workspace / 'target/cu-assets' / f'gcu-{version}-{os.environ["GCU_TARGET"]}.rpm'
if dest.exists():
    raise SystemExit('RPM destination already exists')
shutil.move(candidates[0], dest)
