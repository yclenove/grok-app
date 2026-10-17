#!/usr/bin/env python3
"""Offline release-script checks; all file mutations are in a temporary checkout."""
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location('install_smoke', ROOT / 'scripts/install-smoke/run.py')
SMOKE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SMOKE)


class Packages(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='gcu-packages-test-')
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.workspace = self.root / 'computer-use'
        shutil.copytree(ROOT / 'packages', self.workspace / 'packages', ignore=shutil.ignore_patterns('__pycache__'))
        shutil.copyfile(ROOT / 'Cargo.toml', self.workspace / 'Cargo.toml')
        shutil.copyfile(ROOT / 'Cargo.lock', self.workspace / 'Cargo.lock')

    def run_script(self, script, **env):
        return subprocess.run([sys.executable, str(self.workspace / 'packages' / script)], env={**os.environ, **env}, capture_output=True, text=True)

    def test_version_manual_preview_updates_only_disposable_workspace(self):
        result = self.run_script('version.py', RELEASE_EVENT='workflow_dispatch', REQUESTED_TAG='cu-v1.2.3-rc.1')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.workspace / 'target/release-version').read_text(), '1.2.3-rc.1')
        self.assertIn('version = "1.2.3-rc.1"', (self.workspace / 'Cargo.toml').read_text())
        self.assertIn('name = "xtask"\nversion = "1.2.3-rc.1"', (self.workspace / 'Cargo.lock').read_text())

    def test_version_invalid_input_and_tag_mismatch_write_nothing(self):
        for tag in ['cu-v1.2.3/evil', 'cu-v1.2.3\n', 'cu-v01.2.3', 'cu-v1.2.3-01']:
            with self.subTest(tag=tag):
                self.assertNotEqual(self.run_script('version.py', RELEASE_EVENT='workflow_dispatch', REQUESTED_TAG=tag).returncode, 0)
                self.assertFalse((self.workspace / 'target').exists())
        self.assertNotEqual(self.run_script('version.py', RELEASE_EVENT='push', RELEASE_REF='cu-v9.8.7').returncode, 0)
        self.assertFalse((self.workspace / 'target').exists())

    def test_linux_metadata_uses_actual_target_assets_and_rejects_double_append(self):
        crate = self.workspace / 'crates/gcu-cli'
        crate.mkdir(parents=True)
        shutil.copyfile(ROOT / 'crates/gcu-cli/Cargo.toml', crate / 'Cargo.toml')
        binary = self.workspace / 'target/aarch64-unknown-linux-gnu/release/gcu'
        binary.parent.mkdir(parents=True)
        binary.write_bytes(b'fixture')
        (self.root / 'LICENSE').write_text('MIT')
        result = self.run_script('linux/prepare.py', GCU_TARGET='aarch64-unknown-linux-gnu')
        self.assertEqual(result.returncode, 0, result.stderr)
        import tomllib
        data = tomllib.loads((crate / 'Cargo.toml').read_text())
        self.assertEqual(data['package']['metadata']['deb']['assets'][0][0], str(binary))
        self.assertEqual(data['package']['metadata']['generate-rpm']['assets'][0]['source'], str(binary))
        self.assertNotEqual(self.run_script('linux/prepare.py', GCU_TARGET='aarch64-unknown-linux-gnu').returncode, 0)

    def test_rpm_output_directory_normalizes_without_changing_bytes(self):
        target = self.workspace / 'target'
        (target / 'cu-rpm').mkdir(parents=True)
        (target / 'cu-assets').mkdir()
        (target / 'release-version').write_text('1.2.3')
        (target / 'cu-rpm/native-1.2.3-1.aarch64.rpm').write_bytes(b'unchanged rpm fixture')
        result = self.run_script('linux/finish.py', GCU_TARGET='aarch64-unknown-linux-gnu')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((target / 'cu-assets/gcu-1.2.3-aarch64-unknown-linux-gnu.rpm').read_bytes(), b'unchanged rpm fixture')
        self.assertNotEqual(self.run_script('linux/finish.py', GCU_TARGET='aarch64-unknown-linux-gnu').returncode, 0)

    def test_smoke_rejects_corruption_missing_checksum_and_traversal(self):
        archive = self.root / 'test.tar.gz'
        archive.write_bytes(b'fixture')
        sums = self.root / 'SHA256SUMS'
        sums.write_text(f'{hashlib.sha256(b"fixture").hexdigest()}  test.tar.gz\n')
        SMOKE.verify(archive, sums)
        archive.write_bytes(b'corrupt')
        with self.assertRaises(ValueError):
            SMOKE.verify(archive, sums)
        sums.write_text(f'{"a" * 64}  other.zip\n')
        with self.assertRaises(ValueError):
            SMOKE.verify(archive, sums)
        with tarfile.open(archive, 'w:gz') as stream:
            member = tarfile.TarInfo('../escaped')
            member.size = 1
            stream.addfile(member, io.BytesIO(b'x'))
        with self.assertRaises(ValueError):
            SMOKE.extract(archive, self.root / 'extract')
        self.assertFalse((self.root / 'escaped').exists())


    @unittest.skipIf(os.name == 'nt', 'POSIX fixture executable')
    def test_launcher_forwards_exact_argv_stdio_and_exit_without_shell(self):
        machine = subprocess.run(['node', '-p', 'process.platform+"-"+process.arch'], capture_output=True, text=True, check=True).stdout.strip()
        launcher = self.root / 'gcu.cjs'
        shutil.copyfile(ROOT / 'packages/npm/gcu.cjs', launcher)
        package = self.root / 'node_modules/@grok-computer-use' / machine
        (package / 'bin').mkdir(parents=True)
        (package / 'package.json').write_text('{}')
        binary = package / 'bin/gcu'
        binary.write_text('#!/usr/bin/env node\nprocess.stdout.write(JSON.stringify(process.argv.slice(2)));process.stderr.write("fixture");process.exit(7);')
        binary.chmod(0o755)
        args = ['space arg', '$(touch dangerous)', 'a;b', '中文']
        result = subprocess.run(['node', str(launcher), *args], cwd=self.root, capture_output=True, text=True)
        self.assertEqual(result.returncode, 7, result.stderr)
        self.assertEqual(json.loads(result.stdout), args)
        self.assertEqual(result.stderr, 'fixture')
        self.assertFalse((self.root / 'dangerous').exists())

    def test_launcher_missing_platform_fails_locally_with_guidance(self):
        launcher = self.root / 'gcu.cjs'
        shutil.copyfile(ROOT / 'packages/npm/gcu.cjs', launcher)
        result = subprocess.run(['node', str(launcher), 'version'], env={**os.environ, 'NODE_PATH': ''}, capture_output=True, text=True)
        self.assertEqual(result.returncode, 1)
        self.assertIn('reinstall with optional dependencies', result.stderr)

    @unittest.skipIf(os.name == 'nt', 'POSIX fixture executable')
    def test_native_archive_npm_package_preserves_execute_bit_and_signed_payload(self):
        env = {'GCU_TARGET': 'aarch64-unknown-linux-gnu', 'GCU_NPM': 'linux-arm64'}
        target = self.workspace / 'target'
        payload = target / 'cu-stage/payload'
        payload.mkdir(parents=True)
        binary = payload / 'gcu'
        binary.write_text('#!/bin/sh\nprintf "gcu 1.2.3\\n"\n')
        binary.chmod(0o755)
        (payload / 'release.json').write_text('{"version":"1.2.3"}')
        (target / 'release-version').write_text('1.2.3')
        (target / 'cu-stage/sbom.cdx.json').write_text('{"bomFormat":"CycloneDX"}')
        package = target / 'cu-stage/npm/linux-arm64'
        package.mkdir(parents=True)
        (package / 'package.json').write_text(json.dumps({'name': '@grok-computer-use/linux-arm64', 'version': '1.2.3', 'files': ['bin']}))
        graph = self.workspace / 'crates/gcu-cli/bom.json'
        graph.parent.mkdir(parents=True)
        graph.write_text('{"bomFormat":"CycloneDX","components":[{"name":"fixture"}]}')
        # Isolate npm cache writes to this temporary test directory too.
        result = self.run_script('archive.py', **env, npm_config_cache=str(self.root / 'cache'))
        self.assertEqual(result.returncode, 0, result.stderr)
        assets = target / 'cu-assets'
        for path, member in [(assets / 'gcu-1.2.3-aarch64-unknown-linux-gnu.tar.gz', 'gcu'), (assets / 'grok-computer-use-linux-arm64-1.2.3.tgz', 'package/bin/gcu')]:
            with tarfile.open(path) as stream:
                self.assertTrue(stream.getmember(member).mode & 0o111)
                self.assertEqual(stream.extractfile(member).read(), binary.read_bytes())


if __name__ == '__main__':
    unittest.main()
