"""Check/apply the experimental patch to an exact, explicit source directory.

No downloads, package installation, system paths or Shell restart. Default is
check-only. Requires the standard `patch` program; refuses drift instead of fuzz.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def verify(root, manifest, after=False):
    if digest(root / 'meson.build') != manifest['mesonSha256']:
        raise ValueError('Not the exact pinned Mutter 46.2 source tree')
    for entry in manifest['files']:
        relative = Path(entry['path'])
        if relative.is_absolute() or '..' in relative.parts:
            raise ValueError('Invalid manifest path')
        target = root / relative
        for item in [target, *target.parents]:
            if item == root:
                break
            if item.is_symlink():
                raise ValueError('Refusing symlinked source path: ' + str(target))
        expected = entry['afterSha256' if after else 'beforeSha256']
        if expected is None:
            if target.exists():
                raise ValueError('New source already exists: ' + str(target))
        elif not target.is_file() or digest(target) != expected:
            raise ValueError('Source differs from pinned hash: ' + str(target))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source_root', type=Path)
    parser.add_argument('--apply', action='store_true', help='write only this explicit source tree')
    parser.add_argument('--patch-set', choices=('physical-input', 'eis-region-binding'), default='physical-input')
    args = parser.parse_args()
    here = Path(__file__).resolve().parent
    manifest_name = 'mutter-46.2-sources.json' if args.patch_set == 'physical-input' else 'mutter-46.2-eis-region-binding-sources.json'
    manifest = json.loads((here / manifest_name).read_text())
    patch = here / ('mutter-46.2-' + args.patch_set + '.patch')
    if digest(patch) != manifest['patchSha256']:
        raise ValueError('Patch differs from manifest')
    if args.patch_set == 'physical-input':
        header = next(e for e in manifest['files'] if e['beforeSha256'] is None)
        if digest(here / 'physical-input-counter.h') != header['afterSha256']:
            raise ValueError('Tested counter header differs from patch header')
    root = args.source_root.resolve(strict=True)
    verify(root, manifest)
    command = ['patch', '--batch', '--forward', '--fuzz=0', '-p1', '-i', str(patch)]
    subprocess.run(command + ['--dry-run'], cwd=root, check=True)
    if args.apply:
        subprocess.run(command, cwd=root, check=True)
        verify(root, manifest, after=True)
    else:
        verify(root, manifest)  # check-only must not mutate input sources
    print(json.dumps({'upstreamTag': manifest['upstreamTag'], 'files': len(manifest['files']),
                      'applied': args.apply, 'verified': True, 'installed': False}))


if __name__ == '__main__':
    main()
